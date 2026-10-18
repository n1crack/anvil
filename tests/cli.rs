//! End-to-end tests of the `anvil` binary. Each test gets its own ANVIL_HOME in a
//! temp folder and fake programs (shell scripts) in place of Caddy, PHP-FPM and
//! Redis, so nothing is downloaded and nothing outside the temp folder is touched.

use anvil::Config;
use std::fs;
use std::net::{TcpListener, UdpSocket};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

/// A temp folder with `home/` as ANVIL_HOME. Dropping it kills every process it
/// started (and anything still running from `home/`) and deletes the folder.
struct Env {
    root: PathBuf,
    home: PathBuf,
    children: Vec<Child>,
}

impl Env {
    fn new(name: &str) -> Env {
        let root = std::env::temp_dir().join(format!("anvil-cli-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("home")).unwrap();
        // Canonical, so it matches the paths `anvil park` stores (/var is /private/var on macOS).
        let root = root.canonicalize().unwrap();
        Env {
            home: root.join("home"),
            root,
            children: vec![],
        }
    }

    fn cmd(&self, args: &[&str]) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_anvil"));
        c.args(args).env("ANVIL_HOME", &self.home);
        c
    }

    fn run_in(&self, dir: &Path, args: &[&str]) -> Output {
        self.cmd(args).current_dir(dir).output().unwrap()
    }

    /// Run `anvil <args>` and require it to succeed; returns stdout.
    fn ok(&self, args: &[&str]) -> String {
        let out = self.run_in(&self.root, args);
        assert!(
            out.status.success(),
            "anvil {args:?} failed: {}",
            text(&out)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// Run `anvil <args>` and require it to fail; returns stdout and stderr.
    fn fails(&self, args: &[&str]) -> String {
        let out = self.run_in(&self.root, args);
        assert!(
            !out.status.success(),
            "anvil {args:?} succeeded: {}",
            text(&out)
        );
        text(&out)
    }

    fn config(&self) -> Config {
        toml::from_str(&fs::read_to_string(self.home.join("config.toml")).unwrap()).unwrap()
    }

    fn write_config(&self, cfg: &Config) {
        fs::write(
            self.home.join("config.toml"),
            toml::to_string_pretty(cfg).unwrap(),
        )
        .unwrap();
    }

    /// A stand-in program at `home/<rel>`: it records its arguments in `<rel>.args`
    /// and its pid in `<rel>.pid`, then idles. It stays a shell running the script
    /// by its full path, so its command line names ANVIL_HOME like a real one would.
    fn fake(&self, rel: &str) -> PathBuf {
        let p = self.home.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(
            &p,
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$0.args\"\necho $$ > \"$0.pid\"\nwhile :; do sleep 1; done\n",
        )
        .unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    /// `anvil start` in the background, output in `start.log`; returns once it's
    /// running and has its Ctrl-C handler.
    fn start(&mut self) -> usize {
        let log = self.root.join("start.log");
        let _ = fs::remove_file(&log);
        let out = fs::File::create(&log).unwrap();
        let child = self
            .cmd(&["start"])
            .stdin(Stdio::null())
            .stdout(out.try_clone().unwrap())
            .stderr(out)
            .spawn()
            .unwrap();
        self.children.push(child);
        let i = self.children.len() - 1;
        let up = wait_for(Duration::from_secs(10), || {
            let exited = matches!(self.children[i].try_wait(), Ok(Some(_)));
            assert!(!exited, "anvil start exited: {}", self.log());
            self.log().contains("Anvil running")
        });
        assert!(up, "anvil start didn't come up: {}", self.log());
        // The Ctrl-C handler is set right after that line is printed.
        std::thread::sleep(Duration::from_millis(300));
        i
    }

    fn log(&self) -> String {
        fs::read_to_string(self.root.join("start.log")).unwrap_or_default()
    }

    fn signal(&mut self, i: usize, sig: &str) {
        let pid = self.children[i].id().to_string();
        assert!(
            Command::new("kill")
                .args([sig, &pid])
                .status()
                .unwrap()
                .success()
        );
        let child = &mut self.children[i];
        let done = wait_for(Duration::from_secs(15), || {
            matches!(child.try_wait(), Ok(Some(_)))
        });
        assert!(done, "anvil start ignored {sig}");
    }

    /// The pid a fake program recorded, once it has started.
    fn pid_of(&self, rel: &str) -> String {
        let f = self.home.join(format!("{rel}.pid"));
        assert!(
            wait_for(Duration::from_secs(10), || f.exists()),
            "{rel} never started: {}",
            self.log()
        );
        fs::read_to_string(f).unwrap().trim().to_string()
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        for c in &mut self.children {
            let _ = c.kill();
            let _ = c.wait();
        }
        let _ = Command::new("pkill")
            .args(["-9", "-f"])
            .arg(&self.home)
            .status();
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn wait_for(timeout: Duration, mut f: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + timeout;
    while Instant::now() < end {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    f()
}

fn alive(pid: &str) -> bool {
    Command::new("kill")
        .args(["-0", pid])
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// `n` distinct free TCP ports (held together while picking so they differ).
fn free_ports(n: usize) -> Vec<u16> {
    let held: Vec<_> = (0..n)
        .map(|_| TcpListener::bind("127.0.0.1:0").unwrap())
        .collect();
    held.iter()
        .map(|l| l.local_addr().unwrap().port())
        .collect()
}

fn free_udp_port() -> u16 {
    UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// Web server and DNS on free high ports, so `anvil start` runs next to anything.
fn config_with_free_ports() -> Config {
    let p = free_ports(2);
    Config {
        http_port: p[0],
        https_port: p[1],
        dns_port: free_udp_port(),
        ..Default::default()
    }
}

fn write(path: &Path, content: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

const REDIS: &str = "services/redis/8.10.2/bin/redis-server";

#[test]
fn config_round_trip() {
    let env = Env::new("config");
    let parked = env.root.join("sites");
    write(&parked.join("shop/artisan"), "");
    write(&parked.join("shop/composer.json"), "{}");
    write(
        &parked.join("front/package.json"),
        r#"{"dependencies":{"next":"15.0.0"}}"#,
    );
    write(&parked.join("plain/index.html"), "hi");
    write(
        &parked.join("api/Cargo.toml"),
        "[package]\nname = \"api\"\n",
    );

    env.ok(&["park", parked.to_str().unwrap()]);
    assert_eq!(env.config().paths, std::slice::from_ref(&parked));

    let kinds = |env: &Env| -> Vec<(String, String)> {
        env.ok(&["sites"])
            .lines()
            .map(|l| {
                let mut w = l.split_whitespace();
                (w.next().unwrap().into(), w.next().unwrap().into())
            })
            .collect()
    };
    let want = |list: &[(&str, &str)]| -> Vec<(String, String)> {
        list.iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    };
    assert_eq!(
        kinds(&env),
        want(&[
            ("api.test", "Rust"),
            // Next.js opens at .localhost until its config allows .test.
            ("front.localhost", "Next.js"),
            ("plain.test", "Static"),
            ("shop.test", "Laravel"),
        ])
    );

    // link / unlink
    let other = env.root.join("elsewhere/Extra Project");
    fs::create_dir_all(&other).unwrap();
    let out = env.run_in(&other, &["link", "Extra.test"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(env.config().links["extra"], other);
    assert!(env.ok(&["sites"]).contains("extra.test"));
    env.ok(&["unlink", "extra"]);
    assert!(env.config().links.is_empty());
    assert!(!env.ok(&["sites"]).contains("extra.test"));
    assert!(other.is_dir(), "unlink never deletes the folder");

    // app type
    env.ok(&["app", "type", "plain", "vite"]);
    assert_eq!(env.config().sites["plain"].kind.as_deref(), Some("vite"));
    assert!(env.ok(&["sites"]).contains("Vite"));
    env.fails(&["app", "type", "plain", "cobol"]);
    env.ok(&["app", "type", "plain"]);
    assert_eq!(env.config().sites["plain"].kind, None);

    // app port
    env.ok(&["app", "port", "front", "45001"]);
    assert_eq!(env.config().sites["front"].port, Some(45001));
    env.fails(&["app", "port", "front", "80"]);
    assert_eq!(env.config().sites["front"].port, Some(45001));
    env.ok(&["app", "port", "front"]);
    assert_eq!(env.config().sites["front"].port, None);

    // php.ini, global and per site
    env.ok(&["php", "ini", "memory_limit", "1G"]);
    assert_eq!(env.config().ini["memory_limit"], "1G");
    env.fails(&["php", "ini", "memory_limit", "1G;x"]);
    env.fails(&["php", "ini", "bad]key", "1"]);
    env.ok(&["php", "ini", "memory_limit"]);
    assert!(env.config().ini.is_empty());
    env.ok(&["ini", "Shop", "upload_max_filesize", "64M"]);
    assert_eq!(env.config().sites["shop"].ini["upload_max_filesize"], "64M");
    env.fails(&["ini", "shop", "upload_max_filesize", "1M\n[evil]"]);
    env.ok(&["ini", "shop", "upload_max_filesize"]);
    assert!(env.config().sites["shop"].ini.is_empty());

    // forget
    env.ok(&["forget", parked.to_str().unwrap()]);
    assert!(env.config().paths.is_empty());
    assert_eq!(env.ok(&["sites"]), "");
}

#[test]
fn services_without_network() {
    let env = Env::new("services");
    let bin = env.fake(REDIS);
    let port = free_ports(1)[0].to_string();

    let out = env.ok(&[
        "service",
        "add",
        "redis",
        "cache",
        "--port",
        &port,
        "--accept-license",
    ]);
    assert!(!out.contains("Downloading"), "{out}");
    assert!(
        out.contains(&format!("Added cache on port {port}")),
        "{out}"
    );
    let s = &env.config().services["cache"];
    assert_eq!(s.kind.as_deref(), Some("redis"));
    assert_eq!(s.version.as_deref(), Some("8.10.2"));
    assert_eq!(s.port.map(|p| p.to_string()), Some(port.clone()));
    assert!(s.enabled);

    let list = env.ok(&["service", "list"]);
    let line = list.lines().find(|l| l.starts_with("cache ")).unwrap();
    let words: Vec<_> = line.split_whitespace().collect();
    assert_eq!(
        words,
        ["cache", "redis", "8.10.2", "port", &port, "enabled"]
    );

    // Names and ports are checked.
    env.fails(&[
        "service",
        "add",
        "redis",
        "other",
        "--port",
        &port,
        "--accept-license",
    ]);
    env.fails(&["service", "add", "redis", "../x"]);
    env.fails(&["service", "add", "mongodb"]);
    // Without --accept-license and no terminal to ask on, nothing is added.
    env.fails(&["service", "add", "postgres", "db"]);
    assert_eq!(env.config().services.len(), 1);

    env.ok(&["service", "disable", "cache"]);
    assert!(!env.config().services["cache"].enabled);
    assert!(env.ok(&["service", "list"]).contains(" installed"));
    env.ok(&["service", "enable", "cache"]);
    assert!(env.config().services["cache"].enabled);

    env.ok(&["service", "tag", "cache", "queue", "client-x", "queue", " "]);
    assert_eq!(env.config().services["cache"].tags, ["queue", "client-x"]);
    env.fails(&["service", "tag", "cache", "a,b"]);
    env.ok(&["service", "tag", "cache"]);
    assert!(env.config().services["cache"].tags.is_empty());

    env.ok(&["service", "link", "cache", "shop", "blog", "shop"]);
    assert_eq!(env.config().services["cache"].apps, ["shop", "blog"]);

    // The program can't be removed while a service uses it.
    env.fails(&["service", "uninstall", "redis", "8.10.2"]);
    assert!(bin.exists());

    // Removing keeps the data unless asked to delete it.
    let data = env.home.join("data/cache");
    write(&data.join("dump.rdb"), "x");
    env.ok(&["service", "remove", "cache"]);
    assert!(env.config().services.is_empty());
    assert!(data.join("dump.rdb").exists());
    env.ok(&[
        "service",
        "add",
        "redis",
        "cache",
        "--port",
        &port,
        "--accept-license",
    ]);
    env.ok(&["service", "remove", "cache", "--delete-data"]);
    assert!(!data.exists());
    env.fails(&["service", "remove", "cache"]);
    assert!(bin.exists(), "removing a service keeps the program");
}

#[test]
fn start_stops_children_and_cleans_up_orphans() {
    let mut env = Env::new("lifecycle");
    env.write_config(&config_with_free_ports());
    env.fake("bin/caddy");
    env.fake(REDIS);
    let port = free_ports(1)[0].to_string();
    env.ok(&[
        "service",
        "add",
        "redis",
        "cache",
        "--port",
        &port,
        "--accept-license",
    ]);

    // Ctrl-C stops everything.
    let i = env.start();
    let redis = env.pid_of(REDIS);
    let caddy = env.pid_of("bin/caddy");
    let args = fs::read_to_string(env.home.join(format!("{REDIS}.args"))).unwrap();
    let args: Vec<_> = args.lines().collect();
    let data = env.home.join("data/cache").display().to_string();
    assert!(args.windows(2).any(|w| w == ["--port", &port]), "{args:?}");
    assert!(args.windows(2).any(|w| w == ["--dir", &data]), "{args:?}");
    assert!(
        args.windows(2).any(|w| w == ["--bind", "127.0.0.1"]),
        "{args:?}"
    );
    let caddy_args = fs::read_to_string(env.home.join("bin/caddy.args")).unwrap();
    assert!(caddy_args.starts_with("run\n"), "{caddy_args}");
    assert!(alive(&redis) && alive(&caddy));

    env.signal(i, "-INT");
    assert!(env.children[i].wait().unwrap().success(), "{}", env.log());
    assert!(
        wait_for(Duration::from_secs(5), || !alive(&redis) && !alive(&caddy)),
        "children outlived anvil start"
    );
    assert!(!env.home.join("run/pids").exists());
    assert!(!env.home.join("run/svc-cache.pids").exists());

    // SIGKILL leaves the children behind; the next start ends them.
    for f in [REDIS, "bin/caddy"] {
        fs::remove_file(env.home.join(format!("{f}.pid"))).unwrap();
    }
    let i = env.start();
    let (redis, caddy) = (env.pid_of(REDIS), env.pid_of("bin/caddy"));
    env.signal(i, "-KILL");
    std::thread::sleep(Duration::from_millis(200));
    assert!(alive(&redis) && alive(&caddy), "orphans should still run");

    for f in [REDIS, "bin/caddy"] {
        fs::remove_file(env.home.join(format!("{f}.pid"))).unwrap();
    }
    let i = env.start();
    assert!(
        wait_for(Duration::from_secs(5), || !alive(&redis) && !alive(&caddy)),
        "orphans survived the next start"
    );
    let (redis2, caddy2) = (env.pid_of(REDIS), env.pid_of("bin/caddy"));
    assert!(redis2 != redis && caddy2 != caddy);
    env.signal(i, "-INT");
    assert!(
        wait_for(Duration::from_secs(5), || !alive(&redis2)
            && !alive(&caddy2)),
        "children outlived anvil start"
    );
}

#[test]
fn caddyfile_for_a_few_sites() {
    let mut env = Env::new("caddyfile");
    env.write_config(&config_with_free_ports());
    env.fake("bin/caddy");
    env.fake("php/8.4/php-fpm");
    let parked = env.root.join("sites");
    write(&parked.join("shop/artisan"), "");
    write(&parked.join("shop/composer.json"), "{}");
    write(&parked.join("shop/public/index.php"), "");
    write(&parked.join("plain/index.html"), "hi");
    write(
        &parked.join("front/package.json"),
        r#"{"dependencies":{"next":"15.0.0"},"scripts":{"dev":"next dev","docs:dev":"vitepress dev docs"}}"#,
    );
    write(
        &parked.join("ui/package.json"),
        r#"{"devDependencies":{"vite":"8"},"scripts":{"dev":"vite","docs:dev":"vitepress dev docs"}}"#,
    );
    // A quote in the folder name would end the quoted root in the Caddyfile.
    write(&parked.join("bad\"quote/index.html"), "hi");
    env.ok(&["park", parked.to_str().unwrap()]);
    env.ok(&["use", "8.4"]);
    env.ok(&["ini", "shop", "memory_limit", "1G"]);
    env.ok(&["app", "port", "front", "45123"]);
    // New sites get HTTP; `plain` is switched to HTTPS.
    env.ok(&["https", "plain"]);

    let i = env.start();
    let fpm = env.pid_of("php/8.4/php-fpm");
    let cfg = env.config();
    let file = fs::read_to_string(env.home.join("run/Caddyfile")).unwrap();
    let (home, sites) = (env.home.display(), parked.display());
    for line in [
        format!("\thttp_port {}\n", cfg.http_port),
        format!("\thttps_port {}\n", cfg.https_port),
        "\tdefault_bind 127.0.0.1 [::1]\n".into(),
        "\tadmin off\n".into(),
        format!(
            "\nhttp://shop.test {{\n\troot * \"{sites}/shop/public\"\n\tencode\n\tphp_fastcgi unix/{home}/run/site-shop.sock\n\tfile_server {{\n\t\thide .*\n\t}}\n}}\n"
        ),
        format!(
            "\nplain.test {{\n\troot * \"{sites}/plain\"\n\tencode\n\tphp_fastcgi unix/{home}/run/php-8.4.sock\n"
        ),
        "\nhttp://plain.test {\n\tredir https://{host}".into(),
        "\nhttp://front.test, http://front.localhost {\n\treverse_proxy 127.0.0.1:45123\n".into(),
        // Its docs server has an address of its own.
        "\nhttp://docs.front.localhost {\n\treverse_proxy 127.0.0.1:".into(),
        // A Vite app's docs server answers under the tld.
        "\nhttp://docs.ui.test {\n\treverse_proxy 127.0.0.1:".into(),
    ] {
        assert!(file.contains(&line), "missing {line:?} in:\n{file}");
    }
    assert!(!file.contains("quote"), "{file}");
    assert_eq!(file.matches("hide .*").count(), 2, "{file}");

    let fpm_conf = fs::read_to_string(env.home.join("run/php-8.4-fpm.conf")).unwrap();
    assert!(fpm_conf.contains("[site-shop]"), "{fpm_conf}");
    assert!(
        fpm_conf.contains("php_value[memory_limit] = 1G"),
        "{fpm_conf}"
    );

    env.signal(i, "-INT");
    assert!(
        wait_for(Duration::from_secs(5), || !alive(&fpm)),
        "PHP-FPM outlived anvil start"
    );
}

#[test]
fn paths_from_the_command_line_stay_inside() {
    let env = Env::new("paths");
    let canary = env.root.join("canary");
    write(&canary.join("keep"), "x");
    let canary_s = canary.to_str().unwrap();

    // Runtime versions are folder names; `..` must not reach outside.
    write(&env.home.join("runtimes/node/22/bin/node"), "");
    env.fails(&["runtime", "uninstall", "node", "../../.."]);
    env.fails(&["runtime", "uninstall", "node", ".."]);
    env.fails(&["runtime", "uninstall", "node", canary_s]);
    env.fails(&["service", "uninstall", "redis", "../../.."]);
    env.fails(&["php", "uninstall", "../.."]);
    assert!(canary.join("keep").exists());
    assert!(env.home.join("runtimes/node/22/bin/node").exists());
    env.ok(&["runtime", "uninstall", "node", "22"]);
    assert!(!env.home.join("runtimes/node/22").exists());

    // `clean` only deletes the items it knows, inside the project.
    let parked = env.root.join("sites");
    let shop = parked.join("shop");
    write(&shop.join("artisan"), "");
    write(&shop.join("composer.json"), "{}");
    env.ok(&["park", parked.to_str().unwrap()]);
    for p in ["/etc", canary_s, "../../canary", "."] {
        env.fails(&["clean", "shop", p]);
    }
    assert!(Path::new("/etc").is_dir());
    assert!(canary.join("keep").exists());
    assert!(shop.join("artisan").exists());

    // A symlinked item is removed itself, never what it points to...
    std::os::unix::fs::symlink(&canary, shop.join("vendor")).unwrap();
    env.ok(&["clean", "shop", "vendor"]);
    assert!(fs::symlink_metadata(shop.join("vendor")).is_err());
    assert!(canary.join("keep").exists());
    // ...and a symlinked folder on the way out of the project is refused.
    write(&canary.join("logs/laravel.log"), "x");
    std::os::unix::fs::symlink(&canary, shop.join("storage")).unwrap();
    let out = env.fails(&["clean", "shop", "storage/logs"]);
    assert!(out.contains("outside the project"), "{out}");
    assert!(canary.join("logs/laravel.log").exists());
}
