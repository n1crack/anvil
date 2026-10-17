//! Anvil core: configuration, sites, runtimes and the server supervisor.
//! Shared by the `anvil` CLI and the macOS app.

pub mod apps;
pub mod compose;
pub mod create;
pub mod databases;
pub mod dns;
pub mod git;
pub mod logs;
pub mod packages;
pub mod processes;
pub mod resources;
pub mod runtimes;
pub mod services;
pub mod shims;
pub mod style;
pub mod uninstall;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::Once;

const CADDY_VERSION: &str = "2.11.7";
/// SHA-512 of each Caddy download, from caddy_<version>_checksums.txt, pinned
/// here so a changed file on the release page is refused. Update with the version.
const CADDY_SHA512: [(&str, &str); 4] = [
    (
        "mac_arm64",
        "68f592df8f0ec058fbdb9b137b64e9a0aa792d28c06e1e3b9ec12d0e7fb371683a54e73ef6d8e6d0dd70dc6b941a5b5d5b73faedd3f89de119ad2eb78bdc668e",
    ),
    (
        "mac_amd64",
        "147886811fa2320a8881c6116dc8cc3f0562e2f686509ccc60f55cf85dfd5ea78e9f5f770c75da13bf124a2536b72e887b975fefd58706de3dafb57483610e70",
    ),
    (
        "linux_arm64",
        "3db36ba90c7a6e8dda40ee3dd71fa08844c76b5fb08f61b31e5e78d2ed38e71c51dc7baed875e50d1ca1279196e84302967237386ae87c91ae9f2aaceada682e",
    ),
    (
        "linux_amd64",
        "a7a433a1b133efc3c8d10eb0b99d52a24b5ef5c322dc77f5282182b1c0402139ab83f3a99f0c52409df77d20123fb0b523edad8a66d8f5e49136197bf61ef0e7",
    ),
];
const PHP_INDEX: &str = "https://dl.static-php.dev/static-php-cli/common/";

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub tld: String,
    pub http_port: u16,
    pub https_port: u16,
    pub dns_port: u16,
    pub php: Option<String>,
    pub paths: Vec<PathBuf>,
    pub links: BTreeMap<String, PathBuf>,
    pub sites: BTreeMap<String, SiteConfig>,
    /// macOS app name used for "Open in editor", e.g. "PhpStorm".
    pub editor: Option<String>,
    /// Parked sites removed from the list (their folders are left alone).
    pub hidden: Vec<String>,
    /// User groups in the app's site list: group name -> site names.
    pub groups: BTreeMap<String, Vec<String>>,
    /// php.ini overrides for every site; a site's own `ini` wins over these.
    pub ini: BTreeMap<String, String>,
    /// macOS app used for "Open Terminal", e.g. "iTerm".
    pub terminal: Option<String>,
    /// "light" or "dark"; unset follows the system.
    pub theme: Option<String>,
    /// Keep the Dock icon while no window is open.
    pub dock_icon: bool,
    /// Database, cache and mail services, by id (see `services::SERVICES`).
    pub services: BTreeMap<String, services::ServiceConfig>,
    /// macOS git app, e.g. "Fork"; unset uses the first one installed.
    pub git_client: Option<String>,
    /// Applications without their own choice get HTTPS. Unset only in a config from
    /// before HTTP became the default; `load` then keeps its sites on HTTPS.
    /// The field's own `default` (None) wins over `Config::default()` (Some(false)).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub https_default: Option<bool>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct SiteConfig {
    pub php: Option<String>,
    /// php.ini overrides for this site only, e.g. memory_limit = "1G".
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ini: BTreeMap<String, String>,
    /// Overrides the detected kind (see `apps::KINDS`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Custom start command for apps that run their own server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Fixed private port for apps that run their own server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// Start the app together with Anvil.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub autostart: bool,
    /// Language version for apps with their own server, e.g. Node "24".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,
    /// HTTPS on or off; unset follows `Config::https_default`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub https: Option<bool>,
    /// Per-process settings of the app's other processes, by process id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub processes: BTreeMap<String, ProcessConfig>,
}

/// What the user changed about one process of an app (see `processes::set_process`).
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
pub struct ProcessConfig {
    /// Replaces the detected command.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Start with Anvil.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub autostart: bool,
    /// Not listed among the app's processes.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
    /// A package.json command script shown as a row.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pinned: bool,
    /// Kept on top: listed first after the app's own row.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub top: bool,
    /// The subdomain label of an extra dev server's address (`docs` in
    /// `docs.vuefinder.test`), replacing the automatic one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        // Caddy's own ports: macOS only lets root listen below 1024 on loopback, so
        // 80 and 443 reach these through pf (see `ports_daemon`).
        Config {
            tld: "test".into(),
            http_port: 8080,
            https_port: 8443,
            dns_port: 5354,
            php: None,
            paths: vec![],
            links: BTreeMap::new(),
            sites: BTreeMap::new(),
            editor: None,
            hidden: vec![],
            groups: BTreeMap::new(),
            ini: BTreeMap::new(),
            terminal: None,
            theme: None,
            dock_icon: false,
            services: BTreeMap::new(),
            git_client: None,
            https_default: Some(false),
        }
    }
}

#[derive(Serialize, Clone)]
pub struct Site {
    pub name: String,
    /// Where it opens: `<name>.<tld>`, or `<name>.localhost` for a Next.js app
    /// whose config doesn't accept the tld (see `apps::next_allows`).
    pub host: String,
    /// Served over HTTPS (with HTTP redirecting to it); otherwise HTTP only.
    pub secure: bool,
    /// Project folder.
    pub path: PathBuf,
    /// Web root: `public/` when present, otherwise the project folder.
    pub root: PathBuf,
    /// PHP version in effect (site override or the default).
    pub php: Option<String>,
    /// True when the site has its own PHP version instead of the default.
    pub php_pinned: bool,
    /// php.ini overrides; a site with overrides gets its own PHP-FPM pool.
    pub ini: BTreeMap<String, String>,
    pub linked: bool,
    /// The parked directory this site comes from (`None` for linked sites).
    pub parked_in: Option<PathBuf>,
    /// Project folder modification time, seconds since the Unix epoch.
    pub modified: u64,
    /// What the project is (see `apps::KINDS`), detected or set by the user.
    pub kind: String,
    pub kind_detected: String,
    /// For apps that run their own server: the command and private port.
    pub command: Option<String>,
    pub command_custom: bool,
    pub port: Option<u16>,
    pub autostart: bool,
    /// Language version the app runs with: chosen, from the project, or the newest.
    pub runtime_version: Option<String>,
    pub runtime_pinned: bool,
}

/// `run` (sockets, pid files, generated configs) and `log`. `run` is private:
/// the Postgres socket there accepts anyone who can reach it (`--auth=trust`).
pub fn state_dirs() -> Result<()> {
    state_dirs_in(&home())
}

pub(crate) fn state_dirs_in(home: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let run = home.join("run");
    fs::create_dir_all(&run)?;
    fs::set_permissions(&run, fs::Permissions::from_mode(0o700))?;
    fs::create_dir_all(home.join("log"))?;
    Ok(())
}

pub fn home() -> PathBuf {
    std::env::var_os("ANVIL_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").expect("HOME not set")).join(".anvil")
        })
}

