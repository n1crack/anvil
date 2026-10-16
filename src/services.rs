//! Databases, cache and mail: installed into `~/.anvil/services/<id>`, data in
//! `~/.anvil/data/<id>`, listening on 127.0.0.1 only. They run independently of
//! the web server, so changing a site never restarts a database.

use crate::{Config, fetch_tgz, home, kill_orphans, platform, run, save, terminate};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

pub const SERVICES: [&str; 4] = ["mysql", "postgres", "redis", "mailpit"];

const MYSQL_VERSION: &str = "8.4.11";
const POSTGRES_VERSION: &str = "18.6.0";
const MAILPIT_VERSION: &str = "v1.31.6";

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
pub struct ServiceConfig {
    /// Start with Anvil.
    pub enabled: bool,
    pub port: Option<u16>,
    /// Mailpit's web inbox port.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub web_port: Option<u16>,
}

#[derive(Serialize)]
pub struct ServiceInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub category: &'static str,
    pub version: String,
    pub port: u16,
    /// Mailpit's web inbox.
    pub web: Option<String>,
    pub web_port: Option<u16>,
    pub installed: bool,
    pub enabled: bool,
    pub running: bool,
    /// Set when the port is taken by another program: the next free one.
    pub suggested_port: Option<u16>,
    /// The same for Mailpit's web inbox port.
    pub suggested_web_port: Option<u16>,
}

fn name(id: &str) -> &'static str {
    match id {
        "mysql" => "MySQL",
        "postgres" => "PostgreSQL",
        "redis" => "Redis",
        _ => "Mailpit",
    }
}

fn category(id: &str) -> &'static str {
    match id {
        "mysql" | "postgres" => "Database",
        "redis" => "Cache & Queue",
        _ => "Mail",
    }
}

fn in_use(port: u16) -> bool {
    TcpStream::connect(("127.0.0.1", port)).is_ok()
}

/// The next free port after `port`, for when another program holds it.
fn next_free(port: u16) -> Option<u16> {
    (port + 1..port.saturating_add(100)).find(|p| !in_use(*p))
}

fn default_port(id: &str) -> u16 {
    match id {
        "mysql" => 3306,
        "postgres" => 5432,
        "redis" => 6379,
        _ => 1025, // Mailpit SMTP
    }
}

const MAILPIT_WEB: u16 = 8025;

/// Mailpit's web inbox port; only Mailpit has one.
pub fn web_port(cfg: &Config, id: &str) -> Option<u16> {
    (id == "mailpit").then(|| {
        cfg.services
            .get(id)
            .and_then(|s| s.web_port)
            .unwrap_or(MAILPIT_WEB)
    })
}

pub fn port(cfg: &Config, id: &str) -> u16 {
    cfg.services
        .get(id)
        .and_then(|s| s.port)
        .unwrap_or_else(|| default_port(id))
}

fn dir(id: &str) -> PathBuf {
    home().join("services").join(id)
}

fn data(id: &str) -> PathBuf {
    home().join("data").join(id)
}

fn binary(id: &str) -> PathBuf {
    let d = dir(id);
    match id {
        "mysql" => d.join("bin/mysqld"),
        "postgres" => d.join("bin/postgres"),
        "redis" => d.join("bin/redis-server"),
        _ => d.join("mailpit"),
    }
}

fn check(id: &str) -> Result<()> {
    if !SERVICES.contains(&id) {
        bail!("Unknown service {id}");
    }
    Ok(())
}

pub fn installed(id: &str) -> bool {
    binary(id).exists()
}

