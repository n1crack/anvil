//! Settings › Features › Tunnel: share an app at a temporary public address through a
//! Cloudflare quick tunnel (no account). cloudflared reaches Caddy on this computer
//! with the app's own Host, so Caddy serves it as it serves the browser here.

use crate::platform::{self, OwnGroup};
use crate::{Config, Site, Stopping, home};
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const TURNED_OFF: &str = "Turn on Tunnel in Settings › Features";
/// What a quick tunnel is, said once beside the address.
pub const LIMITS: &str = "A temporary address that changes every time; Cloudflare gives quick tunnels no uptime guarantee.";

/// cloudflared, pinned. A newer pin replaces the older download (`ensure`).
pub const VERSION: &str = "2026.10.2";
/// GitHub's SHA-256 of each release asset, confirmed by hashing the downloads. The sums
/// in Cloudflare's release notes are of the binaries inside the .tgz, not the archives.
/// Windows on ARM has no build.
const ASSETS: [(&str, &str, &str); 5] = [
    (
        "macos-aarch64",
        "cloudflared-darwin-arm64.tgz",
        "529a07089ac2d46762578389230fa148e5cb6ce2689200fc006956864042d17a",
    ),
    (
        "macos-x86_64",
        "cloudflared-darwin-amd64.tgz",
        "b64efd5a2772fc124b855f60eb3d33ba1a4f2ba67b19ddc58d32b705d42b82c9",
    ),
    (
        "linux-aarch64",
        "cloudflared-linux-arm64",
        "b7e55b4d9702c2ab5f8348f780bb1db044527fc555367714c78e0595750a4665",
    ),
    (
        "linux-x86_64",
        "cloudflared-linux-amd64",
        "35fbdd2beb378436c7da377d7358cea8e33f9c94a6705d1d8036bf9b0d6d8005",
    ),
    (
        "windows-x86_64",
        "cloudflared-windows-amd64.exe",
        "12c7c887f8aaab779aef1dfa9b60673530a5adb88ecc55dd6df8f56e0b70f199",
    ),
];

/// The release asset and its digest for `os`-`arch` (Rust's names).
fn asset(os: &str, arch: &str) -> Result<(&'static str, &'static str)> {
    let target = format!("{os}-{arch}");
    ASSETS
        .iter()
        .find(|(t, _, _)| *t == target)
        .map(|(_, file, sum)| (*file, *sum))
        .with_context(|| format!("Cloudflare publishes no cloudflared for {target}"))
}

fn dir() -> PathBuf {
    home().join("tunnel").join(VERSION)
}

fn bin() -> PathBuf {
    dir().join(platform::exe("cloudflared"))
}

fn config_file() -> PathBuf {
    dir().join("config.yml")
}

/// Download the pinned cloudflared once, checked against its digest, and drop older ones.
pub fn ensure() -> Result<PathBuf> {
    if !bin().exists() {
        let (file, sum) = asset(std::env::consts::OS, std::env::consts::ARCH)?;
        let url =
            format!("https://github.com/cloudflare/cloudflared/releases/download/{VERSION}/{file}");
        crate::replace_dir(&dir(), |d| {
            if file.ends_with(".tgz") {
                crate::fetch_tgz(&url, d, false, Some(sum))
            } else {
                // A bare program: Windows' .exe, Linux's binary (made executable).
                let got = crate::download(&url, Some(sum))?;
                let bin = d.join(platform::exe("cloudflared"));
                fs::rename(got, &bin)?;
                Ok(platform::set_mode(&bin, 0o755)?)
            }
        })?;
    }
    // Empty: a ~/.cloudflared/config.yml of the user's (a named tunnel's) stays out of it.
    let config = config_file();
    if !config.exists() {
        fs::write(&config, "")?;
    }
    for old in fs::read_dir(home().join("tunnel"))
        .into_iter()
        .flatten()
        .flatten()
    {
        let name = old.file_name();
        if name != VERSION && !name.to_string_lossy().starts_with('.') {
            let _ = fs::remove_dir_all(old.path());
        }
    }
    Ok(bin())
}

