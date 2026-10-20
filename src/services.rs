//! Databases, cache, mail and S3 storage, added like containers: pick a kind, name and
//! port, and Anvil runs it. Each kind's program is installed once into
//! `~/.anvil/services/<kind>` and shared; every service has its own port, data
//! in `~/.anvil/data/<id>` and log. Nothing exists until it's added.
//! Everything listens on 127.0.0.1 only and runs independently of the web
//! server, so changing a site never restarts a database.

use crate::{
    Config, Stopping, fetch_text, fetch_tgz, home, kill_orphans, platform, published_sum,
    replace_dir, run, save,
};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

pub const SERVICES: [&str; 6] = ["mysql", "postgres", "redis", "mailpit", "versitygw", "silo"];

/// Versions offered per kind, newest first; checked against the download sites.
/// Postgres comes from theseus-rs builds, Redis is built from download.redis.io.
pub fn versions(kind: &str) -> &'static [&'static str] {
    match kind {
        "mysql" => &["9.7.2", "8.4.11", "8.0.46"],
        "postgres" => &["18.6.0", "17.11.0", "16.15.0", "15.19.0", "14.24.0"],
        // Windows: Microsoft's Garnet, which speaks Redis's protocol; Redis has no Windows build.
        "redis" if cfg!(windows) => &[GARNET_VERSION],
        "redis" => &["8.10.2", "7.4.11", "7.2.16"],
        "versitygw" => &["1.9.0"],
        // SILO's own version number; its release is RELEASE.2026-10-18T00-00-00Z.
        "silo" => &["20261018000000"],
        _ => &["1.31.6"],
    }
}

/// Garnet, Anvil's Redis on Windows, and the .NET runtime it runs on.
const GARNET_VERSION: &str = "2.2.3";
const GARNET_DOTNET: &str = "10.0";

/// The license a service's program comes under, shown before it is installed.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct License {
    /// What accepting is recorded under, e.g. "redis:RSALv2-SSPLv1-AGPLv3".
    pub id: String,
    /// The license as people know it.
    pub name: &'static str,
    /// The official text.
    pub url: &'static str,
}

/// Each kind's license; Redis changed licenses between versions, so it goes by version.
pub fn license(kind: &str, version: &str) -> License {
    let (key, name, url) = match kind {
        "mysql" => (
            "GPL-2.0-only",
            "GPL-2.0 with the Universal FOSS Exception",
            "https://github.com/mysql/mysql-server/blob/trunk/LICENSE",
        ),
        "postgres" => (
            "PostgreSQL",
            "the PostgreSQL License",
            "https://www.postgresql.org/about/licence/",
        ),
        "redis" if cfg!(windows) => (
            "Garnet-MIT",
            "the MIT License (Microsoft Garnet)",
            "https://github.com/microsoft/garnet/blob/main/LICENSE",
        ),
        "redis" if version.starts_with("7.2") => (
            "BSD-3-Clause",
            "BSD-3-Clause",
            "https://github.com/redis/redis/blob/7.2/COPYING",
        ),
        "redis" if version.starts_with("7.") => (
            "RSALv2-SSPLv1",
            "RSALv2 or SSPLv1, your choice",
            "https://redis.io/legal/licenses/",
        ),
        "redis" => (
            "RSALv2-SSPLv1-AGPLv3",
            "RSALv2, SSPLv1 or AGPLv3, your choice",
            "https://redis.io/legal/licenses/",
        ),
        "versitygw" => (
            "Apache-2.0",
            "the Apache License 2.0",
            "https://github.com/versity/versitygw/blob/main/LICENSE",
        ),
        "silo" => (
            "AGPL-3.0",
            "the GNU Affero General Public License 3.0",
            "https://github.com/pgsty/silo/blob/main/LICENSE",
        ),
        _ => (
            "MIT",
            "the MIT License",
            "https://github.com/axllent/mailpit/blob/develop/LICENSE",
        ),
    };
    License {
        id: format!("{kind}:{key}"),
        name,
        url,
    }
}

