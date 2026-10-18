//! Databases, cache and mail, added like containers: pick a kind, name and
//! port, and Anvil runs it. Each kind's program is installed once into
//! `~/.anvil/services/<kind>` and shared; every service has its own port, data
//! in `~/.anvil/data/<id>` and log. Nothing exists until it's added.
//! Everything listens on 127.0.0.1 only and runs independently of the web
//! server, so changing a site never restarts a database.

use crate::{
    Config, fetch_text, fetch_tgz, home, kill_orphans, platform, published_sum, replace_dir, run,
    save, terminate,
};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

pub const SERVICES: [&str; 4] = ["mysql", "postgres", "redis", "mailpit"];

/// Versions offered per kind, newest first; checked against the download sites.
/// Postgres comes from theseus-rs builds, Redis is built from download.redis.io.
pub fn versions(kind: &str) -> &'static [&'static str] {
    match kind {
        "mysql" => &["9.7.2", "8.4.11", "8.0.46"],
        "postgres" => &["18.6.0", "17.11.0", "16.15.0", "15.19.0", "14.24.0"],
        "redis" => &["8.10.2", "7.4.11", "7.2.16"],
        _ => &["1.31.6"],
    }
}

/// SHA-256 of each MySQL download, pinned: MySQL only publishes MD5, which
/// catches a broken download but not a forged one. Taken from files whose MD5
/// matched the published one. Add a row for every version offered above.
const MYSQL_SHA256: [(&str, &str, &str); 6] = [
    (
        "9.7.2",
        "arm64",
        "78bd464c0ff6aacee0a19d12bceff45bd57c62813c3e606b72fab2703d3867a6",
    ),
    (
        "9.7.2",
        "x86_64",
        "c290b3ad553a1d4e9831c4bd94e0107a6dc793d2c2a6e5eb37c2943d6a6b8670",
    ),
    (
        "8.4.11",
        "arm64",
        "b96e00493bc3499b9ffd7f08d65c5d64933af0383a8287d9873b64f94c2d6009",
    ),
    (
        "8.4.11",
        "x86_64",
        "90e8aea10698d01b978f0179e72e8d5e2cbe9f9bd5771e88b0b75d7c82244d3f",
    ),
    (
        "8.0.46",
        "arm64",
        "6ca4a361a45f5019fbf6d500997986b4aaf6f0e30ff9c41179d1e8ecd635f3db",
    ),
    (
        "8.0.46",
        "x86_64",
        "8590bc6c3203fe17f51f757dc9132fb755d1b73cbdad9d12a67f85025a3c3b8f",
    ),
];

/// What a new service gets unless another version is picked (MySQL: the LTS).
pub fn default_version(kind: &str) -> &'static str {
    match kind {
        "mysql" => "8.4.11",
        k => versions(k)[0],
    }
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
pub struct ServiceConfig {
    /// Which program runs it (`redis`). Older configs leave it out and use
    /// the kind as the name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Program version, fixed when the service is added: data folders don't
    /// survive a major version change. Unset means the kind's default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Start with Anvil.
    pub enabled: bool,
    pub port: Option<u16>,
    /// Mailpit's web inbox port.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub web_port: Option<u16>,
    /// Free-form labels, e.g. "queue" or "client-x".
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Applications that use it, by site name.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub apps: Vec<String>,
}

#[derive(Serialize)]
pub struct ServiceInfo {
    /// The name it was added under, e.g. `redis` or `cache`.
    pub id: String,
    pub kind: &'static str,
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
    pub tags: Vec<String>,
    pub apps: Vec<String>,
    /// Lines to paste into an app's `.env` to connect.
    pub env: String,
    pub log: PathBuf,
    /// Set when the port is taken by another program: the next free one.
    pub suggested_port: Option<u16>,
    /// The same for Mailpit's web inbox port.
    pub suggested_web_port: Option<u16>,
}