/// The config, or the defaults when there is none yet. Any other read error is
/// reported: treating it as "no config" would let the next save wipe it.
pub fn load() -> Result<Config> {
    match fs::read_to_string(home().join("config.toml")) {
        Ok(s) => {
            let mut cfg: Config = toml::from_str(&s).context("config.toml is invalid")?;
            if cfg.https_default.is_none() {
                // Every site used to be HTTPS: the ones there now keep it, new ones get HTTP.
                for site in sites(&cfg) {
                    cfg.sites
                        .entry(site.name)
                        .or_default()
                        .https
                        .get_or_insert(true);
                }
                cfg.https_default = Some(false);
                save(&cfg)?;
            }
            Ok(cfg)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(e).context("couldn't read config.toml"),
    }
}

/// Write the config atomically: readers (the app, the CLI) never see a half-written file.
pub fn save(cfg: &Config) -> Result<()> {
    fs::create_dir_all(home())?;
    let tmp = home().join(format!("config.toml.{}.tmp", std::process::id()));
    fs::write(&tmp, toml::to_string_pretty(cfg)?)?;
    fs::rename(&tmp, home().join("config.toml"))?;
    Ok(())
}

pub fn site_name(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// The PHP version a site runs: PHP projects always get one (start fails if it's
/// missing), static folders only when it's installed, apps with their own server never.
fn site_php(kind: &str, version: Option<String>, installed: &[String]) -> Option<String> {
    if apps::runs_process(kind) || !apps::serves(kind) {
        return None;
    }
    version.filter(|v| kind != "static" || installed.contains(v))
}

pub fn sites(cfg: &Config) -> Vec<Site> {
    let mut found = BTreeMap::new();
    for dir in &cfg.paths {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if e.path().is_dir()
                && !name.starts_with('.')
                && !cfg.hidden.contains(&site_name(&name))
            {
                found.insert(site_name(&name), (e.path(), Some(dir.clone())));
            }
        }
    }
    for (name, path) in &cfg.links {
        found.insert(name.clone(), (path.clone(), None));
    }
    let installed = installed_php();
    let sites = found
        .into_iter()
        .map(|(name, (path, parked_in))| {
            let public = path.join("public");
            let root = if public.is_dir() {
                public
            } else {
                path.clone()
            };
            let own = cfg.sites.get(&name);
            let pinned = own.and_then(|s| s.php.clone());
            let ini = own.map(|s| s.ini.clone()).unwrap_or_default();
            let detected = apps::detect(&path);
            let kind = own
                .and_then(|s| s.kind.clone())
                .unwrap_or_else(|| detected.to_string());
            let custom = own.and_then(|s| s.command.clone());
            let command = custom
                .clone()
                .or_else(|| apps::default_command(&kind, &path));
            let php_ok = !apps::runs_process(&kind) && apps::serves(&kind);
            let runtime_version = apps::runtime(&kind).and_then(|rt| {
                own.and_then(|s| s.runtime.clone())
                    .or_else(|| apps::runtime_version(rt, &path))
            });
            let modified = fs::metadata(&path)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs());
            // "Pinned" only when the pin is in effect (a static site drops PHP it can't use).
            let pin_set = php_ok && pinned.is_some();
            let php = site_php(&kind, pinned.or_else(|| cfg.php.clone()), &installed);
            let host = if kind == "next" && !apps::next_allows(&path, &cfg.tld) {
                format!("{name}.localhost")
            } else {
                format!("{name}.{}", cfg.tld)
            };
            Site {
                host,
                secure: own
                    .and_then(|s| s.https)
                    .unwrap_or(cfg.https_default == Some(true)),
                modified,
                ini,
                php_pinned: pin_set && php.is_some(),
                php,
                kind_detected: detected.into(),
                kind,
                command_custom: custom.is_some(),
                command,
                port: own.and_then(|s| s.port),
                autostart: own.is_some_and(|s| s.autostart),
                runtime_pinned: own.is_some_and(|s| s.runtime.is_some()),
                runtime_version,
                name,
                path,
                root,
                linked: parked_in.is_none(),
                parked_in,
            }
        })
        .collect::<Vec<_>>();
    let mut sites = sites;
    apps::assign_ports(cfg, &mut sites);
    sites
}

/// Serve `path` as `<name>.<tld>`; returns the cleaned-up name.
pub fn link(cfg: &mut Config, name: &str, path: PathBuf) -> Result<String> {
    let name = site_name(name.trim().trim_end_matches(&format!(".{}", cfg.tld)));
    if name.is_empty() || name.starts_with('.') || name.ends_with('.') {
        bail!("\"{name}\" is not a valid site name");
    }
    cfg.hidden.retain(|h| h != &name);
    cfg.links.insert(name.clone(), path);
    save(cfg)?;
    Ok(name)
}

/// Remove a site from Anvil: linked names are unlinked, parked folders are hidden.
/// Never touches files on disk.
/// Remove a link, or hide a parked app. A hidden app keeps its settings, so
/// showing it again (`unhide`) brings everything back.
pub fn remove(cfg: &mut Config, name: &str) -> Result<()> {
    if cfg.links.remove(name).is_none() {
        if !cfg.hidden.iter().any(|h| h == name) {
            cfg.hidden.push(name.into());
        }
        return save(cfg);
    }
    cfg.sites.remove(name);
    for members in cfg.groups.values_mut() {
        members.retain(|m| m != name);
    }
    cfg.groups.retain(|g, m| !m.is_empty() || g == "Favorites");
    save(cfg)
}

pub fn unhide(cfg: &mut Config, name: &str) -> Result<()> {
    cfg.hidden.retain(|h| h != name);
    save(cfg)
}

/// Move a site into a group, or back to the ungrouped list with `None`.
pub fn set_group(cfg: &mut Config, site: &str, group: Option<&str>) -> Result<()> {
    for members in cfg.groups.values_mut() {
        members.retain(|m| m != site);
    }
    if let Some(g) = group.map(str::trim).filter(|g| !g.is_empty()) {
        cfg.groups.entry(g.into()).or_default().push(site.into());
    }
    cfg.groups.retain(|g, m| !m.is_empty() || g == "Favorites");
    save(cfg)
}

/// The project's own favicon, if it has a non-empty one.
pub fn icon(site: &Site) -> Option<PathBuf> {
    [
        "favicon.svg",
        "favicon.png",
        "apple-touch-icon.png",
        "favicon.ico",
    ]
    .iter()
    .map(|f| site.root.join(f))
    .find(|p| fs::metadata(p).is_ok_and(|m| m.is_file() && m.len() > 0))
}

pub(crate) fn platform() -> Result<(&'static str, &'static str)> {
    // (static-php-cli suffix, caddy suffix)
    Ok(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => ("macos-aarch64", "mac_arm64"),
        ("macos", "x86_64") => ("macos-x86_64", "mac_amd64"),
        ("linux", "aarch64") => ("linux-aarch64", "linux_arm64"),
        ("linux", "x86_64") => ("linux-x86_64", "linux_amd64"),
        (os, arch) => bail!("{os}/{arch} is not supported yet"),
    })
}

pub fn run(cmd: &mut Command) -> Result<()> {
    let status = cmd
        .status()
        .with_context(|| format!("failed to run {cmd:?}"))?;
    if !status.success() {
        bail!("{cmd:?} exited with {status}");
    }
    Ok(())
}