/// Download (or for Redis, build) a service into `~/.anvil/services/<id>`.
pub fn install(id: &str) -> Result<()> {
    check(id)?;
    let (_, caddy_plat) = platform()?;
    let mac = caddy_plat.starts_with("mac");
    let arm = caddy_plat.ends_with("arm64");
    let d = dir(id);
    let _ = fs::remove_dir_all(&d);
    let result = match id {
        "mysql" => {
            // ponytail: macOS builds only; Linux uses the glibc .tar.xz with the Linux port.
            if !mac {
                bail!("MySQL install is macOS-only for now");
            }
            let arch = if arm { "arm64" } else { "x86_64" };
            let minor = &MYSQL_VERSION[..3];
            fetch_tgz(
                &format!(
                    "https://cdn.mysql.com/Downloads/MySQL-{minor}/mysql-{MYSQL_VERSION}-macos15-{arch}.tar.gz"
                ),
                &d,
                true,
            )
        }
        "postgres" => {
            let target = match (mac, arm) {
                (true, true) => "aarch64-apple-darwin",
                (true, false) => "x86_64-apple-darwin",
                (false, true) => "aarch64-unknown-linux-gnu",
                (false, false) => "x86_64-unknown-linux-gnu",
            };
            fetch_tgz(
                &format!(
                    "https://github.com/theseus-rs/postgresql-binaries/releases/download/{POSTGRES_VERSION}/postgresql-{POSTGRES_VERSION}-{target}.tar.gz"
                ),
                &d,
                true,
            )
        }
        "redis" => build_redis(&d),
        _ => {
            let os = if mac { "darwin" } else { "linux" };
            let arch = if arm { "arm64" } else { "amd64" };
            fetch_tgz(
                &format!(
                    "https://github.com/axllent/mailpit/releases/download/{MAILPIT_VERSION}/mailpit-{os}-{arch}.tar.gz"
                ),
                &d,
                false,
            )
        }
    };
    if result.is_err() {
        let _ = fs::remove_dir_all(&d);
    }
    result?;
    fs::write(d.join("VERSION"), version_of(id))?;
    Ok(())
}

/// Redis publishes no macOS binaries, so build it (needs the Xcode command line tools).
fn build_redis(prefix: &std::path::Path) -> Result<()> {
    let src = home().join("build").join("redis");
    let _ = fs::remove_dir_all(&src);
    fetch_tgz("https://download.redis.io/redis-stable.tar.gz", &src, true)?;
    let jobs = std::thread::available_parallelism().map_or(4, |n| n.get());
    // Only the server and CLI: the bundled modules need a newer GNU Make than macOS ships.
    run(Command::new("make")
        .args(["-C", "src", &format!("-j{jobs}"), "BUILD_TLS=no", "redis-server", "redis-cli"])
        .current_dir(&src))
    .context("Building Redis failed — are the Xcode command line tools installed? (xcode-select --install)")?;
    fs::create_dir_all(prefix.join("bin"))?;
    for bin in ["redis-server", "redis-cli"] {
        fs::copy(src.join("src").join(bin), prefix.join("bin").join(bin))?;
    }
    let version = Command::new(prefix.join("bin/redis-server"))
        .arg("--version")
        .output()?;
    let v = String::from_utf8_lossy(&version.stdout);
    let v = v
        .split_whitespace()
        .find_map(|w| w.strip_prefix("v="))
        .unwrap_or("stable");
    fs::write(prefix.join("REDIS_VERSION"), v)?;
    let _ = fs::remove_dir_all(src);
    Ok(())
}

fn version_of(id: &str) -> String {
    match id {
        "mysql" => MYSQL_VERSION.into(),
        "postgres" => POSTGRES_VERSION.into(),
        "redis" => {
            fs::read_to_string(dir(id).join("REDIS_VERSION")).unwrap_or_else(|_| "stable".into())
        }
        _ => MAILPIT_VERSION.trim_start_matches('v').into(),
    }
}

pub fn uninstall(id: &str) -> Result<()> {
    check(id)?;
    // Data stays in ~/.anvil/data/<id> so a reinstall finds the databases again.
    fs::remove_dir_all(dir(id)).with_context(|| format!("{} is not installed", name(id)))
}

pub fn set_enabled(cfg: &mut Config, id: &str, enabled: bool) -> Result<()> {
    check(id)?;
    cfg.services.entry(id.into()).or_default().enabled = enabled;
    save(cfg)
}