/// cloudflared's arguments for `site`: Caddy on this computer with the app's Host, at
/// the port a browser here uses (`port`). An HTTPS app's http:// redirects, so it goes
/// to https with the name its certificate has (Anvil's own CA, which cloudflared
/// doesn't know).
fn args(cfg: &Config, site: &Site, config: &Path) -> Vec<String> {
    let mut a: Vec<String> = ["tunnel", "--no-autoupdate", "--config"]
        .map(String::from)
        .into();
    a.push(config.display().to_string());
    a.push("--url".into());
    if site.secure {
        a.push(format!("https://127.0.0.1:{}", port(cfg, site)));
        a.extend(["--origin-server-name".into(), site.host.clone()]);
        a.push("--no-tls-verify".into());
    } else {
        a.push(format!("http://127.0.0.1:{}", port(cfg, site)));
    }
    a.extend(["--http-host-header".into(), site.host.clone()]);
    a
}

/// Where cloudflared reaches Caddy: 80 or 443 once pf forwards them, the way a browser
/// here does. On macOS, connections straight to Caddy's own ports behind those rules
/// can hang half-open (seen: every one to 127.0.0.1:8443 timing out, while 443 answered
/// at once), which made Share say the web server wasn't running.
fn port(cfg: &Config, site: &Site) -> u16 {
    if site.secure {
        crate::public_https_port(cfg)
    } else {
        crate::public_http_port(cfg)
    }
}

/// The public address cloudflared printed.
fn url_in(log: &str) -> Option<String> {
    let at = log.find("https://")?;
    log[at..].split_whitespace().find_map(|w| {
        let host = w.strip_prefix("https://")?;
        host.ends_with(".trycloudflare.com")
            .then(|| format!("https://{host}"))
    })
}

/// Whether the address is there and cloudflared holds a connection for it.
fn connected(log: &str) -> bool {
    log.contains("Registered tunnel connection") && url_in(log).is_some()
}

/// Why cloudflared ended, from its log.
fn failure(log: &str) -> String {
    if log.contains("status 429") || log.contains("1015") {
        return "Cloudflare is turning away new quick tunnels from this network for now; try again in a few minutes".into();
    }
    if log.contains("failed to request quick Tunnel") {
        return "Couldn't reach Cloudflare to open the tunnel: check the internet connection"
            .into();
    }
    // Its last line that isn't information, without the time and level.
    log.lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.contains(" INF "))
        .map(|l| match l.split_once(" ERR ") {
            Some((_, why)) => why.trim().to_string(),
            None => l.to_string(),
        })
        .unwrap_or_else(|| "cloudflared stopped".into())
}

/// For a Laravel app that doesn't trust the proxy, what its links point to: it builds
/// them from the Host, which is the app's own (Caddy passes X-Forwarded-Host on).
pub fn note(site: &Site) -> Option<String> {
    if !matches!(site.kind.as_str(), "laravel" | "statamic") {
        return None;
    }
    let read = |p: &str| fs::read_to_string(site.path.join(p)).unwrap_or_default();
    let middleware = read("app/Http/Middleware/TrustProxies.php");
    if read("bootstrap/app.php").contains("trustProxies(")
        || middleware.contains("'*'")
        || middleware.contains("127.0.0.1")
    {
        return None;
    }
    Some(format!(
        "Links the app builds still point to {}: trust the proxy with ->trustProxies(at: '*') in bootstrap/app.php",
        site.host
    ))
}