pub fn name(id: &str) -> &'static str {
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
    (kind_of(cfg, id).ok() == Some("mailpit")).then(|| {
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
        .unwrap_or_else(|| kind_of(cfg, id).map_or(0, default_port))
}

/// The version a service runs.
pub fn version(cfg: &Config, id: &str) -> String {
    cfg.services
        .get(id)
        .and_then(|s| s.version.clone())
        .unwrap_or_else(|| kind_of(cfg, id).map_or("", default_version).to_string())
}

/// The program folder a service runs from.
pub fn program_dir(cfg: &Config, id: &str) -> Result<PathBuf> {
    Ok(dir(kind_of(cfg, id)?, &version(cfg, id)))
}

/// Programs live in `services/<kind>/<version>/`.
fn dir(kind: &str, version: &str) -> PathBuf {
    let root = kind_root(kind);
    // Best effort for a path; `migrate_kind` before a start or an install says why it failed.
    let _ = migrate(&root);
    root.join(version)
}

fn kind_root(kind: &str) -> PathBuf {
    home().join("services").join(kind)
}

/// `migrate` for one kind, with what went wrong.
fn migrate_kind(kind: &str) -> Result<()> {
    migrate(&kind_root(kind))
        .with_context(|| format!("couldn't move {kind} into its version folder"))
}

/// Before versions, a kind's program sat directly in `services/<kind>/`; move it
/// into its version folder. Done in steps through `<kind>.migrating`, so a move
/// that stopped halfway is finished on the next call instead of leaving it there.
fn migrate(root: &Path) -> Result<()> {
    let tmp = root.with_extension("migrating");
    let version = |dir: &Path| -> Option<String> {
        let v = fs::read_to_string(dir.join("VERSION")).ok()?;
        let v = v.trim();
        (!v.is_empty() && !v.contains('/') && !v.starts_with('.')).then(|| v.to_string())
    };
    if !tmp.exists() {
        if version(root).is_none() {
            return Ok(());
        }
        fs::rename(root, &tmp)
            .with_context(|| format!("couldn't rename {} to {}", root.display(), tmp.display()))?;
    }
    let v = version(&tmp).with_context(|| format!("{} has no VERSION", tmp.display()))?;
    fs::create_dir_all(root).with_context(|| format!("couldn't create {}", root.display()))?;
    let to = root.join(v);
    fs::rename(&tmp, &to)
        .with_context(|| format!("couldn't move {} to {}", tmp.display(), to.display()))
}

/// Installed versions of a kind, newest first.
pub fn installed_versions(kind: &str) -> Vec<String> {
    versions(kind)
        .iter()
        .filter(|v| installed(kind, v))
        .map(|v| v.to_string())
        .collect()
}

pub fn data(id: &str) -> PathBuf {
    home().join("data").join(id)
}

fn binary(kind: &str, version: &str) -> PathBuf {
    let d = dir(kind, version);
    match kind {
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

/// The service kind behind an instance id.
pub fn kind_of(cfg: &Config, id: &str) -> Result<&'static str> {
    if let Some(k) = SERVICES.iter().find(|k| **k == id) {
        return Ok(k);
    }
    let kind = cfg.services.get(id).and_then(|s| s.kind.as_deref());
    SERVICES
        .into_iter()
        .find(|k| Some(*k) == kind)
        .with_context(|| format!("Unknown service {id}"))
}

/// Every added service, by name.
pub fn instances(cfg: &Config) -> Vec<String> {
    cfg.services
        .keys()
        .filter(|id| kind_of(cfg, id).is_ok())
        .cloned()
        .collect()
}

#[derive(Serialize)]
pub struct Kind {
    pub kind: &'static str,
    pub name: &'static str,
    pub category: &'static str,
    pub installed: bool,
    pub versions: &'static [&'static str],
    pub default_version: &'static str,
    pub installed_versions: Vec<String>,
    /// Name and port the Add Service sheet starts with.
    pub suggested_name: String,
    pub suggested_port: u16,
    /// The kind's usual port; differs from `suggested_port` when that one is taken.
    pub default_port: u16,
}

/// What can be added, with a free name and port for each.
pub fn kinds(cfg: &Config) -> Vec<Kind> {
    SERVICES
        .into_iter()
        .map(|kind| {
            let taken = ports_taken(cfg, "");
            Kind {
                kind,
                name: name(kind),
                category: category(kind),
                installed: installed(kind, default_version(kind)),
                versions: versions(kind),
                default_version: default_version(kind),
                installed_versions: installed_versions(kind),
                suggested_name: free_name(cfg, kind),
                suggested_port: free_port(&taken, default_port(kind)),
                default_port: default_port(kind),
            }
        })
        .collect()
}

const ADJECTIVES: [&str; 24] = [
    "bold", "bright", "brisk", "calm", "eager", "gentle", "golden", "keen", "lucky", "noble",
    "proud", "quick", "quiet", "rapid", "sharp", "silver", "solid", "steady", "swift", "tidy",
    "warm", "wise", "lively", "sturdy",
];
const NOUNS: [&str; 24] = [
    "anvil", "bellows", "blade", "bronze", "chisel", "cinder", "copper", "crucible", "ember",
    "flint", "forge", "hammer", "ingot", "iron", "kiln", "rivet", "spark", "steel", "tongs",
    "furnace", "smelter", "nail", "chain", "bolt",
];

/// A readable name like `redis-swift-ember` that no service has yet.
fn free_name(cfg: &Config, kind: &str) -> String {
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos() as usize ^ d.as_secs() as usize);
    (0..ADJECTIVES.len() * NOUNS.len())
        .map(|i| {
            let n = seed.wrapping_add(i.wrapping_mul(7919));
            format!(
                "{kind}-{}-{}",
                ADJECTIVES[n % ADJECTIVES.len()],
                NOUNS[(n / ADJECTIVES.len()) % NOUNS.len()]
            )
        })
        .chain((2..).map(|i| format!("{kind}-{i}")))
        .find(|n| !cfg.services.contains_key(n))
        .unwrap()
}