/// When the license of this kind and version was accepted, if it was.
pub fn license_accepted<'a>(cfg: &'a Config, kind: &str, version: &str) -> Option<&'a str> {
    cfg.accepted_licenses
        .get(&license(kind, version).id)
        .map(String::as_str)
}

/// Record that the user accepted the license of this kind and version, today.
pub fn accept_license(cfg: &mut Config, kind: &str, version: &str) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    cfg.accepted_licenses
        .entry(license(kind, version).id)
        .or_insert_with(|| ymd(secs));
}

/// "2026-10-18" (UTC) for a Unix time: the civil-from-days algorithm (H. Hinnant).
fn ymd(secs: u64) -> String {
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Adding needs the license accepted, now (`accept`) or before.
pub fn require_license(cfg: &mut Config, kind: &str, version: &str, accept: bool) -> Result<()> {
    if accept {
        accept_license(cfg, kind, version);
    }
    if license_accepted(cfg, kind, version).is_none() {
        let l = license(kind, version);
        bail!(
            "{} {version} comes under {}; accept its license first ({})",
            name(kind),
            l.name,
            l.url
        );
    }
    Ok(())
}

/// SHA-256 of each MySQL download, pinned: MySQL only publishes MD5, which
/// catches a broken download but not a forged one. Taken from files whose MD5
/// matched the published one. Add a row for every version offered above.
const MYSQL_SHA256: [(&str, &str, &str); 9] = [
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
    (
        "9.7.2",
        "winx64",
        "5592ea38e53edd67f5baa68303e7fcf2f0a40fc08978fdb4a7b0c38d12baea17",
    ),
    (
        "8.4.11",
        "winx64",
        "a492371d687d2bab088b0062581144a0044b8964baefdf4faa579292b423d25c",
    ),
    (
        "8.0.46",
        "winx64",
        "28e9eda019d88eff4478d811ea2110b83f02a3966be157fe91cc55def3ab0d4d",
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
    /// Mailpit's web inbox or S3 storage's web console port.
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
    /// Mailpit's web inbox, S3 storage's web console.
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
        "redis" if cfg!(windows) => "Redis (Garnet)",
        "redis" => "Redis",
        "versitygw" => "Versity S3 Gateway",
        "silo" => "SILO (MinIO fork)",
        _ => "Mailpit",
    }
}

fn category(id: &str) -> &'static str {
    match id {
        "mysql" | "postgres" => "Database",
        "redis" => "Cache & Queue",
        "versitygw" | "silo" => "Object Storage",
        _ => "Mail",
    }
}

use crate::port_answers as in_use;

/// The next free port after `port`, for when another program holds it.
fn next_free(port: u16) -> Option<u16> {
    crate::free_port(&[], port + 1..port.saturating_add(100))
}

fn default_port(id: &str) -> u16 {
    match id {
        "mysql" => 3306,
        "postgres" => 5432,
        "redis" => 6379,
        "versitygw" => 7070,
        "silo" => 9000,
        _ => 1025, // Mailpit SMTP
    }
}

/// The usual port of a kind's web page: Mailpit's inbox, S3 storage's console.
fn default_web_port(kind: &str) -> Option<u16> {
    match kind {
        "mailpit" => Some(8025),
        "versitygw" => Some(7071),
        "silo" => Some(9001),
        _ => None,
    }
}

/// The web page's port, for the kinds that have one.
pub fn web_port(cfg: &Config, id: &str) -> Option<u16> {
    let usual = default_web_port(kind_of(cfg, id).ok()?)?;
    Some(
        cfg.services
            .get(id)
            .and_then(|s| s.web_port)
            .unwrap_or(usual),
    )
}

