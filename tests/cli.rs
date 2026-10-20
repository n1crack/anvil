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
        Env::at(&std::env::temp_dir(), name)
    }

    /// Under /tmp: macOS's temp dir is too long a base for the control socket's path.
    fn short(name: &str) -> Env {
        Env::at(Path::new("/tmp"), name)
    }

    fn at(base: &Path, name: &str) -> Env {
        let root = base.join(format!("anvil-cli-{}-{name}", std::process::id()));
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
fn https_refuses_an_app_anvil_doesnt_serve() {
    let env = Env::new("https-compose");
    let dir = env.root.join("stack");
    write(&dir.join("docker-compose.yml"), "services: {}\n");
    env.run_in(&dir, &["link"]);
    let err = env.fails(&["https", "stack"]);
    assert!(err.contains("doesn't serve"), "{err}");
    assert!(
        env.config()
            .sites
            .get("stack")
            .is_none_or(|s| s.https.is_none())
    );
}

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
    env.ok(&["app", "ini", "Shop", "upload_max_filesize", "64M"]);
    assert_eq!(env.config().sites["shop"].ini["upload_max_filesize"], "64M");
    env.fails(&["app", "ini", "shop", "upload_max_filesize", "1M\n[evil]"]);
    env.ok(&["app", "ini", "shop", "upload_max_filesize"]);
    assert!(env.config().sites["shop"].ini.is_empty());

    // An app's PHP: pinned, then back to the default by leaving the version out.
    env.fake("php/8.3/php-fpm");
    let err = env.fails(&["app", "php", "shop", "8.1"]);
    assert!(err.contains("anvil php install 8.1"), "{err}");
    env.ok(&["app", "php", "Shop", "8.3"]);
    assert_eq!(env.config().sites["shop"].php.as_deref(), Some("8.3"));
    env.ok(&["app", "php", "shop"]);
    assert_eq!(env.config().sites["shop"].php, None);

    // An app's web root: shown, set relative to the project, refused outside it, reset.
    assert!(env.ok(&["app", "root", "shop"]).starts_with(". (detected"));
    fs::create_dir_all(parked.join("shop/web")).unwrap();
    env.ok(&["app", "root", "shop", "web/"]);
    assert_eq!(env.config().sites["shop"].root.as_deref(), Some("web"));
    assert!(env.ok(&["app", "root", "shop"]).starts_with("web ("));
    assert!(env.ok(&["sites"]).contains("shop/web"));
    for bad in ["dist", "../front", "/tmp", "we\"b"] {
        env.fails(&["app", "root", "shop", bad]);
    }
    let err = env.fails(&["app", "root", "front", "."]);
    assert!(err.contains("no web root"), "{err}");
    assert_eq!(env.config().sites["shop"].root.as_deref(), Some("web"));
    env.ok(&["app", "root", "shop", "--reset"]);
    assert_eq!(env.config().sites["shop"].root, None);

    // The old spellings are gone.
    assert!(
        env.fails(&["use", "8.3", "--site", "shop"])
            .contains("--site")
    );
    assert!(
        env.fails(&["ini", "shop", "memory_limit", "1G"])
            .contains("unrecognized subcommand")
    );

    // forget
    env.ok(&["forget", parked.to_str().unwrap()]);
    assert!(env.config().paths.is_empty());
    assert_eq!(env.ok(&["sites"]), "");
}

/// What the fake app saw: `(method, client, params)`.
type Seen = std::sync::Arc<std::sync::Mutex<Vec<(String, String, serde_json::Value)>>>;

/// A stand-in for the running app on `env`'s control socket: answers every method
/// (`run` and `service` with rows, like the app) and records what it saw.
fn fake_app(env: &Env) -> Seen {
    use std::sync::{Arc, Mutex};
    // `bind` reads ANVIL_HOME from this process; tests run in parallel.
    static ENV: Mutex<()> = Mutex::new(());
    let _one = ENV.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: the only in-process reader of ANVIL_HOME here, under ENV.
    unsafe { std::env::set_var("ANVIL_HOME", &env.home) };
    let l = anvil::control::bind().unwrap();
    let seen = Arc::new(Mutex::new(vec![]));
    let log = seen.clone();
    std::thread::spawn(move || {
        anvil::control::serve(
            l,
            Arc::new(move |r| {
                let mut log = log.lock().unwrap();
                log.push((r.method.clone(), r.client.clone(), r.params.clone()));
                Ok(match r.method.as_str() {
                    "run" => serde_json::json!({"rows": [
                        {"id": "api", "name": "api", "running": true},
                        {"id": "queue", "name": "queue", "running": false},
                    ]}),
                    "service" => serde_json::json!([
                        {"id": "mysql", "running": false},
                        {"id": "redis", "running": true},
                    ]),
                    _ => serde_json::json!({"version": "test"}),
                })
            }),
        )
    });
    seen
}