/// `from` or the next port no service and no other program uses.
fn free_port(taken: &[u16], from: u16) -> u16 {
    (from..u16::MAX)
        .find(|p| !taken.contains(p) && !in_use(*p))
        .unwrap_or(from)
}

/// Whether a version of a kind's program is installed.
pub fn installed(kind: &str, version: &str) -> bool {
    binary(kind, version).exists()
}

/// Ports every other instance uses (service and web), for clash checks.
fn ports_taken(cfg: &Config, except: &str) -> Vec<u16> {
    instances(cfg)
        .iter()
        .filter(|i| *i != except)
        .flat_map(|i| [Some(port(cfg, i)), web_port(cfg, i)])
        .flatten()
        .collect()
}

/// Add a service: `name` defaults to the kind (`redis`, then `redis-2`…),
/// `port` to its usual one or the next free. Returns the name.
pub fn add(
    cfg: &mut Config,
    kind: &str,
    name: Option<&str>,
    port: Option<u16>,
    version: Option<&str>,
) -> Result<String> {
    let id = insert(cfg, kind, name, port, version)?;
    save(cfg)?;
    Ok(id)
}

fn check_name(id: &str) -> Result<()> {
    let ok = id.len() <= 40
        && id.starts_with(|c: char| c.is_ascii_lowercase())
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !ok {
        bail!("Use lowercase letters, numbers and dashes for the service name");
    }
    Ok(())
}

fn insert(
    cfg: &mut Config,
    kind: &str,
    name: Option<&str>,
    port: Option<u16>,
    version: Option<&str>,
) -> Result<String> {
    check(kind)?;
    let version = version.unwrap_or(default_version(kind));
    if !versions(kind).contains(&version) {
        bail!(
            "Anvil offers {} {}",
            self::name(kind),
            versions(kind).join(", ")
        );
    }
    let id = name.map_or_else(|| free_name(cfg, kind), str::to_string);
    check_name(&id)?;
    if cfg.services.contains_key(&id) {
        bail!("There's already a service called {id}");
    }
    let mut taken = ports_taken(cfg, "");
    let port = match port {
        Some(p) => {
            check_port(cfg, "", p)?;
            // Said now rather than after a download that can take minutes.
            if in_use(p) {
                bail!("Port {p} is in use by another program; pick another one");
            }
            p
        }
        None => free_port(&taken, default_port(kind)),
    };
    taken.push(port);
    let web = (kind == "mailpit").then(|| free_port(&taken, MAILPIT_WEB));
    cfg.services.insert(
        id.clone(),
        ServiceConfig {
            kind: Some(kind.into()),
            enabled: false,
            port: Some(port),
            web_port: web,
            version: Some(version.into()),
            ..Default::default()
        },
    );
    Ok(id)
}