pub fn set_port(cfg: &mut Config, id: &str, port: Option<u16>) -> Result<()> {
    check(id)?;
    if port.is_some_and(|p| p < 1024) {
        bail!("Use a port of 1024 or higher");
    }
    cfg.services.entry(id.into()).or_default().port = port;
    save(cfg)
}

pub fn set_web_port(cfg: &mut Config, id: &str, port: Option<u16>) -> Result<()> {
    check(id)?;
    if id != "mailpit" {
        bail!("Only Mailpit has a web port");
    }
    if port.is_some_and(|p| p < 1024) {
        bail!("Use a port of 1024 or higher");
    }
    cfg.services.entry(id.into()).or_default().web_port = port;
    save(cfg)
}

/// First start: create the data directory the way each server expects.
fn init(id: &str, log: &fs::File) -> Result<()> {
    let data = data(id);
    if data.exists() {
        return Ok(());
    }
    let d = dir(id);
    let status = match id {
        "mysql" => Command::new(d.join("bin/mysqld"))
            .arg("--no-defaults")
            .arg("--initialize-insecure")
            .arg(format!("--basedir={}", d.display()))
            .arg(format!("--datadir={}", data.display()))
            .stdout(log.try_clone()?)
            .stderr(log.try_clone()?)
            .status()?,
        "postgres" => Command::new(d.join("bin/initdb"))
            .arg("-D")
            .arg(&data)
            .args(["-U", "postgres", "--auth=trust", "-E", "UTF8", "--locale=C"])
            .stdout(log.try_clone()?)
            .stderr(log.try_clone()?)
            .status()?,
        _ => {
            fs::create_dir_all(&data)?;
            return Ok(());
        }
    };
    if !status.success() {
        let _ = fs::remove_dir_all(&data);
        bail!(
            "Setting up {} failed — see {}",
            name(id),
            home().join("log").join(format!("{id}.log")).display()
        );
    }
    Ok(())
}

fn command(cfg: &Config, id: &str) -> Command {
    let (d, data, run_dir) = (dir(id), data(id), home().join("run"));
    let port = port(cfg, id).to_string();
    let mut c = Command::new(binary(id));
    match id {
        "mysql" => {
            c.arg("--no-defaults")
                .arg(format!("--basedir={}", d.display()))
                .arg(format!("--datadir={}", data.display()))
                .arg(format!("--port={port}"))
                .arg("--bind-address=127.0.0.1")
                .arg(format!("--socket={}", run_dir.join("mysql.sock").display()))
                .arg("--mysqlx=OFF");
        }
        "postgres" => {
            c.arg("-D")
                .arg(&data)
                .args(["-p", &port, "-c", "listen_addresses=127.0.0.1", "-k"])
                .arg(&run_dir);
        }
        "redis" => {
            c.args(["--port", &port, "--bind", "127.0.0.1", "--dir"])
                .arg(&data);
        }
        _ => {
            c.args([
                "--smtp",
                &format!("127.0.0.1:{port}"),
                "--listen",
                &format!("127.0.0.1:{}", web_port(cfg, id).unwrap_or(MAILPIT_WEB)),
                "--database",
            ])
            .arg(data.join("mailpit.db"));
        }
    }
    c
}

fn pid_file(id: &str) -> PathBuf {
    home().join("run").join(format!("{id}.pids"))
}

/// Running services, keyed by id.
#[derive(Default)]
pub struct Services {
    children: BTreeMap<String, Child>,
}

impl Services {
    pub fn running(&mut self, id: &str) -> bool {
        // Notice services that exited on their own (crash, bad config).
        if let Some(c) = self.children.get_mut(id)
            && !matches!(c.try_wait(), Ok(None))
        {
            self.children.remove(id);
        }
        self.children.contains_key(id)
    }

