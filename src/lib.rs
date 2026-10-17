//! Anvil core: configuration, sites, runtimes and the server supervisor.
//! Shared by the `anvil` CLI and the macOS app.

pub mod apps;
pub mod create;
pub mod databases;
pub mod dns;
pub mod git;
pub mod logs;
pub mod packages;
pub mod resources;
pub mod runtimes;
pub mod services;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::Once;

const CADDY_VERSION: &str = "2.11.7";
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
}

impl Default for Config {
    fn default() -> Self {
        // ponytail: macOS only lets root bind 80/443 on loopback; switch the defaults once a privileged helper exists.
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
        }
    }
}

#[derive(Serialize, Clone)]
pub struct Site {
    pub name: String,
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
        Ok(s) => Ok(toml::from_str(&s).context("config.toml is invalid")?),
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
    if apps::runs_process(kind) {
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
            let php_ok = !apps::runs_process(&kind);
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
            Site {
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
pub fn remove(cfg: &mut Config, name: &str) -> Result<()> {
    if cfg.links.remove(name).is_none() {
        cfg.hidden.push(name.into());
    }
    cfg.sites.remove(name);
    for members in cfg.groups.values_mut() {
        members.retain(|m| m != name);
    }
    cfg.groups.retain(|g, m| !m.is_empty() || g == "Favorites");
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

/// Download a .tar.gz and unpack it into `dir`.
/// With `strip`, the archive's single top-level folder is dropped.
pub(crate) fn fetch_tgz(url: &str, dir: &Path, strip: bool) -> Result<()> {
    fs::create_dir_all(dir)?;
    let tmp = dir.join(".download.tar.gz");
    run(Command::new("curl").args(["-fL#", "-o"]).arg(&tmp).arg(url))?;
    let mut tar = Command::new("tar");
    tar.arg("-xzf").arg(&tmp).arg("-C").arg(dir);
    if strip {
        tar.arg("--strip-components=1");
    }
    run(&mut tar)?;
    fs::remove_file(tmp)?;
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
        .filter(|e| e.path().join("php-fpm").exists())
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
    for kind in ["fpm", "cli"] {
        fetch_tgz(
            &format!("{PHP_INDEX}php-{full}-{kind}-{plat}.tar.gz"),
            &dir,
            false,
        )?;
    }
    let ini = dir.join("php.ini");
    if !ini.exists() {
        fs::write(
            &ini,
            "memory_limit = 512M\nupload_max_filesize = 100M\npost_max_size = 100M\nmax_execution_time = 120\n",
        )?;
    }
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
    fetch_tgz(
        &format!(
            "https://github.com/caddyserver/caddy/releases/download/v{CADDY_VERSION}/caddy_{CADDY_VERSION}_{plat}.tar.gz"
        ),
        &home().join("bin"),
        false,
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
    for site in sites {
        if let (true, Some(port)) = (apps::runs_process(&site.kind), site.port) {
            s += &format!(
                "\n{0}.{1}, http://{0}.{1} {{\n\treverse_proxy 127.0.0.1:{port}\n\thandle_errors {{\n\t\theader Content-Type \"text/html; charset=utf-8\"\n\t\trespond `{2}` 502\n\t}}\n}}\n",
                site.name,
                cfg.tld,
                not_running_page(&site.name),
            );
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
            "\n{0}.{1}, http://{0}.{1} {{\n\troot * \"{2}\"\n\tencode\n",
            site.name,
            cfg.tld,
            site.root.display()
        );
        if let Some(sock) = fpm_socket(site) {
            s += &format!("\tphp_fastcgi unix/{}\n", sock.display());
        }
        s += "\tfile_server {\n\t\thide .*\n\t}\n}\n";
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
    let root = home().join("data/caddy/pki/authorities/local/root.crt");
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

static DNS: Once = Once::new();

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
        for d in ["run", "log"] {
            fs::create_dir_all(home().join(d))?;
        }

        let mut server = Server { children: vec![] };
        for v in &versions {
            for site in &sites {
                if let Some(sock) = fpm_socket(site) {
                    let _ = fs::remove_file(sock);
                }
            }
            let _ = fs::remove_file(home().join("run").join(format!("php-{v}.sock")));
            let conf = home().join("run").join(format!("php-{v}-fpm.conf"));
            fs::write(&conf, fpm_conf(v, &cfg.ini, &sites))?;
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
        server.children.push(
            caddy_cmd()
                .args(["run", "--adapter", "caddyfile", "--config"])
                .arg(&file)
                .stderr(log)
                .spawn()?,
        );

        let pids: Vec<String> = std::iter::once(std::process::id())
            .chain(server.pids())
            .map(|p| p.to_string())
            .collect();
        fs::write(pid_file(), pids.join("\n"))?;

        // ponytail: DNS settings are read once per process; changing tld/dns_port needs a restart of the process.
        let (port, tld) = (cfg.dns_port, cfg.tld.clone());
        DNS.call_once(|| {
            std::thread::spawn(move || {
                if let Err(e) = dns::serve(port, tld) {
                    eprintln!("DNS server stopped: {e}");
                }
            });
        });
        Ok(server)
    }

    pub fn pids(&self) -> Vec<u32> {
        self.children.iter().map(Child::id).collect()
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

    fn site(name: &str, root: &str, php: Option<&str>) -> Site {
        Site {
            name: name.into(),
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
        assert!(!cfg.groups.contains_key("Clients"));
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
        assert!(out.contains("shop.test, http://shop.test {\n\troot * \"/s/shop/public\""));
        assert!(out.contains("php-8.3.sock"));
        assert_eq!(out.matches("php_fastcgi").count(), 1);
        assert_eq!(site_name("My Shop.be"), "my-shop.be");

        // A folder whose path could close the quoted root is left out, not spliced in.
        let evil = [site("evil", "/s/a\"\n}\nimport /etc", None)];
        assert!(!caddyfile(&cfg, &evil).contains("evil.test"));
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