/// Remove a service. Its data stays unless `delete_data`, so adding the same
/// name again finds the databases; the program stays for other services.
pub fn remove(cfg: &mut Config, id: &str, delete_data: bool) -> Result<()> {
    if !cfg.services.contains_key(id) {
        bail!("There's no service called {id}");
    }
    check_name(id)?;
    cfg.services.remove(id);
    save(cfg)?;
    if delete_data {
        match fs::remove_dir_all(data(id)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
    }
    Ok(())
}

/// Download (or for Redis, build) a version into `~/.anvil/services/<kind>/<version>`.
pub fn install(id: &str, version: &str) -> Result<()> {
    check(id)?;
    if !versions(id).contains(&version) {
        bail!("Anvil offers {} {}", name(id), versions(id).join(", "));
    }
    let (_, caddy_plat) = platform()?;
    let mac = caddy_plat.starts_with("mac");
    let arm = caddy_plat.ends_with("arm64");
    replace_dir(&dir(id, version), |d| {
        match id {
            "mysql" => {
                // ponytail: macOS builds only; Linux uses the glibc .tar.xz with the Linux port.
                if !mac {
                    bail!("MySQL install is macOS-only for now");
                }
                let arch = if arm { "arm64" } else { "x86_64" };
                let minor = version.rsplit_once('.').map_or(version, |m| m.0);
                let file = format!("mysql-{version}-macos15-{arch}.tar.gz");
                let url = format!("https://cdn.mysql.com/Downloads/MySQL-{minor}/{file}");
                let sum = MYSQL_SHA256
                    .iter()
                    .find(|(v, a, _)| *v == version && *a == arch)
                    .map(|(_, _, s)| *s)
                    .with_context(|| format!("No pinned checksum for {file}"))?;
                fetch_tgz(&url, d, true, Some(sum))
            }
            "postgres" => {
                let target = match (mac, arm) {
                    (true, true) => "aarch64-apple-darwin",
                    (true, false) => "x86_64-apple-darwin",
                    (false, true) => "aarch64-unknown-linux-gnu",
                    (false, false) => "x86_64-unknown-linux-gnu",
                };
                let file = format!("postgresql-{version}-{target}.tar.gz");
                let url = format!(
                    "https://github.com/theseus-rs/postgresql-binaries/releases/download/{version}/{file}"
                );
                let sum = published_sum(&format!("{url}.sha256"), &file)?;
                fetch_tgz(&url, d, true, Some(&sum))
            }
            "redis" => build_redis(d, version),
            _ => {
                let os = if mac { "darwin" } else { "linux" };
                let arch = if arm { "arm64" } else { "amd64" };
                let file = format!("mailpit-{os}-{arch}.tar.gz");
                let sum = github_digest("axllent/mailpit", &format!("v{version}"), &file)?;
                fetch_tgz(
                    &format!(
                        "https://github.com/axllent/mailpit/releases/download/v{version}/{file}"
                    ),
                    d,
                    false,
                    Some(&sum),
                )
            }
        }?;
        fs::write(d.join("VERSION"), version)?;
        Ok(())
    })
}

/// Mailpit publishes no checksums file; GitHub records a SHA-256 of every
/// release asset at upload, so check against that.
fn github_digest(repo: &str, tag: &str, file: &str) -> Result<String> {
    let release: serde_json::Value = serde_json::from_str(&fetch_text(&format!(
        "https://api.github.com/repos/{repo}/releases/tags/{tag}"
    ))?)?;
    release["assets"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|a| a["name"] == file)
        .and_then(|a| a["digest"].as_str()?.strip_prefix("sha256:"))
        .map(String::from)
        .with_context(|| format!("GitHub lists no checksum for {file}"))
}

/// Redis publishes no macOS binaries, so build it (needs the Xcode command line tools).
fn build_redis(prefix: &std::path::Path, version: &str) -> Result<()> {
    let src = home().join("build").join(format!("redis-{version}"));
    let _ = fs::remove_dir_all(&src);
    let file = format!("redis-{version}.tar.gz");
    let sum = published_sum(
        "https://raw.githubusercontent.com/redis/redis-hashes/master/README",
        &file,
    )?;
    fetch_tgz(
        &format!("https://download.redis.io/releases/{file}"),
        &src,
        true,
        Some(&sum),
    )?;
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

/// Download a service's program version if it isn't there yet.
pub fn ensure_installed(cfg: &Config, id: &str) -> Result<()> {
    let (kind, v) = (kind_of(cfg, id)?, version(cfg, id));
    migrate_kind(kind)?;
    if installed(kind, &v) {
        return Ok(());
    }
    install(kind, &v)
}

/// Remove one version of a program; service data is never touched.
pub fn uninstall(kind: &str, version: &str) -> Result<()> {
    check(kind)?;
    if !versions(kind).contains(&version) {
        bail!("Unknown {} version {version}", name(kind));
    }
    fs::remove_dir_all(dir(kind, version))
        .with_context(|| format!("{} {version} is not installed", name(kind)))
}

/// Replace a service's tags; blanks and duplicates are dropped.
pub fn set_tags(cfg: &mut Config, id: &str, tags: &[String]) -> Result<()> {
    let mut clean: Vec<String> = vec![];
    for t in tags.iter().map(|t| t.trim()).filter(|t| !t.is_empty()) {
        if t.len() > 30 || t.contains(',') {
            bail!("Keep tags short and without commas");
        }
        if !clean.iter().any(|c| c == t) {
            clean.push(t.to_string());
        }
    }
    entry(cfg, id)?.tags = clean;
    save(cfg)
}

/// Which applications use a service.
pub fn set_apps(cfg: &mut Config, id: &str, apps: &[String]) -> Result<()> {
    let mut list: Vec<String> = vec![];
    for a in apps.iter().filter(|a| !a.is_empty()) {
        if !list.contains(a) {
            list.push(a.clone());
        }
    }
    entry(cfg, id)?.apps = list;
    save(cfg)
}

fn entry<'a>(cfg: &'a mut Config, id: &str) -> Result<&'a mut ServiceConfig> {
    cfg.services
        .get_mut(id)
        .with_context(|| format!("There's no service called {id}"))
}