    pub fn start(&mut self, cfg: &Config, id: &str) -> Result<()> {
        check(id)?;
        if self.running(id) {
            return Ok(());
        }
        if !installed(id) {
            bail!("{} is not installed", name(id));
        }
        kill_orphans(&pid_file(id), false);
        let port = port(cfg, id);
        for p in [Some(port), web_port(cfg, id)].into_iter().flatten() {
            if in_use(p) {
                bail!(
                    "{}: port {p} is already in use by another program — pick another port",
                    name(id)
                );
            }
        }
        for d in ["run", "log"] {
            fs::create_dir_all(home().join(d))?;
        }
        let log = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(home().join("log").join(format!("{id}.log")))?;
        init(id, &log)?;
        let mut child = command(cfg, id)
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log)
            .spawn()
            .with_context(|| format!("failed to start {}", name(id)))?;

        // A server that dies right away (bad data dir, port race) should say so now.
        std::thread::sleep(Duration::from_millis(300));
        if let Ok(Some(status)) = child.try_wait() {
            bail!(
                "{} stopped right after starting ({status}) — see {}",
                name(id),
                home().join("log").join(format!("{id}.log")).display()
            );
        }
        fs::write(
            pid_file(id),
            format!("{}\n{}", std::process::id(), child.id()),
        )?;
        self.children.insert(id.into(), child);
        Ok(())
    }

    pub fn pid(&mut self, id: &str) -> Option<u32> {
        self.running(id).then(|| self.children[id].id())
    }

    pub fn stop(&mut self, id: &str) {
        if let Some(mut c) = self.children.remove(id) {
            // Databases get time to flush and shut down cleanly.
            terminate(&mut c, Duration::from_secs(10));
            let _ = fs::remove_file(pid_file(id));
        }
    }

    /// Start every enabled service; returns the ones that failed with why.
    pub fn start_enabled(&mut self, cfg: &Config) -> Vec<(String, String)> {
        SERVICES
            .iter()
            .filter(|id| cfg.services.get(**id).is_some_and(|s| s.enabled))
            .filter_map(|id| {
                self.start(cfg, id)
                    .err()
                    .map(|e| (id.to_string(), format!("{e:#}")))
            })
            .collect()
    }

    pub fn info(&mut self, cfg: &Config) -> Vec<ServiceInfo> {
        SERVICES
            .iter()
            .map(|id| {
                let running = self.running(id);
                let port = port(cfg, id);
                let web_port = web_port(cfg, id);
                let suggest = |p: u16| (!running && in_use(p)).then(|| next_free(p)).flatten();
                ServiceInfo {
                    id,
                    name: name(id),
                    category: category(id),
                    version: version_of(id),
                    port,
                    web: web_port.map(|p| format!("http://127.0.0.1:{p}")),
                    web_port,
                    installed: installed(id),
                    enabled: cfg.services.get(*id).is_some_and(|s| s.enabled),
                    running,
                    suggested_port: suggest(port),
                    suggested_web_port: web_port.and_then(suggest),
                }
            })
            .collect()
    }
}

impl Drop for Services {
    fn drop(&mut self) {
        let ids: Vec<String> = self.children.keys().cloned().collect();
        for id in ids {
            self.stop(&id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ports_and_ids() {
        let mut cfg = Config::default();
        assert_eq!(port(&cfg, "mysql"), 3306);
        cfg.services.insert(
            "redis".into(),
            ServiceConfig {
                enabled: true,
                port: Some(6380),
                web_port: None,
            },
        );
        assert_eq!(port(&cfg, "redis"), 6380);
        assert!(check("mongodb").is_err());
        assert!(crate::alive(&std::process::id().to_string()));

        let args: Vec<String> = command(&cfg, "redis")
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(args.windows(2).any(|w| w == ["--port", "6380"]));
        assert!(args.windows(2).any(|w| w == ["--bind", "127.0.0.1"]));

        assert_eq!(web_port(&cfg, "redis"), None);
        assert_eq!(web_port(&cfg, "mailpit"), Some(8025));
        cfg.services.entry("mailpit".into()).or_default().web_port = Some(8026);
        let args: Vec<String> = command(&cfg, "mailpit")
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(args.windows(2).any(|w| w == ["--listen", "127.0.0.1:8026"]));
        assert!(args.windows(2).any(|w| w == ["--smtp", "127.0.0.1:1025"]));
    }
}