pub(crate) fn fetch_text(url: &str) -> Result<String> {
    let out = Command::new("curl").args(["-fsSL", url]).output()?;
    if !out.status.success() {
        bail!("couldn't fetch {url}");
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The digest for `file` in a checksums listing: the hex word on the line naming
/// it, or the only word of a file that holds just a digest.
pub(crate) fn sum_in(text: &str, file: &str) -> Option<String> {
    let hex =
        |w: &&str| matches!(w.len(), 32 | 64 | 128) && w.bytes().all(|b| b.is_ascii_hexdigit());
    let words: Vec<&str> = text.split_whitespace().collect();
    if let [w] = words[..] {
        return Some(w).filter(hex).map(String::from);
    }
    text.lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>())
        .find(|ws| {
            ws.iter()
                .any(|w| w.trim_start_matches(['*', '.', '/']) == file)
        })?
        .into_iter()
        .find(hex)
        .map(String::from)
}

/// Fetch a published checksums file and pick the digest for `file` from it.
pub(crate) fn published_sum(url: &str, file: &str) -> Result<String> {
    sum_in(&fetch_text(url)?, file).with_context(|| format!("{url} lists no checksum for {file}"))
}

/// Hash `file` and compare with `sum` (hex MD5, SHA-256 or SHA-512, told apart by length).
/// openssl rather than shasum: one tool for all three, and every Mac has it.
pub(crate) fn verify(file: &Path, sum: &str, what: &str) -> Result<()> {
    let algo = match sum.len() {
        32 => "md5",
        64 => "sha256",
        128 => "sha512",
        _ => bail!("The published checksum of {what} is malformed"),
    };
    let out = Command::new("openssl")
        .args(["dgst", &format!("-{algo}"), "-r"])
        .arg(file)
        .output()
        .context("failed to run openssl")?;
    if !out.status.success() {
        bail!("openssl couldn't hash the download of {what}");
    }
    let got = String::from_utf8_lossy(&out.stdout);
    if !got
        .split_whitespace()
        .next()
        .is_some_and(|g| g.eq_ignore_ascii_case(sum))
    {
        bail!("The download of {what} didn't match its published checksum");
    }
    println!("{what}: {algo} checksum verified");
    Ok(())
}

/// Download `url` into ~/.anvil/build and check it against `sum` when the
/// publisher provides one. Returns the downloaded file.
pub(crate) fn download(url: &str, sum: Option<&str>) -> Result<PathBuf> {
    let name = url.rsplit('/').next().unwrap_or("download");
    let build = home().join("build");
    fs::create_dir_all(&build)?;
    // Keep the published name: rustup-init decides what it is from it.
    let tmp = build.join(name);
    let ok = run(Command::new("curl").args(["-fL#", "-o"]).arg(&tmp).arg(url))
        .and_then(|_| sum.map_or(Ok(()), |s| verify(&tmp, s, name)));
    if let Err(e) = ok {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(tmp)
}

/// Download a .tar.gz, verify it against `sum` if given, and unpack it into `dir`.
/// With `strip`, the archive's single top-level folder is dropped.
pub(crate) fn fetch_tgz(url: &str, dir: &Path, strip: bool, sum: Option<&str>) -> Result<()> {
    let tmp = download(url, sum)?;
    let mut tar = Command::new("tar");
    tar.arg("-xzf").arg(&tmp).arg("-C").arg(dir);
    if strip {
        tar.arg("--strip-components=1");
    }
    let ok = fs::create_dir_all(dir)
        .map_err(Into::into)
        .and_then(|_| run(&mut tar));
    let _ = fs::remove_file(tmp);
    ok
}

/// Install into a hidden sibling of `dir` and swap it in only once `fill`
/// succeeds, so a failed reinstall leaves the working one untouched.
pub(crate) fn replace_dir(dir: &Path, fill: impl FnOnce(&Path) -> Result<()>) -> Result<()> {
    let parent = dir.parent().context("no parent folder")?;
    let name = dir.file_name().context("no folder name")?.to_string_lossy();
    let (new, old) = (
        parent.join(format!(".{name}.new")),
        parent.join(format!(".{name}.old")),
    );
    let _ = fs::remove_dir_all(&new);
    let _ = fs::remove_dir_all(&old);
    let ok = fs::create_dir_all(&new)
        .map_err(Into::into)
        .and_then(|_| fill(&new));
    if let Err(e) = ok {
        let _ = fs::remove_dir_all(&new);
        return Err(e);
    }
    if dir.exists()
        && let Err(e) = fs::rename(dir, &old)
    {
        let _ = fs::remove_dir_all(&new);
        return Err(e.into());
    }
    if let Err(e) = fs::rename(&new, dir) {
        let _ = fs::rename(&old, dir);
        let _ = fs::remove_dir_all(&new);
        return Err(e.into());
    }
    let _ = fs::remove_dir_all(&old);
    Ok(())
}

fn php_dir(v: &str) -> PathBuf {
    home().join("php").join(v)
}

/// PHP versions Anvil offers to install, newest first.
pub const PHP_VERSIONS: [&str; 6] = ["8.5", "8.4", "8.3", "8.2", "8.1", "8.0"];

/// Remove an installed PHP version. Refuses the default and versions sites are pinned to.
pub fn php_uninstall(cfg: &Config, v: &str) -> Result<()> {
    if cfg.php.as_deref() == Some(v) {
        bail!("PHP {v} is the default version — choose another default first");
    }
    if let Some(site) = cfg.sites.iter().find(|(_, s)| s.php.as_deref() == Some(v)) {
        bail!(
            "{} uses PHP {v} — switch it to another version first",
            site.0
        );
    }
    if !PHP_VERSIONS.contains(&v) {
        bail!("Unknown PHP version {v}");
    }
    fs::remove_dir_all(php_dir(v)).with_context(|| format!("PHP {v} is not installed"))
}

pub fn installed_php() -> Vec<String> {
    let mut v: Vec<_> = fs::read_dir(home().join("php"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            e.path().join("php-fpm").exists() && !e.file_name().to_string_lossy().starts_with('.')
        })
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

pub fn php_install(cfg: &mut Config, minor: &str) -> Result<()> {
    let (plat, _) = platform()?;
    let out = Command::new("curl")
        .args(["-fsSL", &format!("{PHP_INDEX}?format=json")])
        .output()?;
    let list: Vec<serde_json::Value> =
        serde_json::from_slice(&out.stdout).context("bad PHP index")?;
    let prefix = format!("php-{minor}.");
    let suffix = format!("-fpm-{plat}.tar.gz");
    let patch = list
        .iter()
        .filter_map(|f| f["name"].as_str())
        .filter_map(|n| {
            n.strip_prefix(&prefix)?
                .strip_suffix(&suffix)?
                .parse::<u32>()
                .ok()
        })
        .max()
        .with_context(|| format!("no PHP {minor} build for {plat}"))?;
    let full = format!("{minor}.{patch}");
    let dir = php_dir(minor);
    println!("Installing PHP {full}…");
    replace_dir(&dir, |new| {
        for kind in ["fpm", "cli"] {
            // static-php-cli publishes no checksums; HTTPS is the only check.
            fetch_tgz(
                &format!("{PHP_INDEX}php-{full}-{kind}-{plat}.tar.gz"),
                new,
                false,
                None,
            )?;
        }
        let ini = dir.join("php.ini");
        if ini.exists() {
            fs::copy(ini, new.join("php.ini"))?;
        } else {
            fs::write(
                new.join("php.ini"),
                "memory_limit = 512M\nupload_max_filesize = 100M\npost_max_size = 100M\nmax_execution_time = 120\n",
            )?;
        }
        Ok(())
    })?;
    // The download took a while; save on top of the config as it is now.
    *cfg = load()?;
    if cfg.php.is_none() {
        cfg.php = Some(minor.into());
        save(cfg)?;
    }
    println!("PHP {full} installed in {}", dir.display());
    Ok(())
}

/// Set the PHP version used by every site that isn't pinned.
pub fn set_default_php(cfg: &mut Config, version: &str) -> Result<()> {
    if !installed_php().iter().any(|v| v == version) {
        bail!("PHP {version} is not installed — run `anvil php install {version}`");
    }
    cfg.php = Some(version.into());
    save(cfg)
}

/// Pin a site to a PHP version, or back to the default with `None`.
pub fn set_site_php(cfg: &mut Config, site: &str, version: Option<String>) -> Result<()> {
    if let Some(v) = &version
        && !installed_php().contains(v)
    {
        bail!("PHP {v} is not installed — run `anvil php install {v}`");
    }
    cfg.sites.entry(site_name(site)).or_default().php = version;
    save(cfg)
}

fn caddy() -> PathBuf {
    home().join("bin").join("caddy")
}

pub fn ensure_caddy() -> Result<()> {
    if caddy().exists() {
        return Ok(());
    }
    let (_, plat) = platform()?;
    println!("Downloading Caddy {CADDY_VERSION}…");
    let base = format!("https://github.com/caddyserver/caddy/releases/download/v{CADDY_VERSION}/");
    let file = format!("caddy_{CADDY_VERSION}_{plat}.tar.gz");
    let sum = CADDY_SHA512
        .iter()
        .find(|(p, _)| *p == plat)
        .map(|(_, s)| *s)
        .with_context(|| format!("No pinned checksum for {file}"))?;
    fetch_tgz(
        &format!("{base}{file}"),
        &home().join("bin"),
        false,
        Some(sum),
    )
}

/// Caddy keeps its CA and certs inside ANVIL_HOME, not in the user's global dirs.
fn caddy_cmd() -> Command {
    let mut c = Command::new(caddy());
    c.env("XDG_DATA_HOME", home().join("data"))
        .env("XDG_CONFIG_HOME", home().join("data"));
    c
}

/// Pool name for a site with its own settings (FPM pool names can't contain dots).
fn pool(site: &Site) -> String {
    format!("site-{}", site.name.replace('.', "_"))
}

/// The socket a site's PHP requests go to: its own pool, or the version's shared pool.
fn fpm_socket(site: &Site) -> Option<PathBuf> {
    let v = site.php.as_ref()?;
    let name = if site.ini.is_empty() {
        format!("php-{v}")
    } else {
        pool(site)
    };
    Some(home().join("run").join(format!("{name}.sock")))
}

/// PHP-FPM config for one version: the shared pool plus one pool per customised site.
fn fpm_conf(v: &str, global: &BTreeMap<String, String>, sites: &[Site]) -> String {
    let pool_conf = |name: &str, sock: &Path, max: u8| {
        format!(
            "\n[{name}]\nlisten = {}\npm = ondemand\npm.max_children = {max}\ncatch_workers_output = yes\nclear_env = no\n",
            sock.display()
        )
    };
    let mut s = format!(
        "[global]\nerror_log = {}\ndaemonize = no\n",
        home()
            .join("log")
            .join(format!("php-{v}-fpm.log"))
            .display()
    );
    let values = |ini: &BTreeMap<String, String>| -> String {
        ini.iter()
            .map(|(k, val)| format!("php_value[{k}] = {val}\n"))
            .collect()
    };
    s += &pool_conf("www", &home().join("run").join(format!("php-{v}.sock")), 16);
    s += &values(global);
    for site in sites
        .iter()
        .filter(|s| s.php.as_deref() == Some(v) && !s.ini.is_empty())
    {
        s += &pool_conf(&pool(site), &fpm_socket(site).unwrap(), 8);
        let mut ini = global.clone();
        ini.extend(site.ini.clone());
        s += &values(&ini);
    }
    s
}

/// Reject anything that could break out of a `php_value[key] = value` line.
fn check_ini(key: &str, value: &str) -> Result<()> {
    let key_ok = !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.');
    if !key_ok {
        bail!("\"{key}\" is not a valid php.ini setting name");
    }
    if value.is_empty() || value.chars().any(|c| c.is_control() || c == ';') {
        bail!("Invalid value for {key}");
    }
    Ok(())
}

/// Set (or with `None`, clear) one php.ini override for every site.
pub fn set_global_ini(cfg: &mut Config, key: &str, value: Option<&str>) -> Result<()> {
    let key = key.trim();
    match value.map(str::trim) {
        Some(v) => {
            check_ini(key, v)?;
            cfg.ini.insert(key.into(), v.into());
        }
        None => {
            cfg.ini.remove(key);
        }
    }
    save(cfg)
}

/// Set (or with `None`, clear) one php.ini override for a site.
/// Turn HTTPS on or off for one site.
pub fn set_secure(cfg: &mut Config, site: &str, on: bool) -> Result<()> {
    cfg.sites.entry(site_name(site)).or_default().https = Some(on);
    save(cfg)
}

pub fn set_site_ini(cfg: &mut Config, site: &str, key: &str, value: Option<&str>) -> Result<()> {
    let key = key.trim();
    let entry = cfg.sites.entry(site_name(site)).or_default();
    match value.map(str::trim) {
        Some(v) => {
            check_ini(key, v)?;
            entry.ini.insert(key.into(), v.into());
        }
        None => {
            entry.ini.remove(key);
        }
    }
    save(cfg)
}

pub fn caddyfile(cfg: &Config, sites: &[Site]) -> String {
    let mut s = format!(
        "{{\n\tdefault_bind 127.0.0.1 [::1]\n\thttp_port {}\n\thttps_port {}\n\tadmin off\n\tlocal_certs\n\tskip_install_trust\n\tauto_https disable_redirects\n}}\n",
        cfg.http_port, cfg.https_port
    );
    // A secure site's HTTP addresses redirect; 302, so turning HTTPS off isn't
    // undone by a redirect the browser remembers.
    let https_port = match public_https_port(cfg) {
        443 => String::new(),
        p => format!(":{p}"),
    };
    let redirect = |names: &[String]| {
        let http: Vec<_> = names.iter().map(|n| format!("http://{n}")).collect();
        format!(
            "\n{} {{\n\tredir https://{{host}}{https_port}{{uri}}\n}}\n",
            http.join(", ")
        )
    };
    // A site without HTTPS only answers http://, so Caddy makes no certificate for it.
    let addresses = |site: &Site, names: &[String]| {
        let list: Vec<_> = names
            .iter()
            .map(|n| {
                if site.secure {
                    n.clone()
                } else {
                    format!("http://{n}")
                }
            })
            .collect();
        list.join(", ")
    };
    for site in sites {
        // Containers answer on their own ports; a plain folder has nothing to serve.
        if !apps::serves(&site.kind) {
            continue;
        }
        let mut names = vec![format!("{}.{}", site.name, cfg.tld)];
        if let (true, Some(port)) = (apps::runs_process(&site.kind), site.port) {
            // Dev servers that only trust localhost (Next.js) work at <name>.localhost too.
            if cfg.tld != "localhost" {
                names.push(format!("{}.localhost", site.name));
            }
            s += &format!(
                "\n{} {{\n\treverse_proxy 127.0.0.1:{port}\n\thandle_errors {{\n\t\theader Content-Type \"text/html; charset=utf-8\"\n\t\trespond `{}` 502\n\t}}\n}}\n",
                addresses(site, &names),
                not_running_page(&site.name),
            );
            if site.secure {
                s += &redirect(&names);
            }
            // The app's other dev servers (a docs site) answer at <row>.<host>.
            for row in processes::server_rows(cfg, site) {
                let names = [row.host];
                s += &format!(
                    "\n{} {{\n\treverse_proxy 127.0.0.1:{}\n\thandle_errors {{\n\t\theader Content-Type \"text/html; charset=utf-8\"\n\t\trespond `{}` 502\n\t}}\n}}\n",
                    addresses(site, &names),
                    row.port,
                    not_running_page(&names[0]),
                );
                if site.secure {
                    s += &redirect(&names);
                }
            }
            continue;
        }
        // A quote, backslash or line break in the folder would end the quoted root and
        // break (or rewrite) the whole Caddyfile; such a folder isn't served.
        if site
            .root
            .to_string_lossy()
            .contains(['"', '\\', '\n', '\r'])
        {
            continue;
        }
        s += &format!(
            "\n{} {{\n\troot * \"{}\"\n\tencode\n",
            addresses(site, &names),
            site.root.display()
        );
        if let Some(sock) = fpm_socket(site) {
            s += &format!("\tphp_fastcgi unix/{}\n", sock.display());
        }
        s += "\tfile_server {\n\t\thide .*\n\t}\n}\n";
        if site.secure {
            s += &redirect(&names);
        }
    }
    s
}

/// Shown instead of a bare 502 when an app that runs its own server is stopped.
fn not_running_page(name: &str) -> String {
    format!(
        "<!doctype html><title>{name} is not running</title><body style=\"font:15px -apple-system,system-ui;display:grid;place-items:center;height:90vh;color:#444\"><div><h2>{name} is not running</h2><p>Start it in Anvil, then reload this page.</p></div>"
    )
}

/// Set the kind, start command, port or autostart of an app. `None` resets to the default.
pub fn set_site_app(
    cfg: &mut Config,
    site: &str,
    kind: Option<Option<String>>,
    command: Option<Option<String>>,
    port: Option<Option<u16>>,
    autostart: Option<bool>,
) -> Result<()> {
    let entry = cfg.sites.entry(site_name(site)).or_default();
    if let Some(kind) = kind {
        if let Some(k) = &kind
            && !apps::KINDS.contains(&k.as_str())
        {
            bail!("Unknown kind {k}");
        }
        entry.kind = kind;
    }
    if let Some(command) = command {
        if let Some(c) = &command {
            apps::check_command(c)?;
        }
        entry.command = command.map(|c| c.trim().to_string());
    }
    if let Some(port) = port {
        if port.is_some_and(|p| p < 1024) {
            bail!("Use a port of 1024 or higher");
        }
        entry.port = port;
    }
    if let Some(a) = autostart {
        entry.autostart = a;
    }
    save(cfg)
}

/// Pin an app to an installed language version, or back to automatic with `None`.
pub fn set_site_runtime(cfg: &mut Config, site: &str, version: Option<String>) -> Result<()> {
    let name = site_name(site);
    if let Some(v) = &version {
        let s = sites(cfg).into_iter().find(|s| s.name == name);
        let rt = s
            .as_ref()
            .and_then(|s| apps::runtime(&s.kind))
            .context("This app doesn't run its own server")?;
        if !runtimes::installed(rt).contains(v) {
            bail!("{} {v} isn't installed", runtimes::name(rt));
        }
    }
    cfg.sites.entry(name).or_default().runtime = version;
    save(cfg)
}

/// True when the running server was started with a different set of sites.
pub fn needs_restart(cfg: &Config) -> bool {
    fs::read_to_string(home().join("run").join("Caddyfile"))
        .map_or(true, |old| old != caddyfile(cfg, &sites(cfg)))
}

pub fn trust() -> Result<()> {
    // Caddy creates this CA on its first start.
    let root = root_cert();
    if !root.exists() {
        bail!("No certificate authority yet — start Anvil once, then trust it");
    }
    // ponytail: macOS login keychain only; Linux (update-ca-certificates / NSS) comes with the Linux port.
    let keychain = PathBuf::from(std::env::var_os("HOME").unwrap())
        .join("Library/Keychains/login.keychain-db");
    run(Command::new("security")
        .args(["add-trusted-cert", "-r", "trustRoot", "-k"])
        .arg(keychain)
        .arg(root))
}

/// What still stands between a fresh Mac and working `https://<app>.<tld>` addresses.
#[derive(Serialize)]
pub struct Setup {
    /// `/etc/resolver/<tld>` sends the domain to Anvil's DNS server.
    pub resolver: bool,
    /// The local certificate authority is trusted (true until Caddy has made one).
    pub trusted: bool,
    pub php: bool,
    /// Ports 80 and 443 reach Caddy, so addresses need no `:8443`.
    pub ports: bool,
    /// The terminal shims are written and on PATH in the user's shell rc file.
    pub shims: bool,
}

/// `.localhost` names already reach this Mac (macOS and browsers resolve them
/// to 127.0.0.1), so they need no resolver file, no password and no DNS server.
pub fn needs_resolver(cfg: &Config) -> bool {
    cfg.tld != "localhost"
}

/// Use `tld` for every application's address, e.g. `test` or `localhost`.
pub fn set_tld(cfg: &mut Config, tld: &str) -> Result<()> {
    cfg.tld = clean_tld(tld)?;
    save(cfg)
}

/// `.Test ` becomes `test`; anything but letters, digits and inner dashes is refused.
fn clean_tld(tld: &str) -> Result<String> {
    let tld = tld.trim().trim_start_matches('.').to_ascii_lowercase();
    let ok = !tld.is_empty()
        && tld.len() <= 63
        && tld.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        && !tld.starts_with('-')
        && !tld.ends_with('-');
    if !ok {
        bail!(
            "{tld} isn't a valid domain suffix: use letters, digits and dashes, like test or localhost"
        );
    }
    Ok(tld)
}

/// The `/etc/resolver/<tld>` file Anvil needs.
pub fn resolver_file(cfg: &Config) -> (PathBuf, String) {
    (
        PathBuf::from("/etc/resolver").join(&cfg.tld),
        format!("nameserver 127.0.0.1\nport {}\n", cfg.dns_port),
    )
}

/// The LaunchDaemon that, at every boot, has pf forward ports 80 and 443 on loopback
/// to Caddy's. macOS lets only root listen below 1024 on 127.0.0.1, so Caddy stays
/// unprivileged. The rules live in their own anchor under `com.apple/*`, which
/// `/etc/pf.conf` already evaluates, so that file is left alone.
pub fn ports_daemon(cfg: &Config) -> (PathBuf, String) {
    let mut rules = String::new();
    for (af, lo) in [("inet", "127.0.0.1"), ("inet6", "::1")] {
        for (from, port) in [(80, cfg.http_port), (443, cfg.https_port)] {
            // `\n` for printf: the plist keeps the command on one line.
            rules += &format!(
                "rdr pass on lo0 {af} proto tcp from any to {lo} port {from} -> {lo} port {port}\\n"
            );
        }
    }
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>{PORTS_LABEL}</string>
	<key>ProgramArguments</key>
	<array>
		<string>/bin/sh</string>
		<string>-c</string>
		<string>printf '{rules}' | /sbin/pfctl -a {PF_ANCHOR} -f - &amp;&amp; /sbin/pfctl -E</string>
	</array>
	<key>RunAtLoad</key>
	<true/>
</dict>
</plist>
"#
    );
    (
        PathBuf::from(format!("/Library/LaunchDaemons/{PORTS_LABEL}.plist")),
        plist,
    )
}

/// Write the ports daemon where a root command can copy it from.
pub fn stage_ports_daemon(cfg: &Config) -> Result<PathBuf> {
    let tmp = home().join("run").join("ports.plist");
    fs::create_dir_all(home().join("run"))?;
    fs::write(&tmp, ports_daemon(cfg).1)?;
    Ok(tmp)
}

pub const PORTS_LABEL: &str = "dev.anvil.ports";
pub const PF_ANCHOR: &str = "com.apple/anvil";

/// Whether the standard ports are forwarded to Caddy's.
// ponytail: checks the daemon file, not the live pf rules (reading those needs root);
// rules flushed by other software stay unnoticed until the next boot.
pub fn ports_forwarded(cfg: &Config) -> bool {
    let (file, want) = ports_daemon(cfg);
    fs::read_to_string(file).is_ok_and(|s| s == want)
}

/// The HTTPS port in addresses: 443 once forwarded, otherwise Caddy's own.
pub fn public_https_port(cfg: &Config) -> u16 {
    if ports_forwarded(cfg) {
        443
    } else {
        cfg.https_port
    }
}

/// The port addresses use for plain HTTP: 80 once forwarded, otherwise Caddy's own.
pub fn public_http_port(cfg: &Config) -> u16 {
    if ports_forwarded(cfg) {
        80
    } else {
        cfg.http_port
    }
}

/// Where a site opens: `https://` or `http://`, with the port only when it isn't
/// the scheme's standard one.
pub fn site_url(cfg: &Config, site: &Site) -> String {
    let (scheme, port, standard) = if site.secure {
        ("https", public_https_port(cfg), 443)
    } else {
        ("http", public_http_port(cfg), 80)
    };
    if port == standard {
        format!("{scheme}://{}", site.host)
    } else {
        format!("{scheme}://{}:{port}", site.host)
    }
}

pub fn setup_status(cfg: &Config) -> Setup {
    let (file, want) = resolver_file(cfg);
    let root = root_cert();
    Setup {
        resolver: !needs_resolver(cfg)
            || fs::read_to_string(file)
                .is_ok_and(|s| s.split_whitespace().eq(want.split_whitespace())),
        trusted: !root.exists()
            || Command::new("security")
                .arg("verify-cert")
                .arg("-c")
                .arg(&root)
                .output()
                .is_ok_and(|o| o.status.success()),
        php: !installed_php().is_empty(),
        ports: ports_forwarded(cfg),
        shims: shims::ready(),
    }
}

pub const SYSTEM_KEYCHAIN: &str = "/Library/Keychains/System.keychain";

/// The certificate authority Caddy makes on its first start.
pub fn root_cert() -> PathBuf {
    home().join("data/caddy/pki/authorities/local/root.crt")
}

/// One root shell script for the system steps asked for: the resolver file, the
/// ports daemon (copied from `ports_plist`) and trusting `cert` system-wide.
pub fn setup_script(
    cfg: &Config,
    resolver: bool,
    ports_plist: Option<&Path>,
    cert: Option<&Path>,
) -> Result<String> {
    let q = |p: &Path| shims::sh_quote(&p.to_string_lossy());
    let mut steps = Vec::new();
    if resolver && needs_resolver(cfg) {
        if cfg.tld.is_empty()
            || !cfg
                .tld
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            bail!("{} isn't a valid domain suffix", cfg.tld);
        }
        let (file, _) = resolver_file(cfg);
        steps.push(format!(
            "mkdir -p /etc/resolver && printf 'nameserver 127.0.0.1\\nport {}\\n' > {}",
            cfg.dns_port,
            q(&file)
        ));
    }
    if let Some(tmp) = ports_plist {
        let (file, _) = ports_daemon(cfg);
        let f = q(&file);
        // bootout first: a changed port reloads the daemon.
        steps.push(format!(
            "{{ launchctl bootout system/{PORTS_LABEL} 2>/dev/null; true; }} && cp {} {f} && chown root:wheel {f} && chmod 644 {f} && launchctl bootstrap system {f}",
            q(tmp)
        ));
    }
    if let Some(cert) = cert {
        steps.push(format!(
            "security add-trusted-cert -d -r trustRoot -k {SYSTEM_KEYCHAIN} {}",
            q(cert)
        ));
    }
    Ok(steps.join(" && "))
}

static DNS: Once = Once::new();
/// The suffix the DNS server answers for; every start sets it.
static DNS_TLD: std::sync::RwLock<String> = std::sync::RwLock::new(String::new());

fn pid_file() -> PathBuf {
    home().join("run").join("pids")
}

pub(crate) fn alive(pid: &str) -> bool {
    Command::new("kill")
        .args(["-0", pid])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Clean up after an Anvil that died without stopping its children (crash, SIGKILL).
/// The pid file is `owner pid` followed by child pids; children are only killed when
/// the owner is gone and the process still runs one of our binaries.
/// With `group`, child pids are process-group leaders and the whole group is killed.
pub(crate) fn kill_orphans(pid_file: &Path, group: bool) {
    let Ok(file) = fs::read_to_string(pid_file) else {
        return;
    };
    let mut pids = file.lines();
    if pids.next().is_none_or(alive) {
        return;
    }
    let ours = home().display().to_string();
    let mut killed = false;
    for pid in pids {
        let cmd = Command::new("ps")
            .args(["-p", pid, "-o", "command="])
            .output();
        if cmd.is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains(&ours)) {
            let target = if group { format!("-{pid}") } else { pid.into() };
            let _ = Command::new("kill").args(["--", &target]).status();
            killed = true;
        }
    }
    let _ = fs::remove_file(pid_file);
    if killed {
        std::thread::sleep(std::time::Duration::from_millis(500)); // let ports close
    }
}

/// Ask a process to shut down cleanly (SIGTERM); force it after `grace`.
pub(crate) fn terminate(child: &mut Child, grace: std::time::Duration) {
    let _ = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status();
    let deadline = std::time::Instant::now() + grace;
    while std::time::Instant::now() < deadline {
        if let Ok(Some(_)) = child.try_wait() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Running PHP-FPM pools and the web server.
pub struct Server {
    children: Vec<Child>,
    /// What each child is, parallel to `children`.
    labels: Vec<String>,
}

impl Server {
    pub fn start(cfg: &Config) -> Result<Server> {
        kill_orphans(&pid_file(), false);
        if TcpStream::connect(("127.0.0.1", cfg.https_port)).is_ok() {
            bail!(
                "Port {} is already in use — is Anvil already running?",
                cfg.https_port
            );
        }
        ensure_caddy()?;
        let sites = sites(cfg);
        let versions: BTreeSet<_> = sites.iter().filter_map(|s| s.php.clone()).collect();
        for v in &versions {
            if !php_dir(v).join("php-fpm").exists() {
                bail!("PHP {v} is used but not installed — run `anvil php install {v}`");
            }
        }
        state_dirs()?;

        let mut server = Server {
            children: vec![],
            labels: vec![],
        };
        for v in &versions {
            for site in &sites {
                if let Some(sock) = fpm_socket(site) {
                    let _ = fs::remove_file(sock);
                }
            }
            let _ = fs::remove_file(home().join("run").join(format!("php-{v}.sock")));
            let conf = home().join("run").join(format!("php-{v}-fpm.conf"));
            fs::write(&conf, fpm_conf(v, &cfg.ini, &sites))?;
            server.labels.push(format!("PHP {v} FPM"));
            server.children.push(
                Command::new(php_dir(v).join("php-fpm"))
                    .arg("-y")
                    .arg(&conf)
                    .arg("-c")
                    .arg(php_dir(v).join("php.ini"))
                    .spawn()
                    .with_context(|| format!("failed to start PHP {v}"))?,
            );
        }

        let file = home().join("run").join("Caddyfile");
        fs::write(&file, caddyfile(cfg, &sites))?;
        let log = fs::File::create(home().join("log").join("caddy.log"))?;
        server.labels.push("Caddy".into());
        server.children.push(
            caddy_cmd()
                .args(["run", "--adapter", "caddyfile", "--config"])
                .arg(&file)
                .stderr(log)
                .spawn()?,
        );

        let pids: Vec<String> = std::iter::once(std::process::id())
            .chain(server.pids().into_iter().map(|(_, p)| p))
            .map(|p| p.to_string())
            .collect();
        fs::write(pid_file(), pids.join("\n"))?;

        // ponytail: the DNS port is bound once per process; changing dns_port needs a restart of the process.
        *DNS_TLD.write().unwrap_or_else(|e| e.into_inner()) = cfg.tld.clone();
        // .localhost needs no DNS; switching to another suffix later starts it then.
        if !needs_resolver(cfg) {
            return Ok(server);
        }
        let port = cfg.dns_port;
        DNS.call_once(|| {
            std::thread::spawn(move || {
                if let Err(e) = dns::serve(port, &DNS_TLD) {
                    eprintln!("DNS server stopped: {e}");
                }
            });
        });
        Ok(server)
    }

    /// Each child's label (`Caddy`, `PHP 8.4 FPM`) and pid; Caddy is last.
    pub fn pids(&self) -> Vec<(String, u32)> {
        self.labels
            .iter()
            .cloned()
            .zip(self.children.iter().map(Child::id))
            .collect()
    }

    /// Blocks until the web server exits.
    pub fn wait(&mut self) -> Result<()> {
        self.children.last_mut().unwrap().wait()?;
        Ok(())
    }

    pub fn stop(&mut self) {
        if self.children.is_empty() {
            return;
        }
        for c in &mut self.children {
            terminate(c, std::time::Duration::from_secs(3));
        }
        self.children.clear();
        let _ = fs::remove_file(pid_file());
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_suffixes() {
        assert_eq!(clean_tld(" .Test ").unwrap(), "test");
        assert_eq!(clean_tld("localhost").unwrap(), "localhost");
        for bad in ["", "a b", "-x", "x-", "x/y", "x.y", "../etc"] {
            assert!(clean_tld(bad).is_err(), "{bad}");
        }
        let mut cfg = Config::default();
        assert!(needs_resolver(&cfg));
        cfg.tld = "localhost".into();
        assert!(!needs_resolver(&cfg));
    }

    #[test]
    fn caddy_checksums_cover_every_platform() {
        for p in ["mac_arm64", "mac_amd64", "linux_arm64", "linux_amd64"] {
            let row = CADDY_SHA512.iter().find(|(cp, _)| *cp == p);
            assert!(row.is_some_and(|(_, s)| s.len() == 128), "Caddy {p}");
        }
    }

    pub(crate) fn site(name: &str, root: &str, php: Option<&str>) -> Site {
        Site {
            name: name.into(),
            host: format!("{name}.test"),
            secure: true,
            path: root.into(),
            root: root.into(),
            php: php.map(Into::into),
            php_pinned: false,
            ini: BTreeMap::new(),
            linked: false,
            parked_in: None,
            modified: 0,
            kind: "php".into(),
            kind_detected: "php".into(),
            command: None,
            command_custom: false,
            port: None,
            autostart: false,
            runtime_version: None,
            runtime_pinned: false,
        }
    }

    #[test]
    fn checksum_listings() {
        let node = "aaaa  node-v22.11.0-darwin-arm64.tar.xz\n\
            2e89afe6f4e3aa6c7e21c560d8a0453d84807e97850bbb819b998531a22bdfde  node-v22.11.0-darwin-arm64.tar.gz\n\
            1111111111111111111111111111111111111111111111111111111111111111  node-v22.11.0-darwin-x64.tar.gz\n";
        assert_eq!(
            sum_in(node, "node-v22.11.0-darwin-arm64.tar.gz").unwrap(),
            "2e89afe6f4e3aa6c7e21c560d8a0453d84807e97850bbb819b998531a22bdfde"
        );
        assert_eq!(sum_in(node, "node-v22.11.0-linux-x64.tar.gz"), None);
        let sha512 = "68f592df8f0ec058fbdb9b137b64e9a0aa792d28c06e1e3b9ec12d0e7fb371683a54e73ef6d8e6d0dd70dc6b941a5b5d5b73faedd3f89de119ad2eb78bdc668e";
        let caddy = format!(
            "{}  caddy_2.11.7_linux_amd64.tar.gz\n{sha512}  caddy_2.11.7_mac_arm64.tar.gz\n",
            "f".repeat(128)
        );
        assert_eq!(
            sum_in(&caddy, "caddy_2.11.7_mac_arm64.tar.gz").unwrap(),
            sha512
        );
        // rustup: "<hex> *./rustup-init"; composer: just the digest; redis-hashes: digest mid-line.
        let h = "ec1b9233e7f72990ecd8e62063fa7f6c3dfc2bec8e97f88bff165f9100ac696a";
        assert_eq!(
            sum_in(&format!("{h} *./rustup-init\n"), "rustup-init").unwrap(),
            h
        );
        assert_eq!(sum_in(&format!("{h}\n"), "composer.phar").unwrap(), h);
        let redis = format!(
            "hash redis-8.10.2.tar.gz sha256 {h} http://download.redis.io/releases/redis-8.10.2.tar.gz\n"
        );
        assert_eq!(sum_in(&redis, "redis-8.10.2.tar.gz").unwrap(), h);
        assert_eq!(sum_in("<html>not found</html>", "composer.phar"), None);
    }

    #[test]
    fn verify_compares_digests() {
        let f = std::env::temp_dir().join(format!("anvil-verify-{}", std::process::id()));
        fs::write(&f, "hello\n").unwrap();
        let sha256 = "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03";
        verify(&f, sha256, "hello").unwrap();
        verify(&f, &sha256.to_uppercase(), "hello").unwrap();
        verify(&f, "b1946ac92492d2347c6235b4d2611184", "hello").unwrap(); // md5
        let bad = sha256.replace('5', "6");
        let err = verify(&f, &bad, "hello").unwrap_err().to_string();
        assert_eq!(
            err,
            "The download of hello didn't match its published checksum"
        );
        assert!(verify(&f, "abc", "hello").is_err());
        fs::remove_file(f).unwrap();
    }

    #[test]
    fn failed_reinstall_keeps_the_old_one() {
        let root = std::env::temp_dir().join(format!("anvil-replace-{}", std::process::id()));
        let dir = root.join("22");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("node"), "old").unwrap();

        let err = replace_dir(&dir, |d| {
            fs::write(d.join("node"), "half")?;
            bail!("download failed")
        });
        assert!(err.is_err());
        assert_eq!(fs::read_to_string(dir.join("node")).unwrap(), "old");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);

        replace_dir(&dir, |d| Ok(fs::write(d.join("node"), "new")?)).unwrap();
        assert_eq!(fs::read_to_string(dir.join("node")).unwrap(), "new");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn remove_never_deletes_and_groups_move() {
        let mut cfg = Config::default();
        let dir = std::env::temp_dir().join(format!("anvil-test-{}", std::process::id()));
        fs::create_dir_all(dir.join("shop")).unwrap();
        // SAFETY: tests in this module don't read ANVIL_HOME concurrently with other env changes.
        unsafe { std::env::set_var("ANVIL_HOME", &dir) };
        cfg.paths.push(dir.clone());
        assert_eq!(
            link(&mut cfg, "Admin.Shop.test", dir.join("shop")).unwrap(),
            "admin.shop"
        );
        assert!(link(&mut cfg, " .test", dir.clone()).is_err());

        set_group(&mut cfg, "shop", Some("Work")).unwrap();
        set_group(&mut cfg, "shop", Some("Clients")).unwrap();
        assert!(!cfg.groups.contains_key("Work"));
        assert_eq!(cfg.groups["Clients"], ["shop"]);

        remove(&mut cfg, "admin.shop").unwrap(); // linked: unlinked
        remove(&mut cfg, "shop").unwrap(); // parked: hidden
        assert!(
            sites(&cfg)
                .iter()
                .all(|s| s.name != "shop" && s.name != "admin.shop")
        );
        assert!(dir.join("shop").is_dir());
        // Hiding keeps the app's group, so showing it again restores it as it was.
        assert_eq!(cfg.groups["Clients"], ["shop"]);
        unhide(&mut cfg, "shop").unwrap();
        assert!(sites(&cfg).iter().any(|s| s.name == "shop"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn customised_sites_get_their_own_pool() {
        let mut shop = site("shop.be", "/s/shop/public", Some("8.4"));
        shop.ini.insert("memory_limit".into(), "1G".into());
        let sites = [shop, site("blog", "/s/blog", Some("8.4"))];

        let global = BTreeMap::from([
            ("memory_limit".to_string(), "256M".to_string()),
            ("max_execution_time".to_string(), "60".to_string()),
        ]);
        let conf = fpm_conf("8.4", &global, &sites);
        // www gets the global values; the site pool merges them with its own.
        let site_pool = &conf[conf.find("[site-shop_be]").unwrap()..];
        assert!(site_pool.contains("php_value[max_execution_time] = 60"));
        assert!(!site_pool.contains("256M"));
        assert!(
            conf[..conf.find("[site-shop_be]").unwrap()].contains("php_value[memory_limit] = 256M")
        );
        assert!(conf.contains("[www]"));
        assert!(conf.contains("[site-shop_be]\nlisten = "));
        assert!(conf.contains("php_value[memory_limit] = 1G"));
        assert!(!conf.contains("site-blog"));

        let caddy = caddyfile(&Config::default(), &sites);
        assert!(caddy.contains("site-shop_be.sock"));
        assert!(caddy.contains("php-8.4.sock"));

        assert!(check_ini("upload_max_filesize", "100M").is_ok());
        assert!(check_ini("memory_limit", "1G\n[evil]").is_err());
        assert!(check_ini("memory_limit", "1G ; x").is_err());
        assert!(check_ini("a]b", "1").is_err());
    }

    #[test]
    fn an_old_config_is_told_apart_from_a_new_one() {
        let old: Config = toml::from_str("tld = \"test\"\n").unwrap();
        assert_eq!(old.https_default, None);
        let new: Config = toml::from_str(&toml::to_string(&Config::default()).unwrap()).unwrap();
        assert_eq!(new.https_default, Some(false));
    }

    #[test]
    fn one_root_script_for_every_setup_step() {
        let cfg = Config::default();
        let s = setup_script(
            &cfg,
            true,
            Some(Path::new("/tmp/it's/ports.plist")),
            Some(Path::new("/h/root.crt")),
        )
        .unwrap();
        assert_eq!(
            s,
            "mkdir -p /etc/resolver && printf 'nameserver 127.0.0.1\\nport 5354\\n' > '/etc/resolver/test' \
             && { launchctl bootout system/dev.anvil.ports 2>/dev/null; true; } && cp '/tmp/it'\\''s/ports.plist' \
             '/Library/LaunchDaemons/dev.anvil.ports.plist' && chown root:wheel '/Library/LaunchDaemons/dev.anvil.ports.plist' \
             && chmod 644 '/Library/LaunchDaemons/dev.anvil.ports.plist' && launchctl bootstrap system \
             '/Library/LaunchDaemons/dev.anvil.ports.plist' \
             && security add-trusted-cert -d -r trustRoot -k /Library/Keychains/System.keychain '/h/root.crt'"
        );
        // Only what is asked for; .localhost needs no resolver file.
        let local = Config {
            tld: "localhost".into(),
            ..Config::default()
        };
        assert_eq!(setup_script(&local, true, None, None).unwrap(), "");
        let bad = Config {
            tld: "test;rm".into(),
            ..Config::default()
        };
        assert!(setup_script(&bad, true, None, None).is_err());
        assert!(setup_script(&bad, false, None, None).is_ok());
    }

    #[test]
    fn ports_daemon_forwards_loopback_only() {
        let (file, plist) = ports_daemon(&Config::default());
        assert_eq!(
            file,
            PathBuf::from("/Library/LaunchDaemons/dev.anvil.ports.plist")
        );
        for rule in [
            "rdr pass on lo0 inet proto tcp from any to 127.0.0.1 port 80 -> 127.0.0.1 port 8080\\n",
            "rdr pass on lo0 inet proto tcp from any to 127.0.0.1 port 443 -> 127.0.0.1 port 8443\\n",
            "rdr pass on lo0 inet6 proto tcp from any to ::1 port 443 -> ::1 port 8443\\n",
        ] {
            assert!(plist.contains(rule), "missing {rule} in {plist}");
        }
        let tmp = std::env::temp_dir().join(format!("anvil-ports-{}.plist", std::process::id()));
        fs::write(&tmp, &plist).unwrap();
        let lint = Command::new("plutil")
            .arg("-lint")
            .arg(&tmp)
            .output()
            .unwrap();
        fs::remove_file(&tmp).unwrap();
        assert!(
            lint.status.success(),
            "{}",
            String::from_utf8_lossy(&lint.stdout)
        );
    }

    #[test]
    fn caddyfile_routes_php_sites_to_their_socket() {
        let cfg = Config::default();
        let sites = [
            site("shop", "/s/shop/public", Some("8.3")),
            site("static", "/s/static", None),
        ];
        let out = caddyfile(&cfg, &sites);
        assert!(out.contains("https_port 8443"));
        assert!(out.contains("default_bind 127.0.0.1 [::1]"));
        assert_eq!(out.matches("hide .*").count(), 2);
        assert!(out.contains("\nshop.test {\n\troot * \"/s/shop/public\""));
        // HTTPS sites send their HTTP address on with a redirect the browser doesn't keep.
        assert!(out.contains("\nhttp://shop.test {\n\tredir https://{host}"));
        assert!(!out.contains("permanent"));
        assert!(out.contains("php-8.3.sock"));
        assert_eq!(out.matches("php_fastcgi").count(), 1);
        assert_eq!(site_name("My Shop.be"), "my-shop.be");

        // A folder whose path could close the quoted root is left out, not spliced in.
        let evil = [site("evil", "/s/a\"\n}\nimport /etc", None)];
        assert!(!caddyfile(&cfg, &evil).contains("evil.test"));

        // Apps with their own server also answer at <name>.localhost.
        let mut app = site("web", "/s/web", None);
        app.kind = "next".into();
        app.port = Some(41000);
        let out = caddyfile(&cfg, &[app.clone()]);
        assert!(out.contains("\nweb.test, web.localhost {\n\treverse_proxy 127.0.0.1:41000"));
        assert!(out.contains("\nhttp://web.test, http://web.localhost {\n\tredir "));
        // Without HTTPS only the http:// addresses exist: no certificate, no redirect.
        app.secure = false;
        let out = caddyfile(&cfg, &[app.clone()]);
        assert!(out.contains("\nhttp://web.test, http://web.localhost {\n\treverse_proxy"));
        assert!(!out.contains("\tredir "));
        let local = Config {
            tld: "localhost".into(),
            ..Config::default()
        };
        assert!(caddyfile(&local, &[app]).contains("\nhttp://web.localhost {\n\treverse_proxy"));

        // A vite app's extra dev server gets its own address, honouring HTTPS.
        let dir = std::env::temp_dir().join(format!("anvil-caddy-rows-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("package.json"),
            r#"{"scripts":{"dev":"vite","docs:dev":"vitepress dev docs"}}"#,
        )
        .unwrap();
        let mut vf = site("vuefinder", dir.to_str().unwrap(), None);
        vf.kind = "vite".into();
        vf.port = Some(41001);
        let port = processes::server_rows(&cfg, &vf)[0].port;
        let out = caddyfile(&cfg, &[vf.clone()]);
        assert!(out.contains(&format!(
            "\ndocs.vuefinder.test {{\n\treverse_proxy 127.0.0.1:{port}"
        )));
        assert!(out.contains("\nhttp://docs.vuefinder.test {\n\tredir https://{host}"));
        vf.secure = false;
        let out = caddyfile(&cfg, &[vf]);
        assert!(out.contains(&format!(
            "\nhttp://docs.vuefinder.test {{\n\treverse_proxy 127.0.0.1:{port}"
        )));
        assert_eq!(out.matches("\tredir ").count(), 0);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn static_sites_only_get_php_when_installed() {
        let v = || Some("8.4".to_string());
        let installed = ["8.4".to_string()];
        assert_eq!(site_php("static", v(), &[]), None);
        assert_eq!(site_php("static", v(), &installed), v());
        assert_eq!(site_php("laravel", v(), &[]), v());
        assert_eq!(site_php("php", v(), &[]), v());
        assert_eq!(site_php("next", v(), &installed), None);

        let mut st = site(
            "static",
            "/s/static",
            site_php("static", v(), &[]).as_deref(),
        );
        st.kind = "static".into();
        let out = caddyfile(&Config::default(), &[st]);
        assert!(!out.contains("php_fastcgi"));
        assert!(out.contains("file_server"));
    }
}