/// What an app puts in `.env` to use this service (Laravel names plus a URL).
pub fn env_lines(cfg: &Config, id: &str) -> String {
    let p = port(cfg, id);
    match kind_of(cfg, id).unwrap_or("redis") {
        "mysql" => format!(
            "DB_CONNECTION=mysql\nDB_HOST=127.0.0.1\nDB_PORT={p}\nDB_USERNAME=root\nDB_PASSWORD=\nDATABASE_URL=mysql://root@127.0.0.1:{p}/"
        ),
        "postgres" => format!(
            "DB_CONNECTION=pgsql\nDB_HOST=127.0.0.1\nDB_PORT={p}\nDB_USERNAME=postgres\nDB_PASSWORD=\nDATABASE_URL=postgres://postgres@127.0.0.1:{p}/postgres"
        ),
        "redis" => format!("REDIS_HOST=127.0.0.1\nREDIS_PORT={p}\nREDIS_URL=redis://127.0.0.1:{p}"),
        _ => format!("MAIL_MAILER=smtp\nMAIL_HOST=127.0.0.1\nMAIL_PORT={p}"),
    }
}

pub fn set_enabled(cfg: &mut Config, id: &str, enabled: bool) -> Result<()> {
    kind_of(cfg, id)?;
    cfg.services.entry(id.into()).or_default().enabled = enabled;
    save(cfg)
}

pub fn set_port(cfg: &mut Config, id: &str, port: Option<u16>) -> Result<()> {
    let kind = kind_of(cfg, id)?;
    check_port(cfg, id, port.unwrap_or(default_port(kind)))?;
    cfg.services.entry(id.into()).or_default().port = port;
    save(cfg)
}

fn check_port(cfg: &Config, id: &str, port: u16) -> Result<()> {
    if port < 1024 {
        bail!("Use a port of 1024 or higher");
    }
    if ports_taken(cfg, id).contains(&port) {
        bail!("Another Anvil service already uses port {port}");
    }
    if [cfg.http_port, cfg.https_port, cfg.dns_port].contains(&port) {
        bail!("Port {port} is Anvil's own web or DNS port");
    }
    Ok(())
}

pub fn set_web_port(cfg: &mut Config, id: &str, port: Option<u16>) -> Result<()> {
    if kind_of(cfg, id)? != "mailpit" {
        bail!("Only Mailpit has a web port");
    }
    check_port(cfg, id, port.unwrap_or(MAILPIT_WEB))?;
    cfg.services.entry(id.into()).or_default().web_port = port;
    save(cfg)
}

/// First start: create the data directory the way each server expects.
fn init(cfg: &Config, kind: &str, id: &str, log: &fs::File) -> Result<()> {
    let data = data(id);
    if data.exists() {
        return Ok(());
    }
    let d = dir(kind, &version(cfg, id));
    let status = match kind {
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
        bail!("Setting up {id} failed — see {}", log_file(id).display());
    }
    Ok(())
}