/// Whether the public address answers yet, asked through Cloudflare's DNS so this
/// computer's resolver doesn't keep a "no such name" from before the name existed.
/// 530 is Cloudflare's own page for a tunnel it can't find yet.
fn reachable(url: &str) -> bool {
    let out = platform::command("curl")
        .args(["-s", "--max-time", "5", "-w", "\n%{http_code}"])
        .args(["--doh-url", "https://cloudflare-dns.com/dns-query"])
        .arg(url)
        .output();
    let code = out.map(|o| {
        String::from_utf8_lossy(&o.stdout)
            .lines()
            .last()
            .unwrap_or("")
            .to_string()
    });
    code.is_ok_and(|c| c != "000" && c != "530" && !c.is_empty())
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum State {
    Starting,
    Ready { url: String },
    Failed { error: String },
}

struct Share {
    child: Arc<Mutex<Option<Child>>>,
    state: Arc<Mutex<State>>,
    log: PathBuf,
    /// cloudflared's arguments: they follow the app's HTTPS (`refresh`).
    args: Vec<String>,
    changed: Changed,
}

type Changed = Arc<dyn Fn() + Send + Sync>;

/// The apps shared now, by site name: one tunnel each.
pub struct Shares {
    home: PathBuf,
    list: BTreeMap<String, Share>,
}

impl Default for Shares {
    fn default() -> Self {
        Shares {
            home: home(),
            list: BTreeMap::new(),
        }
    }
}

fn pid_file(home: &Path, site: &str) -> PathBuf {
    home.join("run").join(format!("tunnel-{site}.pids"))
}

impl Shares {
    /// Open a tunnel to `site` (a failed one is tried again); `changed` is called once it
    /// is ready or has failed.
    pub fn start(
        &mut self,
        cfg: &Config,
        site: &Site,
        changed: impl Fn() + Send + Sync + 'static,
    ) -> Result<()> {
        self.open(cfg, site, Arc::new(changed))
    }

    /// Open again, at a new address, each tunnel whose app turned HTTPS on or off since:
    /// its old origin would only redirect visitors to the app's own .test address.
    pub fn refresh(&mut self, cfg: &Config, sites: &[Site]) {
        for site in sites {
            let Some(s) = self.list.get(&site.name) else {
                continue;
            };
            if s.args != args(cfg, site, &config_file()) {
                // A Caddy just started takes a moment to listen. Failing (the web server
                // down), the tunnel is gone, as it would be then anyway.
                let port = port(cfg, site);
                let until = Instant::now() + Duration::from_secs(5);
                while !crate::port_answers(port) && Instant::now() < until {
                    std::thread::sleep(Duration::from_millis(100));
                }
                let changed = s.changed.clone();
                let _ = self.open(cfg, site, changed);
            }
        }
    }

    fn open(&mut self, cfg: &Config, site: &Site, changed: Changed) -> Result<()> {
        if !crate::apps::serves(&site.kind) {
            bail!("{} has no address to share", site.name);
        }
        let args = args(cfg, site, &config_file());
        let current = |s: &Share| s.args == args;
        let starting = |s: &Share| *crate::lock(&s.state) == State::Starting;
        if self
            .list
            .get(&site.name)
            .is_some_and(|s| current(s) && (starting(s) || self.alive(&site.name)))
        {
            return Ok(());
        }
        if let Some(old) = self.take(&site.name) {
            old.wait(crate::apps::GRACE);
        }
        if !crate::port_answers(port(cfg, site)) {
            bail!("Anvil's web server isn't running");
        }
        let bin = ensure()?;
        crate::state_dirs_in(&self.home)?;
        let log = self
            .home
            .join("log")
            .join(format!("tunnel-{}.log", site.name));
        let out = fs::File::create(&log)?;
        let child = platform::command(&bin)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(out.try_clone()?)
            .stderr(out)
            .own_group()
            .spawn()
            .context("couldn't start cloudflared")?;
        let at = crate::started(child.id()).unwrap_or_default();
        fs::write(
            pid_file(&self.home, &site.name),
            format!("{}\n{} {at}", crate::owner_line(), child.id()).trim_end(),
        )?;
        let share = Share {
            child: Arc::new(Mutex::new(Some(child))),
            state: Arc::new(Mutex::new(State::Starting)),
            log: log.clone(),
            args,
            changed: changed.clone(),
        };
        let (child, state) = (share.child.clone(), share.state.clone());
        std::thread::spawn(move || watch(&child, &state, &log, || changed()));
        self.list.insert(site.name.clone(), share);
        Ok(())
    }

    fn alive(&self, site: &str) -> bool {
        self.list.get(site).is_some_and(|s| {
            crate::lock(&s.child)
                .as_mut()
                .is_some_and(|c| matches!(c.try_wait(), Ok(None)))
        })
    }

    /// Each shared app's state; a tunnel that ended on its own says why.
    pub fn states(&self) -> BTreeMap<String, State> {
        self.list
            .iter()
            .map(|(name, s)| {
                let mut state = crate::lock(&s.state).clone();
                if matches!(state, State::Ready { .. }) && !self.alive(name) {
                    state = State::Failed {
                        error: failure(&fs::read_to_string(&s.log).unwrap_or_default()),
                    };
                    *crate::lock(&s.state) = state.clone();
                }
                (name.clone(), state)
            })
            .collect()
    }

    /// Forget `site`'s tunnel and hand back its process to stop once the lock is released.
    pub fn take(&mut self, site: &str) -> Option<Stopping> {
        let share = self.list.remove(site)?;
        let child = crate::lock(&share.child).take()?;
        Some(Stopping {
            child,
            pid_files: vec![pid_file(&self.home, site)],
            dir: None,
        })
    }

    pub fn take_all(&mut self) -> Vec<Stopping> {
        let names: Vec<String> = self.list.keys().cloned().collect();
        names.iter().filter_map(|n| self.take(n)).collect()
    }
}

impl Drop for Shares {
    fn drop(&mut self) {
        crate::stop_all(self.take_all(), crate::apps::GRACE);
    }
}

/// Wait for the address to answer (or 30 s after cloudflared connected, in case only
/// the check can't get through), or for cloudflared to end.
fn watch(child: &Mutex<Option<Child>>, state: &Mutex<State>, log: &Path, changed: impl Fn()) {
    let mut since = None;
    loop {
        let ended = match crate::lock(child).as_mut() {
            None => return, // stopped
            Some(c) => !matches!(c.try_wait(), Ok(None)),
        };
        let text = fs::read_to_string(log).unwrap_or_default();
        let next = if ended {
            Some(State::Failed {
                error: failure(&text),
            })
        } else if connected(&text) {
            let url = url_in(&text).unwrap_or_default();
            let since = *since.get_or_insert_with(Instant::now);
            (reachable(&url) || since.elapsed() > Duration::from_secs(30))
                .then_some(State::Ready { url })
        } else {
            None
        };
        if let Some(s) = next {
            *crate::lock(state) = s;
            changed();
            return;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = "2026-10-20T14:00:38Z INF Thank you for trying Cloudflare Tunnel. ... (https://www.cloudflare.com/website-terms/), and ... https://developers.cloudflare.com/cloudflare-one/connections/connect-apps
2026-10-20T14:00:38Z INF Requesting new quick Tunnel on trycloudflare.com...
2026-10-20T14:00:43Z INF +--------------------------------------------------------------------------------------------+
2026-10-20T14:00:43Z INF |  Your quick Tunnel has been created! Visit it at (it may take some time to be reachable):  |
2026-10-20T14:00:43Z INF |  https://extending-effectiveness-social-bible.trycloudflare.com                            |
2026-10-20T14:00:43Z INF +--------------------------------------------------------------------------------------------+
";

    fn site(dir: &Path, kind: &str, secure: bool) -> Site {
        let cfg = Config {
            http_port: 18080,
            https_port: 18443,
            ..Config::default()
        };
        let mut s = crate::site_at(&cfg, &[], "hello".into(), dir.into(), None, None);
        s.kind = kind.into();
        s.secure = secure;
        s
    }

    #[test]
    fn reaches_caddy_with_the_apps_host() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config {
            http_port: 18080,
            https_port: 18443,
            ..Config::default()
        };
        let c = Path::new("/a/config.yml");
        assert_eq!(
            args(&cfg, &site(dir.path(), "php", false), c).join(" "),
            "tunnel --no-autoupdate --config /a/config.yml --url http://127.0.0.1:18080 --http-host-header hello.test"
        );
        // HTTPS: http:// would only redirect.
        assert_eq!(
            args(&cfg, &site(dir.path(), "php", true), c).join(" "),
            "tunnel --no-autoupdate --config /a/config.yml --url https://127.0.0.1:18443 --origin-server-name hello.test --no-tls-verify --http-host-header hello.test"
        );
    }

    #[test]
    fn reads_the_public_address_once_connected() {
        assert_eq!(
            url_in(LOG).as_deref(),
            Some("https://extending-effectiveness-social-bible.trycloudflare.com")
        );
        assert!(!connected(LOG));
        assert!(connected(&format!(
            "{LOG}2026-10-20T14:00:43Z INF Registered tunnel connection connIndex=0 location=ist07 protocol=quic\n"
        )));
        assert_eq!(
            url_in("INF Requesting new quick Tunnel on trycloudflare.com..."),
            None
        );
    }

    #[test]
    fn says_why_it_failed() {
        let no_net = "2026-10-20T14:05:19Z INF Requesting new quick Tunnel on trycloudflare.com...\nfailed to request quick Tunnel: Post \"https://api.trycloudflare.com/tunnel\": dial tcp: lookup api.trycloudflare.com: no such host\n";
        assert!(failure(no_net).starts_with("Couldn't reach Cloudflare"));
        let limited = "INF Requesting new quick Tunnel on trycloudflare.com...\nquick tunnel provisioning failed with status 429\n";
        assert!(failure(limited).starts_with("Cloudflare is turning away"));
        assert_eq!(
            failure("x INF fine\n2026-10-20T14:05:19Z ERR something odd happened\n"),
            "something odd happened"
        );
        assert_eq!(failure("x INF fine\n"), "cloudflared stopped");
    }

    #[test]
    fn pins_a_download_per_platform() {
        assert_eq!(
            asset("macos", "aarch64").unwrap().0,
            "cloudflared-darwin-arm64.tgz"
        );
        assert_eq!(
            asset("macos", "x86_64").unwrap().0,
            "cloudflared-darwin-amd64.tgz"
        );
        assert_eq!(
            asset("windows", "x86_64").unwrap().0,
            "cloudflared-windows-amd64.exe"
        );
        assert_eq!(
            asset("linux", "x86_64").unwrap().0,
            "cloudflared-linux-amd64"
        );
        assert!(asset("windows", "aarch64").is_err());
        for (_, _, sum) in ASSETS {
            assert!(sum.len() == 64 && sum.bytes().all(|b| b.is_ascii_hexdigit()));
        }
    }

    #[test]
    fn notes_links_only_for_laravel_that_doesnt_trust_the_proxy() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(note(&site(dir.path(), "static", false)), None);
        let laravel = site(dir.path(), "laravel", false);
        assert!(note(&laravel).unwrap().contains("hello.test"));
        fs::create_dir_all(dir.path().join("bootstrap")).unwrap();
        fs::write(
            dir.path().join("bootstrap/app.php"),
            "->withMiddleware(function (Middleware $m) { $m->trustProxies(at: '*'); })",
        )
        .unwrap();
        assert_eq!(note(&laravel), None);
    }

    /// A cloudflared stand-in that prints what the real one does and ends or waits.
    #[cfg(unix)]
    fn fake(script: &str) -> (tempfile::TempDir, std::sync::MutexGuard<'static, ()>) {
        let dir = tempfile::tempdir().unwrap();
        let env = crate::lock(&crate::ENV_LOCK);
        // SAFETY: every test that sets ANVIL_HOME holds ENV_LOCK.
        unsafe { std::env::set_var("ANVIL_HOME", dir.path()) };
        fs::create_dir_all(super::dir()).unwrap();
        fs::write(bin(), format!("#!/bin/sh\n{script}\n")).unwrap();
        platform::set_mode(&bin(), 0o755).unwrap();
        (dir, env)
    }

    #[cfg(unix)]
    #[test]
    fn a_failed_tunnel_says_why_and_a_stopped_one_is_gone() {
        let (dir, _env) = fake(
            "echo 'failed to request quick Tunnel: Post \"https://api.trycloudflare.com/tunnel\": dial tcp: no such host' >&2",
        );
        let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let cfg = Config {
            http_port: server.local_addr().unwrap().port(),
            ..Config::default()
        };
        let s = site(dir.path(), "php", false);
        let mut shares = Shares::default();
        let (tx, rx) = std::sync::mpsc::channel();
        shares
            .start(&cfg, &s, move || tx.send(()).unwrap())
            .unwrap();
        rx.recv_timeout(Duration::from_secs(10)).unwrap();
        match &shares.states()["hello"] {
            State::Failed { error } => assert!(error.starts_with("Couldn't reach Cloudflare")),
            other => panic!("{other:?}"),
        }
        // A waiting one, stopped: its process and pid file are gone.
        fs::write(bin(), "#!/bin/sh\nexec sleep 30\n").unwrap();
        shares.start(&cfg, &s, || {}).unwrap();
        assert_eq!(shares.states()["hello"], State::Starting);
        let pid = crate::lock(&shares.list["hello"].child)
            .as_ref()
            .unwrap()
            .id();
        assert!(pid_file(&shares.home, "hello").exists());
        shares.take("hello").unwrap().wait(Duration::from_secs(5));
        assert!(!platform::alive(pid));
        assert!(!pid_file(&shares.home, "hello").exists());
        assert!(shares.states().is_empty());
    }

    /// HTTPS turned on while shared: http:// would only redirect visitors to hello.test.
    #[cfg(unix)]
    #[test]
    fn a_tunnel_follows_the_apps_https() {
        let (dir, _env) = fake("exec sleep 30");
        let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = server.local_addr().unwrap().port();
        let cfg = Config {
            http_port: port,
            https_port: port,
            ..Config::default()
        };
        let mut s = site(dir.path(), "php", false);
        let mut shares = Shares::default();
        shares.start(&cfg, &s, || {}).unwrap();
        let pid = |sh: &Shares| crate::lock(&sh.list["hello"].child).as_ref().unwrap().id();
        let first = pid(&shares);
        shares.refresh(&cfg, std::slice::from_ref(&s));
        assert_eq!(pid(&shares), first, "nothing changed");
        s.secure = true;
        shares.refresh(&cfg, std::slice::from_ref(&s));
        assert!(!platform::alive(first));
        assert_ne!(pid(&shares), first);
        assert!(
            shares.list["hello"]
                .args
                .contains(&format!("https://127.0.0.1:{port}"))
        );
        shares.take("hello").unwrap().wait(Duration::from_secs(5));
    }

    #[cfg(unix)]
    #[test]
    fn refuses_while_the_web_server_is_down() {
        let (dir, _env) = fake("exec sleep 30");
        // Nothing listens on port 1. A port bound and let go can be another test's a
        // moment later, so it isn't one that surely doesn't answer.
        let cfg = Config {
            http_port: 1,
            ..Config::default()
        };
        let err = Shares::default()
            .start(&cfg, &site(dir.path(), "php", false), || {})
            .unwrap_err();
        assert!(err.to_string().contains("isn't running"));
    }
}