#[test]
fn edit_without_app_prints_nothing_extra() {
    let env = Env::short("ctl-none");
    let dir = env.root.join("extra");
    fs::create_dir_all(&dir).unwrap();
    let t = Instant::now();
    let out = env.run_in(&dir, &["link"]);
    // Well under the socket's 5 s timeout, with room for a loaded machine.
    assert!(t.elapsed() < Duration::from_secs(3));
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!("Linked extra.test -> {}\n", dir.display())
    );
    assert_eq!(String::from_utf8_lossy(&out.stderr), "");
}

#[test]
fn edit_tells_a_running_app() {
    let env = Env::short("ctl-edit");
    let seen = fake_app(&env);
    let dir = env.root.join("extra");
    fs::create_dir_all(&dir).unwrap();
    let out = env.run_in(&dir, &["link"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        seen.lock()
            .unwrap()
            .iter()
            .any(|(m, c, _)| m == "config_changed" && c == "Terminal"),
        "{:?}",
        seen.lock().unwrap()
    );
}

#[test]
fn app_restart_sends_run() {
    let env = Env::short("ctl-run");
    let seen = fake_app(&env);
    assert_eq!(env.ok(&["app", "restart", "shop", "api"]), "api: running\n");
    assert_eq!(
        seen.lock().unwrap().last().unwrap(),
        &(
            "run".into(),
            "Terminal".into(),
            serde_json::json!({"site": "shop", "process": "api", "action": "restart"})
        )
    );
}

#[test]
fn app_stop_lists_every_process() {
    let env = Env::short("ctl-all");
    let seen = fake_app(&env);
    let out = env.ok(&["app", "stop", "shop"]);
    assert_eq!(out, "api: running\nqueue: stopped\n");
    assert_eq!(
        seen.lock().unwrap().last().unwrap().2,
        serde_json::json!({"site": "shop", "action": "stop"})
    );
}

#[test]
fn service_stop_sends_service() {
    let env = Env::short("ctl-svc");
    let seen = fake_app(&env);
    assert_eq!(env.ok(&["service", "stop", "mysql"]), "mysql: stopped\n");
    assert_eq!(
        seen.lock().unwrap().last().unwrap(),
        &(
            "service".into(),
            "Terminal".into(),
            serde_json::json!({"id": "mysql", "action": "stop"})
        )
    );
}

#[test]
fn app_start_without_anvil() {
    let env = Env::short("ctl-off");
    let out = env.run_in(&env.root, &["app", "start", "shop"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "Anvil isn't running. Open it, then try again.\n"
    );
}

#[test]
fn start_refuses_while_the_app_runs() {
    let env = Env::short("ctl-start");
    let _seen = fake_app(&env);
    let out = env.fails(&["start"]);
    assert!(
        out.contains("Anvil is running and already serves your apps"),
        "{out}"
    );
}

#[test]
fn mcp_off_by_default() {
    let env = Env::short("mcp-off");
    let out = env.run_in(&env.root, &["mcp"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "Turn on Agents (MCP) in Settings › Plugins\n"
    );
    assert!(out.stdout.is_empty());
}

#[test]
fn mcp_on_answers_on_stdout() {
    use std::io::Write;
    let env = Env::short("mcp-on");
    write(&env.home.join("config.toml"), "mcp = true\n");
    let mut child = env
        .cmd(&["mcp"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let init =
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"t"}}}"#;
    let list = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#;
    writeln!(child.stdin.take().unwrap(), "{init}\n{list}").unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    let lines: Vec<serde_json::Value> = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["result"]["serverInfo"]["name"], "anvil");
    assert_eq!(lines[1]["result"]["tools"][0]["name"], "apps");
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
    env.ok(&["app", "ini", "shop", "memory_limit", "1G"]);
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
            "\nhttp://shop.test {{\n\troot * \"{sites}/shop/public\"\n\tencode\n\t@hidden path */.*\n\tphp_fastcgi unix/{home}/run/site-shop.sock\n\tfile_server @hidden {{\n\t\thide *\n\t}}\n\tfile_server\n}}\n"
        ),
        format!(
            "\nplain.test {{\n\troot * \"{sites}/plain\"\n\tencode\n\t@hidden path */.*\n\tphp_fastcgi unix/{home}/run/php-8.4.sock\n"
        ),
        "\nhttp://plain.test {\n\tredir https://{host}".into(),
        "\nhttp://front.test, http://front.localhost {\n\treverse_proxy 127.0.0.1:45123\n".into(),
        // Its docs server has an address of its own.
        "\nhttp://front--docs.test, http://front--docs.localhost {\n\treverse_proxy 127.0.0.1:"
            .into(),
        // ...and the old one redirects to it.
        "\nhttp://docs.front.localhost {\n\tredir http://front--docs.test:".into(),
        // A Vite app's docs server answers under the tld.
        "\nhttp://ui--docs.test, http://ui--docs.localhost {\n\treverse_proxy 127.0.0.1:".into(),
    ] {
        assert!(file.contains(&line), "missing {line:?} in:\n{file}");
    }
    assert!(!file.contains("quote"), "{file}");
    assert_eq!(file.matches("@hidden path */.*").count(), 2, "{file}");

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