fn command(cfg: &Config, id: &str) -> Command {
    let kind = kind_of(cfg, id).unwrap_or("redis");
    let v = version(cfg, id);
    let (d, data, run_dir) = (dir(kind, &v), data(id), home().join("run"));
    let port = port(cfg, id).to_string();
    let mut c = Command::new(binary(kind, &v));
    match kind {
        "mysql" => {
            c.arg("--no-defaults")
                .arg(format!("--basedir={}", d.display()))
                .arg(format!("--datadir={}", data.display()))
                .arg(format!("--port={port}"))
                .arg("--bind-address=127.0.0.1")
                .arg(format!(
                    "--socket={}",
                    run_dir.join(format!("{id}.sock")).display()
                ))
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

/// `svc-<id>`: a service named `app-blog` or `caddy` gets files of its own instead of
/// the app `blog`'s or the web server's.
fn pid_file(id: &str) -> PathBuf {
    pid_in(&home(), id)
}

fn pid_in(home: &Path, id: &str) -> PathBuf {
    home.join("run").join(format!("svc-{id}.pids"))
}

pub fn log_file(id: &str) -> PathBuf {
    log_in(&home(), id)
}

fn log_in(home: &Path, id: &str) -> PathBuf {
    home.join("log").join(format!("svc-{id}.log"))
}

/// The pid file name from before `svc-`: still read, for a service an older Anvil
/// started and still runs (`migrate_files` leaves that one where it is).
fn legacy_pid_file(id: &str) -> PathBuf {
    home().join("run").join(format!("{id}.pids"))
}

/// Move the services' pid files and logs from `<id>.pids`/`<id>.log` to the `svc-`
/// names. Run at startup, before `apps::kill_all_orphans` (it would take a service's
/// `app-<x>.pids` for an app's). A pid file whose Anvil still runs stays: that Anvil
/// removes it by its old name when it stops the service. A log that could be another
/// one's (`caddy.log`, `app-*`, `create-*`, `php-*`) stays too.
pub fn migrate_files(cfg: &Config) {
    migrate_files_in(&home(), &instances(cfg));
}

fn migrate_files_in(home: &Path, ids: &[String]) {
    for id in ids {
        let old = home.join("run").join(format!("{id}.pids"));
        if let Ok(f) = fs::read_to_string(&old) {
            let mut lines = f.lines();
            let owner_gone = !lines.next().is_some_and(crate::alive);
            // An app's pid file has "<pid> <start time>" lines; a service's only pids.
            let a_service_s = !id.starts_with("app-") || lines.all(|l| !l.contains(' '));
            let new = pid_in(home, id);
            if owner_gone && a_service_s && !new.exists() {
                let _ = fs::rename(&old, new);
            }
        }
        let shared = id == "caddy"
            || ["app-", "create-", "php-"]
                .iter()
                .any(|p| id.starts_with(p));
        let (old, new) = (home.join("log").join(format!("{id}.log")), log_in(home, id));
        if !shared && old.is_file() && !new.exists() {
            // Safe while a service still writes to it: its open file moves along.
            let _ = fs::rename(old, new);
        }
    }
}

/// Running services, keyed by id.
#[derive(Default)]
pub struct Services {
    children: BTreeMap<String, Child>,
    /// Ids between `begin` and `finish`: a second start of one leaves it alone.
    starting: std::collections::BTreeSet<String>,
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
        if !self.begin(cfg, id)? {
            return Ok(());
        }
        let launched = launch(cfg, id);
        self.finish(id, launched)
    }

    /// A start in three steps, so `Services` needn't stay locked through `launch` (an
    /// init and a 300 ms check): false when it already runs or another start is on it.
    pub fn begin(&mut self, cfg: &Config, id: &str) -> Result<bool> {
        kind_of(cfg, id)?;
        Ok(!self.running(id) && self.starting.insert(id.into()))
    }

    /// Record what `launch` did after `begin`.
    pub fn finish(&mut self, id: &str, launched: Result<Child>) -> Result<()> {
        self.starting.remove(id);
        self.children.insert(id.into(), launched?);
        Ok(())
    }

    /// Running services and their pids, to look at without holding on to `Services`.
    pub fn pids(&mut self) -> BTreeMap<String, u32> {
        let ids: Vec<String> = self.children.keys().cloned().collect();
        ids.into_iter()
            .filter_map(|id| self.pid(&id).map(|p| (id, p)))
            .collect()
    }

    pub fn pid(&mut self, id: &str) -> Option<u32> {
        self.running(id).then(|| self.children[id].id())
    }

    pub fn stop(&mut self, id: &str) {
        if let Some(c) = self.take(id) {
            halt(id, c);
        }
    }

    /// Forget a running service and hand back its process, to `halt` once the lock on
    /// `Services` is released.
    pub fn take(&mut self, id: &str) -> Option<Child> {
        self.children.remove(id)
    }

    /// Start every enabled service; returns the ones that failed with why.
    pub fn start_enabled(&mut self, cfg: &Config) -> Vec<(String, String)> {
        enabled(cfg)
            .iter()
            .filter_map(|id| {
                self.start(cfg, id)
                    .err()
                    .map(|e| (id.to_string(), format!("{e:#}")))
            })
            .collect()
    }

    pub fn info(&mut self, cfg: &Config) -> Vec<ServiceInfo> {
        instances(cfg)
            .into_iter()
            .map(|id| {
                let kind = kind_of(cfg, &id).unwrap_or("redis");
                let running = self.running(&id);
                let port = port(cfg, &id);
                let web_port = web_port(cfg, &id);
                let suggest = |p: u16| (!running && in_use(p)).then(|| next_free(p)).flatten();
                ServiceInfo {
                    kind,
                    name: name(kind),
                    category: category(kind),
                    version: version(cfg, &id),
                    port,
                    web: web_port.map(|p| format!("http://127.0.0.1:{p}")),
                    web_port,
                    installed: installed(kind, &version(cfg, &id)),
                    enabled: cfg.services.get(&id).is_some_and(|s| s.enabled),
                    running,
                    suggested_port: suggest(port),
                    suggested_web_port: web_port.and_then(suggest),
                    tags: cfg
                        .services
                        .get(&id)
                        .map(|s| s.tags.clone())
                        .unwrap_or_default(),
                    apps: cfg
                        .services
                        .get(&id)
                        .map(|s| s.apps.clone())
                        .unwrap_or_default(),
                    env: env_lines(cfg, &id),
                    log: log_file(&id),
                    id,
                }
            })
            .collect()
    }
}

/// Services that start with Anvil.
pub fn enabled(cfg: &Config) -> Vec<String> {
    instances(cfg)
        .into_iter()
        .filter(|id| cfg.services.get(id).is_some_and(|s| s.enabled))
        .collect()
}

/// Stop a service taken out of `Services`. Databases get 10 s to flush and shut down cleanly.
pub fn halt(id: &str, mut c: Child) {
    terminate(&mut c, Duration::from_secs(10));
    crate::remove_pid_file(&pid_file(id), c.id());
    crate::remove_pid_file(&legacy_pid_file(id), c.id());
}

/// Everything of a start but the bookkeeping (`Services::begin`/`finish`): check, init,
/// spawn and see that it stays up.
pub fn launch(cfg: &Config, id: &str) -> Result<Child> {
    let kind = kind_of(cfg, id)?;
    let v = version(cfg, id);
    migrate_kind(kind)?;
    if !installed(kind, &v) {
        bail!("{} {v} is not installed", name(kind));
    }
    kill_orphans(&pid_file(id), false);
    if !id.starts_with("app-") {
        // Left by an Anvil from before `svc-` (an `app-` one is the app's to clean up).
        kill_orphans(&legacy_pid_file(id), false);
    }
    let port = port(cfg, id);
    for p in [Some(port), web_port(cfg, id)].into_iter().flatten() {
        if in_use(p) {
            bail!("{id}: port {p} is already in use by another program — pick another port");
        }
    }
    crate::state_dirs()?;
    let log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file(id))?;
    init(cfg, kind, id, &log)?;
    let mut child = command(cfg, id)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .spawn()
        .with_context(|| format!("failed to start {id}"))?;

    // A server that dies right away (bad data dir, port race) should say so now.
    std::thread::sleep(Duration::from_millis(300));
    if let Ok(Some(status)) = child.try_wait() {
        bail!(
            "{id} stopped right after starting ({status}) — see {}",
            log_file(id).display()
        );
    }
    fs::write(
        pid_file(id),
        format!("{}\n{}", std::process::id(), child.id()),
    )?;
    Ok(child)
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
    fn a_program_moves_into_its_version_folder_even_after_an_interrupted_move() {
        let base = std::env::temp_dir().join(format!("anvil-migrate-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let root = base.join("redis");
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::write(root.join("VERSION"), "7.2.16\n").unwrap();
        migrate(&root).unwrap();
        assert!(root.join("7.2.16/bin").is_dir() && !base.join("redis.migrating").exists());
        migrate(&root).unwrap(); // done: nothing to do

        // Stopped after the first step: the program waits in `redis.migrating`.
        let pg = base.join("postgres");
        let tmp = base.join("postgres.migrating");
        fs::create_dir_all(&tmp).unwrap();
        fs::write(tmp.join("VERSION"), "17.2").unwrap();
        migrate(&pg).unwrap();
        assert!(pg.join("17.2/VERSION").is_file() && !tmp.exists());

        // A move that can't finish says so, and what's there stays.
        fs::create_dir_all(&tmp).unwrap();
        fs::write(tmp.join("VERSION"), "17.2").unwrap();
        fs::write(pg.join("17.2/keep"), "x").unwrap();
        let e = format!("{:#}", migrate(&pg).unwrap_err());
        assert!(e.contains("postgres.migrating"), "{e}");
        assert!(tmp.join("VERSION").is_file() && pg.join("17.2/keep").is_file());
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn service_files_move_to_svc_names_and_never_take_anothers() {
        let home = std::env::temp_dir().join(format!("anvil-svc-files-{}", std::process::id()));
        let _ = fs::remove_dir_all(&home);
        crate::state_dirs_in(&home).unwrap();
        let (run, log) = (home.join("run"), home.join("log"));
        // A service named like an app's files and one named like the web server's log.
        assert_ne!(pid_in(&home, "app-blog"), run.join("app-blog.pids"));
        assert_ne!(log_in(&home, "caddy"), log.join("caddy.log"));

        let dead = "999999"; // no such process: its Anvil is gone
        let me = std::process::id().to_string();
        fs::write(run.join("redis.pids"), format!("{dead}\n123")).unwrap();
        fs::write(log.join("redis.log"), "old output").unwrap();
        fs::write(run.join("mysql.pids"), format!("{me}\n456")).unwrap(); // its Anvil runs
        fs::write(log.join("caddy.log"), "web server").unwrap();
        fs::write(run.join("app-blog.pids"), format!("{dead}\n789 Mon Oct 12")).unwrap();
        let ids = ["redis", "mysql", "caddy", "app-blog"].map(String::from);
        migrate_files_in(&home, &ids);

        assert_eq!(
            fs::read_to_string(pid_in(&home, "redis")).unwrap(),
            format!("{dead}\n123")
        );
        assert!(!run.join("redis.pids").exists());
        assert_eq!(
            fs::read_to_string(log_in(&home, "redis")).unwrap(),
            "old output"
        );
        assert!(run.join("mysql.pids").exists() && !pid_in(&home, "mysql").exists());
        assert_eq!(
            fs::read_to_string(log.join("caddy.log")).unwrap(),
            "web server"
        );
        assert!(!log_in(&home, "caddy").exists());
        // The app blog's pid file (with start times) is not the service's.
        assert!(run.join("app-blog.pids").exists() && !pid_in(&home, "app-blog").exists());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn every_mysql_version_has_pinned_checksums() {
        for v in versions("mysql") {
            for arch in ["arm64", "x86_64"] {
                let row = MYSQL_SHA256.iter().find(|(pv, a, _)| pv == v && *a == arch);
                assert!(
                    row.is_some_and(|(_, _, s)| s.len() == 64),
                    "MySQL {v} {arch}"
                );
            }
        }
    }

    #[test]
    fn ports_and_ids() {
        let mut cfg = Config::default();
        assert_eq!(port(&cfg, "mysql"), 3306);
        cfg.services.insert(
            "redis".into(),
            ServiceConfig {
                enabled: true,
                port: Some(6380),
                ..Default::default()
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

    #[test]
    fn added_services() {
        let mut cfg = Config::default();
        assert!(instances(&cfg).is_empty());
        assert_eq!(
            insert(&mut cfg, "redis", Some("redis"), Some(16379), None).unwrap(),
            "redis"
        );
        let id = insert(&mut cfg, "redis", Some("redis-2"), None, None).unwrap();
        let auto = insert(&mut cfg, "redis", None, None, None).unwrap();
        assert!(
            auto.starts_with("redis-") && auto.matches('-').count() == 2,
            "{auto}"
        );
        assert!(check_name(&auto).is_ok());
        cfg.services.remove(&auto);
        assert_eq!(kind_of(&cfg, &id).unwrap(), "redis");
        let p = port(&cfg, &id);
        assert_ne!(p, 16379);
        assert!(insert(&mut cfg, "redis", Some("cache"), Some(p), None).is_err()); // port taken
        assert!(insert(&mut cfg, "redis", Some("redis-2"), None, None).is_err());
        assert!(insert(&mut cfg, "redis", Some("../x"), None, None).is_err());
        assert!(insert(&mut cfg, "mongodb", None, None, None).is_err());
        assert!(insert(&mut cfg, "redis", Some("old"), None, Some("6.0.0")).is_err());
        let old = insert(&mut cfg, "redis", Some("old"), None, Some("7.2.16")).unwrap();
        assert_eq!(version(&cfg, &old), "7.2.16");
        assert_eq!(version(&cfg, &id), "8.10.2");
        assert_eq!(command(&cfg, &old).get_program(), binary("redis", "7.2.16"));
        cfg.services.remove(&old);
        assert!(check_port(&cfg, "redis", p).is_err());
        assert!(check_port(&cfg, "redis-2", p).is_ok());
        // Anvil's own ports are never a service's.
        for own in [cfg.http_port, cfg.https_port, cfg.dns_port] {
            assert!(check_port(&cfg, "redis-2", own).is_err());
        }

        // Each service gets its own data, log and socket; the program is shared.
        let args: Vec<String> = command(&cfg, &id)
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(args.iter().any(|a| a.ends_with("data/redis-2")));
        assert_eq!(command(&cfg, &id).get_program(), binary("redis", "8.10.2"));

        let a = insert(&mut cfg, "mailpit", Some("mail-a"), None, None).unwrap();
        let b = insert(&mut cfg, "mailpit", Some("mail-b"), None, None).unwrap();
        assert_ne!(web_port(&cfg, &a), web_port(&cfg, &b));
        assert_eq!(instances(&cfg), ["mail-a", "mail-b", "redis", "redis-2"]);
    }
}