/// S3 storage's one account and the bucket its first start makes. Fixed: it
/// listens on 127.0.0.1 only, and the .env lines show them anyway.
pub const S3_ACCESS_KEY: &str = "anvil";
pub const S3_SECRET_KEY: &str = "anvil-secret";
pub const S3_BUCKET: &str = "anvil";

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
    let exe = crate::platform::exe;
    match kind {
        "mysql" => d.join("bin").join(exe("mysqld")),
        "postgres" => d.join("bin").join(exe("postgres")),
        "redis" if cfg!(windows) => d.join("garnet").join("GarnetServer.exe"),
        "redis" => d.join("bin/redis-server"),
        "versitygw" => d.join(exe("versitygw")),
        "silo" => d.join(exe("silo")),
        _ => d.join(exe("mailpit")),
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
    /// Each version's license, and when it was accepted (None: not yet).
    pub licenses: Vec<(String, License, Option<String>)>,
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
                licenses: versions(kind)
                    .iter()
                    .map(|v| {
                        let accepted = license_accepted(cfg, kind, v).map(String::from);
                        (v.to_string(), license(kind, v), accepted)
                    })
                    .collect(),
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
    crate::free_port(taken, from..u16::MAX).unwrap_or(from)
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
    crate::validate::service_id(&id)?;
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
    let web = default_web_port(kind).map(|p| free_port(&taken, p));
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
    crate::validate::service_id(id)?;
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
    let windows = cfg!(windows);
    // Windows downloads are zips; Windows' tar reads them (`fetch_tgz`).
    let archive = if windows { "zip" } else { "tar.gz" };
    replace_dir(&dir(id, version), |d| {
        match id {
            "mysql" => {
                // ponytail: macOS and Windows builds only; Linux uses the glibc .tar.xz with the Linux port.
                if !mac && !windows {
                    bail!("MySQL install is macOS-only for now");
                }
                let arch = if windows {
                    "winx64"
                } else if arm {
                    "arm64"
                } else {
                    "x86_64"
                };
                let minor = version.rsplit_once('.').map_or(version, |m| m.0);
                let file = if windows {
                    format!("mysql-{version}-winx64.zip")
                } else {
                    format!("mysql-{version}-macos15-{arch}.tar.gz")
                };
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
                    _ if windows => "x86_64-pc-windows-msvc",
                    (true, true) => "aarch64-apple-darwin",
                    (true, false) => "x86_64-apple-darwin",
                    (false, true) => "aarch64-unknown-linux-gnu",
                    (false, false) => "x86_64-unknown-linux-gnu",
                };
                let file = format!("postgresql-{version}-{target}.{archive}");
                let url = format!(
                    "https://github.com/theseus-rs/postgresql-binaries/releases/download/{version}/{file}"
                );
                let sum = published_sum(&format!("{url}.sha256"), &file)?;
                fetch_tgz(&url, d, true, Some(&sum))
            }
            "redis" if windows => install_garnet(d, version),
            "redis" => build_redis(d, version),
            "versitygw" => {
                let os = if windows {
                    "Windows"
                } else if mac {
                    "Darwin"
                } else {
                    "Linux"
                };
                let arch = if arm { "arm64" } else { "x86_64" };
                let file = format!("versitygw_v{version}_{os}_{arch}.{archive}");
                let base =
                    format!("https://github.com/versity/versitygw/releases/download/v{version}");
                let sum = published_sum(&format!("{base}/checksums.txt"), &file)?;
                fetch_tgz(&format!("{base}/{file}"), d, true, Some(&sum))
            }
            "silo" => {
                let os = if windows {
                    "windows"
                } else if mac {
                    "darwin"
                } else {
                    "linux"
                };
                let arch = if arm { "arm64" } else { "amd64" };
                let file = format!("silo_{version}.0.0_{os}_{arch}.tar.gz");
                let base = format!(
                    "https://github.com/pgsty/silo/releases/download/{}",
                    silo_tag(version)
                );
                let sums = format!("{base}/silo_{version}.0.0_checksums.txt");
                let sum = published_sum(&sums, &file)?;
                fetch_tgz(&format!("{base}/{file}"), d, false, Some(&sum))
            }
            _ => {
                let os = if windows {
                    "windows"
                } else if mac {
                    "darwin"
                } else {
                    "linux"
                };
                let arch = if arm { "arm64" } else { "amd64" };
                let file = format!("mailpit-{os}-{arch}.{archive}");
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

/// SILO's release tag for its version number: 20261018000000 is
/// RELEASE.2026-10-18T00-00-00Z.
fn silo_tag(v: &str) -> String {
    let p = |a: usize, b: usize| v.get(a..b).unwrap_or("");
    let (y, mo, d) = (p(0, 4), p(4, 6), p(6, 8));
    let (h, mi, s) = (p(8, 10), p(10, 12), p(12, 14));
    format!("RELEASE.{y}-{mo}-{d}T{h}-{mi}-{s}Z")
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

/// Windows' Redis: Garnet's release for x64 in `<prefix>/garnet`, and the .NET runtime it
/// needs (it ships without one) in `<prefix>/dotnet`, from Microsoft's release list with
/// its SHA-512. Garnet's own file is checked against GitHub's digest.
fn install_garnet(prefix: &Path, version: &str) -> Result<()> {
    let file = "win-x64-based-readytorun.zip";
    let sum = github_digest("microsoft/garnet", &format!("v{version}"), file)?;
    let tmp = prefix.join("garnet.unpack");
    fetch_tgz(
        &format!("https://github.com/microsoft/garnet/releases/download/v{version}/{file}"),
        &tmp,
        false,
        Some(&sum),
    )?;
    // The server sits in a folder named for its .NET version (net10.0).
    let server = walk_find(&tmp, "GarnetServer.exe")
        .with_context(|| format!("{file} has no GarnetServer.exe"))?;
    fs::rename(server.parent().context("no folder")?, prefix.join("garnet"))?;
    let _ = fs::remove_dir_all(&tmp);

    let meta: serde_json::Value = serde_json::from_str(&fetch_text(&format!(
        "https://builds.dotnet.microsoft.com/dotnet/release-metadata/{GARNET_DOTNET}/releases.json"
    ))?)?;
    let latest = meta["latest-runtime"]
        .as_str()
        .context("no .NET runtime release")?;
    let runtime = meta["releases"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|r| r["runtime"]["version"] == latest)
        .and_then(|r| r["runtime"]["files"].as_array())
        .into_iter()
        .flatten()
        .find(|f| f["name"] == "dotnet-runtime-win-x64.zip")
        .context("no .NET runtime for Windows x64")?;
    let (url, hash) = (
        runtime["url"].as_str().unwrap_or_default(),
        runtime["hash"].as_str().unwrap_or_default(),
    );
    if !url.starts_with("https://builds.dotnet.microsoft.com/") || hash.len() != 128 {
        bail!(".NET's release list names an unexpected download");
    }
    fetch_tgz(url, &prefix.join("dotnet"), false, Some(hash))
}

/// The first file named `name` under `dir`, depth first.
fn walk_find(dir: &Path, name: &str) -> Option<PathBuf> {
    for e in fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        if p.is_dir() {
            if let Some(f) = walk_find(&p, name) {
                return Some(f);
            }
        } else if p.file_name().is_some_and(|n| n == name) {
            return Some(p);
        }
    }
    None
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
    run(crate::platform::command("make")
        .args(["-C", "src", &format!("-j{jobs}"), "BUILD_TLS=no", "redis-server", "redis-cli"])
        .current_dir(&src))
    .context("Building Redis failed — are the Xcode command line tools installed? (xcode-select --install)")?;
    fs::create_dir_all(prefix.join("bin"))?;
    for bin in ["redis-server", "redis-cli"] {
        fs::copy(src.join("src").join(bin), prefix.join("bin").join(bin))?;
    }
    let version = crate::platform::command(prefix.join("bin/redis-server"))
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
        "versitygw" | "silo" => format!(
            "AWS_ACCESS_KEY_ID={S3_ACCESS_KEY}\nAWS_SECRET_ACCESS_KEY={S3_SECRET_KEY}\nAWS_DEFAULT_REGION=us-east-1\nAWS_BUCKET={S3_BUCKET}\nAWS_ENDPOINT=http://127.0.0.1:{p}\nAWS_USE_PATH_STYLE_ENDPOINT=true"
        ),
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
    let usual =
        default_web_port(kind_of(cfg, id)?).with_context(|| format!("{id} has no web port"))?;
    check_port(cfg, id, port.unwrap_or(usual))?;
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
        "mysql" => crate::platform::command(d.join("bin/mysqld"))
            .arg("--no-defaults")
            .arg("--initialize-insecure")
            .arg(format!("--basedir={}", d.display()))
            .arg(format!("--datadir={}", data.display()))
            .stdout(log.try_clone()?)
            .stderr(log.try_clone()?)
            .status()?,
        "postgres" => crate::platform::command(d.join("bin/initdb"))
            .arg("-D")
            .arg(&data)
            .args(["-U", "postgres", "--auth=trust", "-E", "UTF8", "--locale=C"])
            .stdout(log.try_clone()?)
            .stderr(log.try_clone()?)
            .status()?,
        // Every folder in the data folder is a bucket, so the first one is just made;
        // SILO takes it as one too when it formats the empty folder.
        "versitygw" | "silo" => {
            fs::create_dir_all(data.join(S3_BUCKET))?;
            return Ok(());
        }
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
    let mut c = crate::platform::command(binary(kind, &v));
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
        "redis" if cfg!(windows) => {
            // Garnet: the .NET runtime beside it; snapshots in the data folder, read back
            // at start, as Redis's dump.rdb is.
            c.env("DOTNET_ROOT", d.join("dotnet"))
                .env("DOTNET_ROOT_X64", d.join("dotnet"))
                .args(["--port", &port, "--bind", "127.0.0.1", "--checkpointdir"])
                .arg(&data)
                .arg("--recover");
        }
        "redis" => {
            c.args(["--port", &port, "--bind", "127.0.0.1", "--dir"])
                .arg(&data);
        }
        "versitygw" => {
            let web = format!("127.0.0.1:{}", web_port(cfg, id).unwrap_or(7071));
            c.env("ROOT_ACCESS_KEY", S3_ACCESS_KEY)
                .env("ROOT_SECRET_KEY", S3_SECRET_KEY)
                .args(["--port", &format!("127.0.0.1:{port}"), "--webui", &web])
                .args(["--cors-allow-origin", &format!("http://{web}"), "posix"])
                .arg(&data);
        }
        "silo" => {
            // Its config and certs folders go in the data folder instead of ~/.silo;
            // both are needed, either one left out is made there anyway.
            let own = data.join(".silo");
            let web = format!("127.0.0.1:{}", web_port(cfg, id).unwrap_or(9001));
            c.env("MINIO_ROOT_USER", S3_ACCESS_KEY)
                .env("MINIO_ROOT_PASSWORD", S3_SECRET_KEY)
                .arg("server")
                .arg(&data)
                .args(["--address", &format!("127.0.0.1:{port}")])
                .args(["--console-address", &web, "--config-dir"])
                .arg(&own)
                .arg("--certs-dir")
                .arg(own.join("certs"));
        }
        _ => {
            c.args([
                "--smtp",
                &format!("127.0.0.1:{port}"),
                "--listen",
                &format!("127.0.0.1:{}", web_port(cfg, id).unwrap_or(8025)),
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

/// Running in some Anvil (the app or `anvil start`), by the pid its launch wrote: for the
/// CLI, whose own `Services` started nothing.
pub fn runs_somewhere(id: &str) -> bool {
    pid_alive(&pid_file(id))
}

fn pid_alive(file: &Path) -> bool {
    fs::read_to_string(file)
        .ok()
        // `pid` or `pid started`.
        .and_then(|t| {
            let line = t.lines().nth(1)?;
            Some(crate::alive(line.split(' ').next()?.trim()))
        })
        .unwrap_or(false)
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
        if let Some(s) = self.take(id) {
            s.wait(GRACE);
        }
    }

    /// Forget a running service and hand back its process, to stop with
    /// `Stopping::wait` once the lock on `Services` is released.
    pub fn take(&mut self, id: &str) -> Option<Stopping> {
        let child = self.children.remove(id)?;
        Some(Stopping {
            child,
            pid_files: vec![pid_file(id), legacy_pid_file(id)],
        })
    }

    /// `take` every service, to stop together with `stop_all`.
    pub fn take_all(&mut self) -> Vec<Stopping> {
        let ids: Vec<String> = self.children.keys().cloned().collect();
        ids.iter().filter_map(|id| self.take(id)).collect()
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

/// How long a service gets to exit after SIGTERM: databases flush and shut down cleanly.
pub const GRACE: Duration = Duration::from_secs(10);

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
        format!(
            "{}\n{} {}",
            crate::owner_line(),
            child.id(),
            crate::started(child.id()).unwrap_or_default()
        )
        .trim_end(),
    )?;
    Ok(child)
}

impl Drop for Services {
    fn drop(&mut self) {
        crate::stop_all(self.take_all(), GRACE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_kind_and_redis_version_has_its_license() {
        assert_eq!(license("mysql", "8.4.11").id, "mysql:GPL-2.0-only");
        assert_eq!(license("postgres", "18.6.0").id, "postgres:PostgreSQL");
        assert_eq!(license("mailpit", "1.31.6").id, "mailpit:MIT");
        assert_eq!(license("redis", "8.10.2").id, "redis:RSALv2-SSPLv1-AGPLv3");
        assert_eq!(license("redis", "7.4.11").id, "redis:RSALv2-SSPLv1");
        assert_eq!(license("redis", "7.2.16").id, "redis:BSD-3-Clause");
        assert_eq!(license("versitygw", "1.9.0").id, "versitygw:Apache-2.0");
        assert_eq!(license("silo", "20261018000000").id, "silo:AGPL-3.0");
        // Every offered version has an https link to its text.
        for kind in SERVICES {
            for v in versions(kind) {
                assert!(license(kind, v).url.starts_with("https://"), "{kind} {v}");
            }
        }
    }

    #[test]
    fn adding_needs_the_license_accepted_once_per_license() {
        let mut cfg = Config::default();
        let err = require_license(&mut cfg, "redis", "8.10.2", false).unwrap_err();
        assert!(format!("{err:#}").contains("accept its license"), "{err:#}");
        require_license(&mut cfg, "redis", "8.10.2", true).unwrap();
        // Remembered, with the date it was accepted.
        require_license(&mut cfg, "redis", "8.10.2", false).unwrap();
        assert_eq!(
            license_accepted(&cfg, "redis", "8.10.2").map(str::len),
            Some(10)
        );
        // Another Redis version under another license asks again.
        assert!(require_license(&mut cfg, "redis", "7.2.16", false).is_err());
        // An old config without the field still loads.
        let old: Config = toml::from_str("tld = \"test\"").unwrap();
        assert!(old.accepted_licenses.is_empty());
    }

    #[test]
    fn dates_are_civil_utc() {
        assert_eq!(ymd(0), "1970-01-01");
        assert_eq!(ymd(951_782_400), "2000-02-29");
        assert_eq!(ymd(1_792_281_600), "2026-10-18");
    }

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
    fn s3_gateway_listens_locally_with_its_console_and_env() {
        let mut cfg = Config::default();
        let id = insert(&mut cfg, "versitygw", Some("files"), Some(17070), None).unwrap();
        assert_eq!(version(&cfg, &id), "1.9.0");
        let web = web_port(&cfg, &id).unwrap();
        assert_ne!(web, 17070);

        let c = command(&cfg, &id);
        assert_eq!(c.get_program(), binary("versitygw", "1.9.0"));
        let args: Vec<String> = c
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(args.windows(2).any(|w| w == ["--port", "127.0.0.1:17070"]));
        let ui = format!("127.0.0.1:{web}");
        assert!(args.windows(2).any(|w| w[0] == "--webui" && w[1] == ui));
        // The backend and its root folder come last: posix <data>.
        assert_eq!(args[args.len() - 2], "posix");
        assert!(args.last().unwrap().ends_with("data/files"));
        let envs: Vec<_> = c.get_envs().collect();
        assert!(envs.contains(&("ROOT_ACCESS_KEY".as_ref(), Some(S3_ACCESS_KEY.as_ref()))));
        assert!(envs.contains(&("ROOT_SECRET_KEY".as_ref(), Some(S3_SECRET_KEY.as_ref()))));

        let env = env_lines(&cfg, &id);
        for line in [
            "AWS_ENDPOINT=http://127.0.0.1:17070",
            "AWS_USE_PATH_STYLE_ENDPOINT=true",
            "AWS_BUCKET=anvil",
            "AWS_ACCESS_KEY_ID=anvil",
            "AWS_SECRET_ACCESS_KEY=anvil-secret",
        ] {
            assert!(env.lines().any(|l| l == line), "{line} in {env}");
        }
        // Its console port moves like Mailpit's inbox port; others have none.
        cfg.services.get_mut(&id).unwrap().web_port = Some(17099);
        assert_eq!(web_port(&cfg, &id), Some(17099));
        cfg.services
            .insert("redis".into(), ServiceConfig::default());
        assert!(set_web_port(&mut cfg, "redis", Some(17100)).is_err());
    }

    #[test]
    fn silo_serves_its_data_folder_locally_with_its_console_and_env() {
        assert_eq!(silo_tag("20261018000000"), "RELEASE.2026-10-18T00-00-00Z");
        assert_eq!(silo_tag("20260903131801"), "RELEASE.2026-09-03T13-18-01Z");

        let mut cfg = Config::default();
        let id = insert(&mut cfg, "silo", Some("objects"), Some(19000), None).unwrap();
        assert_eq!(version(&cfg, &id), "20261018000000");
        let web = web_port(&cfg, &id).unwrap();
        assert_ne!(web, 19000);

        let c = command(&cfg, &id);
        assert_eq!(c.get_program(), binary("silo", "20261018000000"));
        let args: Vec<String> = c
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args[0], "server");
        assert!(args[1].ends_with("data/objects"));
        assert!(
            args.windows(2)
                .any(|w| w == ["--address", "127.0.0.1:19000"])
        );
        let ui = format!("127.0.0.1:{web}");
        assert!(
            args.windows(2)
                .any(|w| w[0] == "--console-address" && w[1] == ui)
        );
        // Nothing goes to ~/.silo: config and certs live with the data.
        let own = format!("{}/.silo", args[1]);
        assert!(args.windows(2).any(|w| w == ["--config-dir", own.as_str()]));
        assert!(
            args.windows(2)
                .any(|w| w[0] == "--certs-dir" && w[1].starts_with(&own))
        );
        let envs: Vec<_> = c.get_envs().collect();
        assert!(envs.contains(&("MINIO_ROOT_USER".as_ref(), Some(S3_ACCESS_KEY.as_ref()))));
        assert!(envs.contains(&("MINIO_ROOT_PASSWORD".as_ref(), Some(S3_SECRET_KEY.as_ref()))));

        let env = env_lines(&cfg, &id);
        assert!(
            env.lines()
                .any(|l| l == "AWS_ENDPOINT=http://127.0.0.1:19000")
        );
        assert!(env.lines().any(|l| l == "AWS_USE_PATH_STYLE_ENDPOINT=true"));
        assert!(!env.contains("FILESYSTEM_DISK"));
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
        assert!(crate::validate::service_id(&auto).is_ok());
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
    #[test]
    fn a_service_runs_somewhere_while_its_launched_pid_lives() {
        let dir = std::env::temp_dir().join(format!("anvil-svc-pid-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("svc-x.pids");
        assert!(!pid_alive(&file));
        let mut child = crate::platform::command("sleep").arg("30").spawn().unwrap();
        fs::write(&file, format!("1\n{}", child.id())).unwrap();
        assert!(pid_alive(&file));
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(!pid_alive(&file));
        let _ = fs::remove_dir_all(dir);
    }
}