#[test]
fn worktree_add_without_a_terminal_lists_after_create_and_runs_nothing() {
    let env = Env::new("after-create");
    let shop = env.root.join("sites/shop");
    write(&shop.join("artisan"), "");
    let git = |args: &[&str]| {
        let ok = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(args)
            .current_dir(&shop)
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["add", "artisan"]);
    git(&["commit", "-q", "-m", "i"]);
    let parked = env.root.join("sites");
    write(
        &env.home.join("config.toml"),
        &format!("paths = [{parked:?}]\n[sites.shop.worktree]\nafter_create = [\"touch ran\"]\n"),
    );
    // `output()` gives the CLI no terminal: nothing to ask, so nothing runs.
    let out = env.ok(&["worktree", "add", "feat", "--new", "--app", "shop"]);
    assert!(
        out.contains("Not run (no terminal to ask): touch ran"),
        "{out}"
    );
    assert!(env.root.join("sites/shop--feat").is_dir());
    assert!(!env.root.join("sites/shop--feat/ran").exists());
}

#[test]
fn worktree_remove_without_a_terminal_names_the_data_and_refuses() {
    let env = Env::new("remove-losses");
    let shop = env.root.join("sites/shop");
    write(&shop.join("artisan"), "");
    write(
        &shop.join(".gitignore"),
        ".env\n/vendor\ndatabase/*.sqlite\n",
    );
    write(&shop.join(".env"), "DB_CONNECTION=sqlite\nMAIL_HOST=main\n");
    let git = |args: &[&str]| {
        let ok = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(args)
            .current_dir(&shop)
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["add", "artisan", ".gitignore"]);
    git(&["commit", "-q", "-m", "i"]);
    let parked = env.root.join("sites");
    write(
        &env.home.join("config.toml"),
        &format!("paths = [{parked:?}]\n"),
    );
    env.ok(&["worktree", "add", "feat", "--new", "--app", "shop"]);
    let wt = env.root.join("sites/shop--feat");
    write(&wt.join("database/database.sqlite"), "data");
    write(&wt.join("vendor/autoload.php"), "");
    let dotenv = fs::read_to_string(wt.join(".env")).unwrap();
    write(&wt.join(".env"), &dotenv.replace("=main", "=edited"));
    // `output()` gives the CLI no terminal: it names what goes and refuses.
    let out = env.fails(&["worktree", "remove", "shop--feat"]);
    assert!(
        out.contains(
            "These will be deleted with the folder: database/database.sqlite, .env: add --yes"
        ),
        "{out}"
    );
    assert!(wt.join("database/database.sqlite").exists());
    env.ok(&["worktree", "remove", "shop--feat", "--yes"]);
    assert!(!wt.exists());
}

#[test]
fn cli_php_ext_add_lists_it() {
    let env = Env::new("php-ext");
    env.write_config(&config_with_free_ports());
    env.fake("php/8.4/php-fpm");
    // Says it is 8.4.23 when asked; otherwise prints the arguments it got and the ini
    // file in its first scan dir.
    let php = env.home.join("php/8.4/php");
    write(
        &php,
        "#!/bin/sh\nif [ \"$1\" = -n ]; then printf 8.4.23; exit; fi\nprintf '%s\\n' \"$@\"\nd=${PHP_INI_SCAN_DIR%%:*}\n[ -z \"$d\" ] || cat \"$d/anvil.ini\"\n",
    );
    fs::set_permissions(&php, fs::Permissions::from_mode(0o755)).unwrap();
    // Built before: turning it on needs no build.
    let so = env.home.join("php/8.4/ext/8.4.23/apcu.so");
    write(&so, "");
    env.ok(&["use", "8.4"]);

    env.ok(&["php", "ext", "add", "apcu"]);
    assert_eq!(env.config().php_ext["8.4"], ["apcu"]);
    let list = env.ok(&["php", "ext", "list"]);
    let line = |name: &str| list.lines().find(|l| l.starts_with(name)).unwrap();
    assert!(line("apcu").contains("on"), "{list}");
    assert!(line("xdebug").contains("off"), "{list}");
    // The terminal's php loads it, as PHP-FPM does: from its scan dir, not `-d`.
    let out = env.ok(&["shim", "php", "-v"]);
    assert_eq!(out.trim(), format!("-v\nextension={}", so.display()));

    let e = env.fails(&["php", "ext", "add", "apcu", "--php", "8.3"]);
    assert!(e.contains("not installed"), "{e}");
    let e = env.fails(&["php", "ext", "add", "swoole"]);
    assert!(e.contains("not in Anvil's catalog"), "{e}");

    env.ok(&["php", "ext", "remove", "apcu"]);
    assert!(env.config().php_ext.is_empty());
    let list = env.ok(&["php", "ext", "list"]);
    assert!(list.lines().all(|l| l.contains("off")), "{list}");
    assert!(so.exists(), "the build stays");
    assert!(!env.ok(&["shim", "php", "-v"]).contains("apcu"));
}
