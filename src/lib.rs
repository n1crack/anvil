//! Anvil core: configuration, sites, runtimes and the server supervisor.
//! Shared by the `anvil` CLI and the desktop app.

pub mod apps;
pub mod browser;
pub mod check;
pub mod compose;
pub mod control;
pub mod create;
pub mod databases;
pub mod dbviewer;
pub mod dns;
pub mod git;
pub mod logs;
pub mod mail;
pub mod mcp;
pub mod octane;
pub mod opcache;
pub mod packages;
pub mod phpext;
pub mod platform;
pub mod processes;
pub mod project;
pub mod requests;
pub mod resources;
pub mod runtimes;
pub mod services;
pub mod shims;
pub mod style;
pub mod tunnel;
pub mod uninstall;
pub mod validate;
pub mod worktrees;

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
const CADDY_SHA512: [(&str, &str); 5] = [
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
    (
        "windows_amd64",
        "c308154504e53755958ffc62d8a2657ab17eefd6dd25c45dbb2c3d93d29519971f77ca652f7d0f2bcb7f980b9ba3822478865e29969d0272d7613550a43152b1",
    ),
];
/// Anvil's own builds of php.net's releases (github.com/n1crack/anvil-php): the newest
/// patch of each version, whether it is a preview, and each file's URL and SHA-256.
const PHP_INDEX: &str = "https://raw.githubusercontent.com/n1crack/anvil-php/main/index.json";

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
    /// Weekly, trim apps' big `storage/logs/*.log` and delete old daily logs (`logs::trim_app_logs`).
    pub trim_app_logs: bool,
    /// macOS notifications for crashes and finished New applications while Anvil isn't in front.
    pub notifications: bool,
    /// Service licenses the user accepted before installing: license id -> date.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub accepted_licenses: BTreeMap<String, String>,
    /// Settings › Worktrees › Copy the database: the last layer of the hooks'
    /// `copy_database` (`worktrees::resolve`), so New Worktree's checkbox and what a
    /// worktree opened outside Anvil uses, unless a preset or the app says otherwise.
    /// Stored only when turned on, under a key of its own: the `worktree_db = "copy"` older
    /// versions wrote to every config wasn't a choice, so it is read as unset (and drops out
    /// at the next save).
    #[serde(
        rename = "worktree_database",
        skip_serializing_if = "worktrees::DbChoice::is_empty"
    )]
    pub worktree_db: worktrees::DbChoice,
    /// Settings › Worktrees: the user's changes to Anvil's presets, by preset id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub worktree_presets: BTreeMap<String, worktrees::Hooks>,
    /// Per worktree site (`<app>--<name>`): its database choice and copies made.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub worktrees: BTreeMap<String, worktrees::WorktreeConfig>,
    /// Settings › Features › Agents (MCP): `anvil mcp` serves AI agents. Stored only when on.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub mcp: bool,
    /// Settings › Features › Tunnel: an app's Overview offers Share (`tunnel`). Stored only when on.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub tunnel: bool,
    /// Settings › Features › Mail catcher (`mail`): an SMTP listener with an inbox per app.
    /// Stored only when on.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub mail_catcher: bool,
    /// Settings › Features › Database viewer (`dbviewer`): Adminer at db.<tld>. Stored only when on.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub db_viewer: bool,
    /// Settings › Features › Request log: Caddy writes each app's requests (`requests`)
    /// for its Requests tab. On unless turned off; stored only when off.
    #[serde(skip_serializing_if = "is_true")]
    pub request_log: bool,
    /// The catcher's SMTP port; unset is `mail::DEFAULT_PORT`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mail_port: Option<u16>,
    /// Settings › PHP › Extensions: catalog extensions (`phpext::CATALOG`) loaded for
    /// every app on a version, by minor version. Stored only when there are some.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub php_ext: BTreeMap<String, Vec<String>>,
    /// Settings › PHP › Keep PHP up to date: a newer patch of an installed version
    /// installs on its own (`php_updates`). On unless turned off; stored only when off.
    #[serde(skip_serializing_if = "is_true")]
    pub php_auto_update: bool,
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct SiteConfig {
    pub php: Option<String>,
    /// php.ini overrides for this site only, e.g. memory_limit = "1G".
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ini: BTreeMap<String, String>,
    /// Catalog extensions for this site only (`phpext::CATALOG`), over its version's.
    /// A worktree's unset is its app's; its own list, even empty, replaces that.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ext: Option<Vec<String>>,
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
    /// The web root, relative to the project folder (`web`, `dist`, `.`), so a worktree
    /// serves the same folder of its own checkout; unset is `public/` when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
    /// Per-process settings of the app's other processes, by process id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub processes: BTreeMap<String, ProcessConfig>,
    /// The app's own worktree hooks (its Settings tab), over `.anvil.toml`'s and the preset's.
    #[serde(default, skip_serializing_if = "worktrees::Hooks::is_empty")]
    pub worktree: worktrees::Hooks,
    /// The Mail tab's "Don't catch": the app keeps its own mail setup while the catcher is on.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub mail_uncaught: bool,
    /// The Settings tab's "Browser errors": the app's pages send their errors to Anvil
    /// (`browser`). Off unless turned on; a worktree's unset follows its app.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser: Option<bool>,
    /// The Packages tab's badges, by package name (or PHP, Node…); unset is the detected
    /// ones (`packages::Packages::defaults`), a worktree's unset its app's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub packages_pinned: Option<Vec<String>>,
    /// The Settings tab's "Shared dependencies" (`apps::shared_deps`); a worktree's unset is
    /// on, not its app's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared_deps: Option<bool>,
}

/// What the user changed about one process of an app (see `processes::set_process`).
/// Older configs may still hold `pinned` and `top`; unknown keys are ignored on read
/// and dropped on the next save.
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
    /// The subdomain label of an extra dev server's address (`docs` in
    /// `vuefinder--docs.test`), replacing the automatic one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// Left out when the app's processes start together (Dev start); started by hand only.
    /// Unset: the row's default (Octane's is on, every other row's off).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manual: Option<bool>,
    /// Added in Anvil, not detected: `command` is what it runs and the id is its name.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub added: bool,
}

impl Default for Config {
    fn default() -> Self {
        // Caddy's own ports: macOS only lets root listen below 1024 on loopback, so
        // 80 and 443 reach these through pf (see `ports_daemon`). Windows lets anyone
        // listen there; its DNS rules (NRPT) only send to port 53.
        let windows = cfg!(windows);
        Config {
            tld: "test".into(),
            http_port: if windows { 80 } else { 8080 },
            https_port: if windows { 443 } else { 8443 },
            dns_port: if windows { 53 } else { 5354 },
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
            trim_app_logs: true,
            notifications: true,
            request_log: true,
            accepted_licenses: BTreeMap::new(),
            // The owner's "default off": a new worktree gets an empty database of its own.
            worktree_db: worktrees::DbChoice::Empty,
            worktree_presets: BTreeMap::new(),
            worktrees: BTreeMap::new(),
            mcp: false,
            tunnel: false,
            mail_catcher: false,
            db_viewer: false,
            mail_port: None,
            php_ext: BTreeMap::new(),
            php_auto_update: true,
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
    /// Web root: `root_custom` in the project folder, else `root_detected`.
    pub root: PathBuf,
    /// The web root the user set (`SiteConfig::root`), relative to the project folder.
    pub root_custom: Option<String>,
    /// The web root Anvil would pick: `public` when present, else `.` (the project folder).
    pub root_detected: String,
    /// PHP version in effect (site override or the default).
    pub php: Option<String>,
    /// True when the site has its own PHP version instead of the default.
    pub php_pinned: bool,
    /// php.ini overrides; a site with overrides gets its own PHP-FPM pool.
    pub ini: BTreeMap<String, String>,
    /// Its own extensions (`SiteConfig::ext`), on top of its version's.
    pub ext: Vec<String>,
    /// Served by a PHP-FPM master of its own (`mark_own_masters`).
    pub own_master: bool,
    /// Its Octane row's port and RoadRunner's RPC one (`processes::assign_octane_ports`).
    #[serde(skip)]
    pub octane_ports: Option<(u16, u16)>,
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
    /// Its Settings tab's "Browser errors" switch (`browser`); a worktree's unset is its app's.
    pub browser: bool,
    /// Its Packages tab's badges (`SiteConfig::packages_pinned`); `None` is the defaults.
    pub packages_pinned: Option<Vec<String>>,
    /// A pnpm app's "Shared dependencies" in effect (`apps::shared_deps`); `None` for an
    /// app without pnpm-lock.yaml.
    pub shared_deps: Option<bool>,
    /// For a git worktree of another app: that app's name (its processes and runtimes are shared).
    pub worktree_of: Option<String>,
    /// A worktree's branch, `None` when detached.
    pub branch: Option<String>,
    /// A worktree's addresses derived from its app's aliases (`admin--x.<tld>`).
    pub aliases: Vec<String>,
}

/// `run` (sockets, pid files, generated configs) and `log`. `run` is private:
/// the Postgres socket there accepts anyone who can reach it (`--auth=trust`).
pub fn state_dirs() -> Result<()> {
    state_dirs_in(&home())
}

pub(crate) fn state_dirs_in(home: &Path) -> Result<()> {
    let run = home.join("run");
    fs::create_dir_all(&run)?;
    platform::set_mode(&run, 0o700)?;
    fs::create_dir_all(home.join("log"))?;
    Ok(())
}

/// Held by every test that sets ANVIL_HOME, which is process-wide.
#[cfg(test)]
pub(crate) static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn home() -> PathBuf {
    std::env::var_os("ANVIL_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| user_home().join(".anvil"))
}

/// The user's home folder: `$HOME`, else the account's entry when HOME isn't set, so
/// a path is never silently made relative to the current folder.
pub fn user_home() -> PathBuf {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
        .or_else(platform::account_home)
        .expect("no home folder: HOME is not set and the account has none")
}

/// The config, or the defaults when there is none yet. Any other read error is
/// reported: treating it as "no config" would let the next save wipe it.
pub fn load() -> Result<Config> {
    match fs::read_to_string(home().join("config.toml")) {
        Ok(s) => toml::from_str(&s).context("config.toml is invalid"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(first_config()),
        Err(e) => Err(e).context("couldn't read config.toml"),
    }
}

/// The config before there is a config.toml. Linux starts on .localhost: it can't
/// send .test to Anvil's DNS yet, and .localhost needs no setup.
fn first_config() -> Config {
    let mut cfg = Config::default();
    if cfg!(target_os = "linux") {
        cfg.tld = "localhost".into();
    }
    cfg
}

/// Load, change and save the config with no other change in between: one lock for
/// the threads of this process, and an advisory lock on `config.lock` for the other
/// process (the CLI next to the app). Returns the config as saved, and what `f` returned.
///
/// `f` must not call `update` (or anything that does, such as `php_install`): the
/// locks aren't reentrant. Keep it short; long work (downloads) goes before or after.
pub fn update<T>(f: impl FnOnce(&mut Config) -> Result<T>) -> Result<(Config, T)> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = crate::lock(&LOCK);
    fs::create_dir_all(home())?;
    let file = fs::File::create(home().join("config.lock")).context("couldn't open config.lock")?;
    file.lock().context("couldn't lock config.lock")?;
    let mut cfg = load()?;
    let out = f(&mut cfg)?;
    // ponytail: most core setters save themselves too, so this can write twice; harmless.
    save(&cfg)?;
    Ok((cfg, out))
}

/// Write the config atomically: readers (the app, the CLI) never see a half-written
/// file. Each write gets its own temp file, so saves from two threads can't clash.
/// A change based on what's there goes through `update`, which holds the lock.
pub fn save(cfg: &Config) -> Result<()> {
    let home = home();
    fs::create_dir_all(&home)?;
    let mut tmp = tempfile::Builder::new()
        .prefix("config.toml.")
        .suffix(".tmp")
        .tempfile_in(&home)?;
    use std::io::Write;
    tmp.write_all(toml::to_string_pretty(cfg)?.as_bytes())?;
    tmp.persist(home.join("config.toml"))
        .context("couldn't save config.toml")?;
    Ok(())
}

/// Six hex digits of a hash that stays the same from run to run and release to
/// release (std's `DefaultHasher` makes no such promise): djb2.
pub(crate) fn stable_hash(s: &str) -> u32 {
    s.bytes()
        .fold(5381u32, |h, b| h.wrapping_mul(33) ^ u32::from(b))
}

/// `stable_hash` as six hex digits, for a name that must differ from a clashing one.
pub(crate) fn short_hash(s: &str) -> String {
    format!("{:06x}", stable_hash(s) & 0xff_ffff)
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

/// A parked folder hidden from Anvil: only what the list needs.
#[derive(Serialize)]
pub struct HiddenSite {
    pub name: String,
    pub path: PathBuf,
    pub kind: String,
}

/// The parked folders whose names are in `cfg.hidden`, for the list's "Show Hidden Applications".
pub fn hidden_sites(cfg: &Config) -> Vec<HiddenSite> {
    parked(cfg)
        .0
        .into_iter()
        .filter(|(name, _)| cfg.hidden.contains(name))
        .map(|(name, (path, _))| {
            let kind = cfg
                .sites
                .get(&name)
                .and_then(|s| s.kind.clone())
                .unwrap_or_else(|| apps::detect(&path).to_string());
            HiddenSite { name, path, kind }
        })
        .collect()
}

/// The parked folders by site name, hidden ones too, and a line for each one whose name another folder
/// already has (`My App` and `my-app`, or `shop` in two parked folders). The folder
/// whose name is the site name as it is keeps it, else the one with the first path;
/// the others get `-<hash of their path>`, so a name doesn't depend on `read_dir` order.
fn parked(cfg: &Config) -> (BTreeMap<String, (PathBuf, PathBuf)>, Vec<String>) {
    let mut all: Vec<(String, bool, PathBuf, PathBuf)> = vec![];
    for dir in &cfg.paths {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        for e in entries.flatten() {
            let folder = e.file_name().to_string_lossy().into_owned();
            if e.path().is_dir() && !folder.starts_with('.') {
                let name = site_name(&folder);
                all.push((name.clone(), folder != name, e.path(), dir.clone()));
            }
        }
    }
    // Keepers first: the exact name, then by path.
    all.sort();
    let (mut found, mut clashes) = (BTreeMap::new(), vec![]);
    for (name, _, path, dir) in all {
        let name = if found.contains_key(&name) {
            let unique = format!("{name}-{}", short_hash(&path.to_string_lossy()));
            clashes.push(format!(
                "{} is served as {unique}: {name} is another folder's name. Rename the folder to choose.",
                path.display()
            ));
            unique
        } else {
            name
        };
        found.insert(name, (path, dir));
    }
    (found, clashes)
}

/// What `sites` does about parked folders that would get the same name (see `parked`).
pub fn site_name_clashes(cfg: &Config) -> Vec<String> {
    parked(cfg).1
}

/// Every app's folder (canonical), hidden ones too.
pub(crate) fn app_folders(cfg: &Config) -> BTreeSet<PathBuf> {
    let real = |p: &Path| crate::platform::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    parked(cfg)
        .0
        .into_values()
        .map(|(p, _)| real(&p))
        .chain(cfg.links.values().map(|p| real(p)))
        .collect()
}

pub fn sites(cfg: &Config) -> Vec<Site> {
    let all_parked = parked(cfg).0;
    // Each app folder's real path, hidden ones too, resolved once per scan: a realpath
    // per app was a third of a scan's time.
    let canon = |p: &Path| crate::platform::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let reals: BTreeMap<PathBuf, PathBuf> = all_parked
        .values()
        .map(|(p, _)| p)
        .chain(cfg.links.values())
        .map(|p| (p.clone(), canon(p)))
        .collect();
    let real = |p: &Path| reals.get(p).cloned().unwrap_or_else(|| canon(p));
    let mut found: BTreeMap<String, (PathBuf, Option<PathBuf>)> = all_parked
        .into_iter()
        .filter(|(name, _)| !cfg.hidden.contains(name))
        .map(|(name, (path, dir))| (name, (path, Some(dir))))
        .collect();
    for (name, path) in &cfg.links {
        found.insert(name.clone(), (path.clone(), None));
    }
    // The git worktrees of each app, read from the repository's files (git may be broken
    // or missing); an alias (a link to a parked app's folder) is the same app. A worktree
    // deleted by hand counts as removed.
    let mut seen = BTreeSet::new();
    let mut mains = vec![];
    let parked_first = found.iter().filter(|(_, (_, d))| d.is_some());
    for (name, (path, _)) in parked_first.chain(found.iter().filter(|(_, (_, d))| d.is_none())) {
        if seen.insert(real(path)) {
            let mut list = worktrees::list(path);
            list.retain(|w| !w.prunable && w.path.exists());
            if !list.is_empty() {
                mains.push((name.clone(), path.clone(), list));
            }
        }
    }
    let worktree_paths: BTreeSet<PathBuf> = mains
        .iter()
        .flat_map(|(_, _, l)| l.iter().map(|w| w.path.clone()))
        .collect();
    // Every app's folder, hidden ones too: a folder that is a worktree of one of them is
    // never an app of its own (that would run on main's database). A hidden app's
    // worktrees are hidden with it (`app_folders`).
    let app_paths: BTreeSet<PathBuf> = reals.values().cloned().collect();
    // A link pointing straight at a worktree folder is that worktree, at one more address.
    let mut linked_to: BTreeMap<PathBuf, Vec<String>> = BTreeMap::new();
    found.retain(|name, (path, dir)| {
        let p = real(path);
        if worktree_paths.contains(&p) {
            if dir.is_none() {
                linked_to.entry(p).or_default().push(name.clone());
            }
            return false;
        }
        !worktrees::main_of(&p).is_some_and(|m| app_paths.contains(&m))
    });
    let installed = installed_php();
    let mut sites: Vec<Site> = found
        .into_iter()
        .map(|(name, (path, parked_in))| {
            let own = cfg.sites.get(&name);
            site_at(cfg, &installed, name, path, parked_in, own)
        })
        .collect();
    let mut taken: BTreeSet<String> = sites.iter().map(|s| s.name.clone()).collect();
    let tld = format!(".{}", cfg.tld);
    for (main, main_path, list) in &mains {
        if !sites.iter().any(|s| &s.name == main) {
            continue;
        }
        let aliases: Vec<&String> = cfg
            .links
            .iter()
            .filter(|(n, p)| real(p) == real(main_path) && *n != main)
            .map(|(n, _)| n)
            .collect();
        // The extra dev servers of the app and its aliases keep their `<app>--<row>` names.
        for s in sites
            .iter()
            .filter(|s| &s.name == main || aliases.contains(&&s.name))
        {
            taken.extend(
                processes::server_rows(cfg, s)
                    .into_iter()
                    .map(|r| r.host.strip_suffix(&tld).unwrap_or(&r.host).to_string()),
            );
        }
        for w in list {
            let base = worktrees::worktree_name(main, &w.path);
            let (mut label, mut n) = (base.clone(), 2);
            while taken.contains(&worktrees::capped(main, &label)) {
                label = format!("{base}-{n}");
                n += 1;
            }
            let name = worktrees::capped(main, &label);
            taken.insert(name.clone());
            let own = worktree_settings(cfg, main, &name);
            let mut site = site_at(cfg, &installed, name, w.path.clone(), None, Some(&own));
            site.linked = false;
            site.worktree_of = Some(main.clone());
            site.branch = w.branch.clone();
            // A derived alias address another app, dev server or worktree has is left out.
            for a in &aliases {
                let host = worktrees::capped(a, &label);
                if taken.insert(host.clone()) {
                    site.aliases.push(format!("{host}{tld}"));
                }
            }
            for n in linked_to.get(&w.path).into_iter().flatten() {
                if taken.insert(n.clone()) {
                    site.aliases.push(format!("{n}{tld}"));
                }
            }
            sites.push(site);
        }
    }
    sites.sort_by(|a, b| a.name.cmp(&b.name));
    apps::assign_ports(cfg, &mut sites);
    processes::assign_octane_ports(cfg, &mut sites);
    mark_own_masters(&mut sites);
    sites
}

/// A site with extensions of its own gets a PHP-FPM master of its own, and so does every
/// site on the same folder: two masters' pools never run the same files (one OPcache per
/// master), and a zend extension such as Xdebug can be one app's only.
fn mark_own_masters(sites: &mut [Site]) {
    let real = |p: &Path| crate::platform::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let paths: BTreeSet<PathBuf> = sites
        .iter()
        .filter(|s| !s.ext.is_empty())
        .map(|s| real(&s.path))
        .collect();
    if paths.is_empty() {
        return;
    }
    for s in sites.iter_mut() {
        s.own_master = paths.contains(&real(&s.path));
    }
}

/// A worktree's settings, field by field its own else its app's: a pin of either beats the
/// worktree's `.anvil.toml`, as for apps. Its own port; never with Anvil. Process settings
/// stay the app's (`processes::settings_of`).
fn worktree_settings(cfg: &Config, main: &str, name: &str) -> SiteConfig {
    let (o, m) = (cfg.sites.get(name), cfg.sites.get(main));
    let pick = |f: fn(&SiteConfig) -> Option<String>| o.and_then(f).or_else(|| m.and_then(f));
    SiteConfig {
        php: pick(|s| s.php.clone()),
        runtime: pick(|s| s.runtime.clone()),
        kind: pick(|s| s.kind.clone()),
        command: pick(|s| s.command.clone()),
        https: o.and_then(|s| s.https).or_else(|| m.and_then(|s| s.https)),
        root: pick(|s| s.root.clone()),
        ini: o
            .filter(|s| !s.ini.is_empty())
            .or(m)
            .map(|s| s.ini.clone())
            .unwrap_or_default(),
        ext: o
            .and_then(|s| s.ext.clone())
            .or_else(|| m.and_then(|s| s.ext.clone())),
        port: o.and_then(|s| s.port),
        autostart: false,
        processes: BTreeMap::new(),
        // The app's, read through `worktree_of` (`worktrees::hooks`).
        worktree: worktrees::Hooks::default(),
        // A worktree has an inbox of its own, so its own switch.
        mail_uncaught: o.is_some_and(|s| s.mail_uncaught),
        browser: o
            .and_then(|s| s.browser)
            .or_else(|| m.and_then(|s| s.browser)),
        packages_pinned: o
            .and_then(|s| s.packages_pinned.clone())
            .or_else(|| m.and_then(|s| s.packages_pinned.clone())),
        // A worktree's default, on (`apps::shared_deps`).
        shared_deps: Some(o.and_then(|s| s.shared_deps).unwrap_or(true)),
    }
}

/// The site for the folder at `path` served as `name`, with the settings `own`.
fn site_at(
    cfg: &Config,
    installed: &[String],
    name: String,
    path: PathBuf,
    parked_in: Option<PathBuf>,
    own: Option<&SiteConfig>,
) -> Site {
    let root_detected = if path.join("public").is_dir() {
        "public"
    } else if path.join("web/wp-config.php").is_file() {
        "web" // WordPress as Bedrock lays it out
    } else {
        "."
    };
    let root_custom = own.and_then(|s| s.root.clone());
    let root = match root_custom.as_deref().unwrap_or(root_detected) {
        "." => path.clone(),
        rel => path.join(rel),
    };
    let pinned = own.and_then(|s| s.php.clone());
    let ini = own.map(|s| s.ini.clone()).unwrap_or_default();
    let ext = own.and_then(|s| s.ext.clone()).unwrap_or_default();
    let detected = apps::detect(&path);
    let kind = own
        .and_then(|s| s.kind.clone())
        .unwrap_or_else(|| detected.to_string());
    let custom = own.and_then(|s| s.command.clone());
    let command = custom
        .clone()
        .or_else(|| apps::default_command(&kind, &path));
    let php_ok = !apps::runs_process(&kind) && apps::serves(&kind);
    // dotnet picks its SDK from global.json: a .NET pin an older Anvil saved does nothing.
    let runtime_pin = own
        .and_then(|s| s.runtime.clone())
        .filter(|_| kind != "dotnet");
    let runtime_pinned = runtime_pin.is_some();
    let runtime_version = apps::runtime(&kind)
        .and_then(|rt| runtime_pin.or_else(|| apps::runtime_version(rt, &path)));
    let modified = fs::metadata(&path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    // "Pinned" only when the pin is in effect (a static site drops PHP it can't use).
    let pin_set = php_ok && pinned.is_some();
    let php = site_php(
        &kind,
        project::php_version(pinned, &path, installed, cfg.php.clone()),
        installed,
    );
    let host = if kind == "next" && !apps::next_allows(&path, &cfg.tld) {
        format!("{name}.localhost")
    } else {
        format!("{name}.{}", cfg.tld)
    };
    let shared_deps = path
        .join("pnpm-lock.yaml")
        .is_file()
        .then(|| apps::shared_deps(&kind, own.and_then(|s| s.shared_deps), false));
    Site {
        host,
        secure: own
            .and_then(|s| s.https)
            .unwrap_or(cfg.https_default == Some(true)),
        modified,
        ini,
        ext,
        own_master: false,
        octane_ports: None,
        php_pinned: pin_set && php.is_some(),
        php,
        kind_detected: detected.into(),
        kind,
        command_custom: custom.is_some(),
        command,
        port: own.and_then(|s| s.port),
        autostart: own.is_some_and(|s| s.autostart),
        runtime_pinned,
        browser: own.and_then(|s| s.browser).unwrap_or(false),
        packages_pinned: own.and_then(|s| s.packages_pinned.clone()),
        shared_deps,
        worktree_of: None,
        branch: None,
        aliases: vec![],
        runtime_version,
        name,
        path,
        root,
        root_custom,
        root_detected: root_detected.into(),
        linked: parked_in.is_none(),
        parked_in,
    }
}

/// Serve `path` as `<name>.<tld>`; returns the cleaned-up name.
pub fn link(cfg: &mut Config, name: &str, path: PathBuf) -> Result<String> {
    let name = site_name(name.trim().trim_end_matches(&format!(".{}", cfg.tld)));
    if name.is_empty() || name.starts_with('.') || name.ends_with('.') {
        bail!("\"{name}\" is not a valid app name");
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
    // (PHP build suffix, caddy suffix)
    Ok(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => ("macos-aarch64", "mac_arm64"),
        ("macos", "x86_64") => ("macos-x86_64", "mac_amd64"),
        ("linux", "aarch64") => ("linux-aarch64", "linux_arm64"),
        ("linux", "x86_64") => ("linux-x86_64", "linux_amd64"),
        ("windows", "x86_64") => ("windows-x86_64", "windows_amd64"),
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

/// Runs a shell `script` as root, after macOS asks for the password with `prompt`. The
/// script goes to a private (0600) file and the file's path and the prompt reach
/// AppleScript as arguments, so nothing in them is ever parsed as AppleScript.
#[cfg(target_os = "macos")]
pub fn run_as_admin(script: &str, prompt: &str) -> Result<()> {
    run_sh_file(script, prompt, ADMIN_SH)
}

/// Linux has no administrator steps yet: `setup_script` refuses them first.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn run_as_admin(script: &str, prompt: &str) -> Result<()> {
    let _ = (script, prompt);
    bail!("{LINUX_NOT_YET}")
}

/// What Linux can't set up yet: the suffix's DNS, ports 80 and 443, HTTPS trust.
pub const LINUX_NOT_YET: &str = "On Linux, Anvil can't set up .test addresses, ports 80 and 443 or HTTPS trust yet: `anvil tld localhost` needs none of them, and addresses keep Caddy's port";

/// Runs a PowerShell `script` as administrator, after Windows asks (UAC). `prompt` has
/// no place in that dialog; the script stops at its first error.
#[cfg(windows)]
pub fn run_as_admin(script: &str, prompt: &str) -> Result<()> {
    let _ = prompt;
    let mut file = tempfile::Builder::new()
        .prefix("anvil-admin-")
        .suffix(".ps1")
        .tempfile()?;
    std::io::Write::write_all(
        &mut file,
        format!("$ErrorActionPreference = 'Stop'\r\n{script}\r\n").as_bytes(),
    )?;
    // Closed, so PowerShell can open it (Windows locks an open file); removed on drop.
    let file = file.into_temp_path();
    // Already an administrator (an elevated terminal): no need to ask.
    if platform::elevated() {
        let out = platform::command("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(&file)
            .output()
            .context("couldn't run PowerShell")?;
        if !out.status.success() {
            bail!(
                "the administrator step failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        return Ok(());
    }
    let path = file.display().to_string().replace('\'', "''");
    let outer = format!(
        "$p = Start-Process powershell.exe -Verb RunAs -Wait -PassThru -WindowStyle Hidden \
         -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File','\"{path}\"'; exit $p.ExitCode"
    );
    let out = platform::command("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &outer])
        .output()
        .context("couldn't run PowerShell")?;
    if !out.status.success() {
        let why = String::from_utf8_lossy(&out.stderr);
        if why.contains("canceled") || why.contains("cancelled") {
            bail!("Windows didn't get permission (the request was declined)");
        }
        bail!(
            "the administrator step failed ({}): {}",
            out.status,
            why.trim()
        );
    }
    Ok(())
}

#[cfg(target_os = "macos")]
const ADMIN_SH: &str = "do shell script \"/bin/sh \" & quoted form of (item 1 of argv) with prompt (item 2 of argv) with administrator privileges";

#[cfg(target_os = "macos")]
fn run_sh_file(script: &str, prompt: &str, applescript: &str) -> Result<()> {
    // tempfile creates the file 0600; it is removed when `file` drops.
    let mut file = tempfile::Builder::new()
        .prefix("anvil-admin-")
        .suffix(".sh")
        .tempfile()?;
    std::io::Write::write_all(&mut file, script.as_bytes())?;
    run(crate::platform::command("osascript")
        .args(["-e", "on run argv", "-e", applescript, "-e", "end run"])
        .arg(file.path())
        .arg(prompt))
}

/// `curl <args> <url>`'s body; an error, with curl's reason, when it fails (`-f` makes an
/// HTTP error a failure).
pub(crate) fn curl(args: &[&str], url: &str) -> Result<Vec<u8>> {
    let out = crate::platform::command("curl")
        .args(args)
        .arg(url)
        .output()
        .context("couldn't run curl")?;
    if !out.status.success() {
        let why = String::from_utf8_lossy(&out.stderr);
        let why = why.trim().trim_start_matches("curl: ");
        bail!(
            "couldn't fetch {url}: {}",
            if why.is_empty() { "no answer" } else { why }
        );
    }
    Ok(out.stdout)
}

pub(crate) fn fetch_text(url: &str) -> Result<String> {
    Ok(String::from_utf8_lossy(&curl(&["-fsSL"], url)?).into_owned())
}

/// `fetch_text` of a JSON document.
pub(crate) fn fetch_json<T: serde::de::DeserializeOwned>(url: &str) -> Result<T> {
    serde_json::from_str(&fetch_text(url)?)
        .with_context(|| format!("{url} sent something unreadable"))
}

/// The digest for `file` in a checksums listing: the hex word on the line naming
/// it, or on the line after when that line has none (certutil's `SHA256 hash of <file>:`),
/// or the only word of a file that holds just a digest.
pub(crate) fn sum_in(text: &str, file: &str) -> Option<String> {
    let hex = |w: &&str| {
        matches!(w.len(), 32 | 40 | 64 | 128) && w.bytes().all(|b| b.is_ascii_hexdigit())
    };
    let words: Vec<&str> = text.split_whitespace().collect();
    if let [w] = words[..] {
        return Some(w).filter(hex).map(String::from);
    }
    let lines: Vec<Vec<&str>> = text
        .lines()
        .map(|l| l.split_whitespace().collect())
        .collect();
    let at = lines.iter().position(|ws| {
        ws.iter()
            .any(|w| w.trim_start_matches(['*', '.', '/']).trim_end_matches(':') == file)
    })?;
    lines[at]
        .iter()
        .find(|w| hex(w))
        .or_else(|| lines.get(at + 1)?.iter().find(|w| hex(w)))
        .map(|w| w.to_string())
}

/// Fetch a published checksums file and pick the digest for `file` from it.
pub(crate) fn published_sum(url: &str, file: &str) -> Result<String> {
    sum_in(&fetch_text(url)?, file).with_context(|| format!("{url} lists no checksum for {file}"))
}

/// Hash `file` and compare with `sum` (hex MD5, SHA-1, SHA-256 or SHA-512, told apart by
/// length). openssl rather than shasum: one tool for all of them, and every Mac has it.
pub(crate) fn verify(file: &Path, sum: &str, what: &str) -> Result<()> {
    let algo = match sum.len() {
        32 => "md5",
        // WordPress publishes only SHA-1 and MD5.
        40 => "sha1",
        64 => "sha256",
        128 => "sha512",
        _ => bail!("The published checksum of {what} is malformed"),
    };
    let got = if cfg!(windows) {
        // certutil: a title line, the digest (older Windows spaces its bytes), a status line.
        let out = crate::platform::command("certutil")
            .arg("-hashfile")
            .arg(file)
            .arg(algo.to_ascii_uppercase())
            .output()
            .context("failed to run certutil")?;
        if !out.status.success() {
            bail!("certutil couldn't hash the download of {what}");
        }
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .nth(1)
            .unwrap_or_default()
            .replace(' ', "")
    } else {
        let out = crate::platform::command("openssl")
            .args(["dgst", &format!("-{algo}"), "-r"])
            .arg(file)
            .output()
            .context("failed to run openssl")?;
        if !out.status.success() {
            bail!("openssl couldn't hash the download of {what}");
        }
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
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

/// The file `download` is writing while it does, so its size says how far it got.
/// ponytail: one at a time; two downloads at once show the later one.
static DOWNLOADING: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

/// Bytes the running download has written, if one runs.
pub fn downloaded() -> Option<u64> {
    let path = lock(&DOWNLOADING).clone()?;
    Some(fs::metadata(path).map_or(0, |m| m.len()))
}

/// Download `url` into ~/.anvil/build and check it against `sum` when the
/// publisher provides one. Returns the downloaded file.
pub(crate) fn download(url: &str, sum: Option<&str>) -> Result<PathBuf> {
    let name = url.rsplit('/').next().unwrap_or("download");
    let build = home().join("build");
    fs::create_dir_all(&build)?;
    // Keep the published name: rustup-init decides what it is from it.
    let tmp = build.join(name);
    *lock(&DOWNLOADING) = Some(tmp.clone());
    let fetched = run(crate::platform::command("curl")
        .args(["-fL#", "-o"])
        .arg(&tmp)
        .arg(url));
    *lock(&DOWNLOADING) = None;
    let ok = fetched.and_then(|_| sum.map_or(Ok(()), |s| verify(&tmp, s, name)));
    if let Err(e) = ok {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(tmp)
}

/// Download a .tar.gz, verify it against `sum` if given, and unpack it into `dir`.
/// With `strip`, the archive's single top-level folder is dropped. On Windows a .zip
/// too: its own tar (bsdtar) reads both. A .tar.xz (MySQL on Linux) is left to tar to
/// recognise.
pub(crate) fn fetch_tgz(url: &str, dir: &Path, strip: bool, sum: Option<&str>) -> Result<()> {
    let tmp = download(url, sum)?;
    let mut tar = if cfg!(windows) {
        // Not a tar earlier on PATH (Git's, which can't read zip).
        let system = std::env::var_os("SystemRoot").map_or("C:\\Windows".into(), PathBuf::from);
        crate::platform::command(system.join("System32").join("tar.exe"))
    } else {
        crate::platform::command("tar")
    };
    tar.arg(if cfg!(windows) || url.ends_with(".tar.xz") {
        "-xf"
    } else {
        "-xzf"
    })
    .arg(&tmp)
    .arg("-C")
    .arg(dir);
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

/// The program that serves a PHP version's requests: PHP-FPM, or on Windows (which has
/// none) a pool of php-cgi processes (`cgi_ports`).
pub const PHP_SERVER: &str = if cfg!(windows) {
    "php-cgi.exe"
} else {
    "php-fpm"
};

/// `php` of an installed version.
pub fn php_bin(v: &str) -> PathBuf {
    php_dir(v).join(platform::exe("php"))
}

pub fn php_dir(v: &str) -> PathBuf {
    home().join("php").join(v)
}

/// PHP versions Anvil offers to install, newest first.
/// One the source doesn't publish yet (8.6 before its first release candidate) is
/// listed only once it does (`php_releases`).
pub const PHP_VERSIONS: [&str; 6] = ["8.6", "8.5", "8.4", "8.3", "8.2", "8.1"];

/// Versions published only as previews (betas, RCs) so far: not offered until the
/// source is known to have one.
pub const PHP_PREVIEWS: [&str; 1] = ["8.6"];

/// Refuse a PHP version Anvil doesn't offer, saying why for 8.0: it is end of life, and
/// its static-php-cli macOS build can't load extensions.
pub fn php_offered(v: &str) -> Result<(), String> {
    if PHP_VERSIONS.contains(&v) {
        return Ok(());
    }
    let why = if v == "8.0" {
        "is end of life"
    } else {
        "isn't offered"
    };
    Err(format!(
        "PHP {v} {why} — Anvil offers PHP {}",
        PHP_VERSIONS.join(", ")
    ))
}

/// The apps pinned to `version` of `lang` (`php` or a runtime id), by name: removing
/// it would break them. Only pins in effect count (a .NET one never is).
pub fn pinned_to(cfg: &Config, lang: &str, version: &str) -> Vec<String> {
    let pins: Vec<&String> = cfg
        .sites
        .iter()
        .filter(|(_, s)| {
            let pin = if lang == "php" { &s.php } else { &s.runtime };
            pin.as_deref() == Some(version)
        })
        .map(|(n, _)| n)
        .collect();
    if lang == "php" || pins.is_empty() {
        return pins.into_iter().cloned().collect();
    }
    // A runtime pin is a bare version: it counts for apps that run on this language.
    let all = sites(cfg);
    pins.into_iter()
        .filter(|n| {
            all.iter()
                .any(|s| &&s.name == n && s.runtime_pinned && apps::runtime(&s.kind) == Some(lang))
        })
        .cloned()
        .collect()
}

/// Refuse to remove a version while apps are pinned to it, naming them.
fn refuse_pinned(cfg: &Config, lang: &str, label: &str, version: &str) -> Result<()> {
    match pinned_to(cfg, lang, version).as_slice() {
        [] => Ok(()),
        [one] => bail!("{one} uses {label} {version} — switch it to another version first"),
        many => bail!(
            "{} use {label} {version} — switch them to another version first",
            many.join(", ")
        ),
    }
}

/// Remove an installed language version, refused while apps are pinned to it.
pub fn runtime_uninstall(cfg: &Config, id: &str, version: &str) -> Result<()> {
    refuse_pinned(cfg, id, runtimes::name(id), version)?;
    runtimes::uninstall(id, version)
}

/// Remove an installed PHP version. Refuses the default and versions sites are pinned to.
pub fn php_uninstall(cfg: &Config, v: &str) -> Result<()> {
    if cfg.php.as_deref() == Some(v) {
        bail!("PHP {v} is the default version — choose another default first");
    }
    refuse_pinned(cfg, "php", "PHP", v)?;
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
            e.path().join(PHP_SERVER).exists()
                && PHP_VERSIONS.contains(&e.file_name().to_string_lossy().as_ref())
        })
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

/// The source's list of PHP builds: Anvil's index.json, or windows.php.net's
/// releases.json. Both list each version's newest patch under `[minor].version`.
fn php_index() -> Result<serde_json::Value> {
    let url = if cfg!(windows) {
        format!("{PHP_WINDOWS}releases.json")
    } else {
        PHP_INDEX.to_string()
    };
    // `anvil php list` asks too: a dead network mustn't hold the list up.
    serde_json::from_slice(&curl(&["-fsSL", "--max-time", "10"], &url)?)
        .with_context(|| format!("{url} sent something unreadable"))
}

/// What the source has of each PHP version: minor -> (newest patch, preview).
pub type PhpReleases = BTreeMap<String, (String, bool)>;

/// The source's PHP versions Anvil offers, each with its newest patch.
pub fn php_releases() -> Result<PhpReleases> {
    let plat = if cfg!(windows) { "" } else { platform()?.0 };
    Ok(releases_from(&php_index()?, plat))
}

/// The versions in `index` with a build for `plat` (Windows' releases.json has one
/// build per version, so `plat` is ""). Only Anvil's index marks previews.
fn releases_from(index: &serde_json::Value, plat: &str) -> PhpReleases {
    PHP_VERSIONS
        .iter()
        .filter_map(|m| {
            let rel = &index[*m];
            if !plat.is_empty() && rel["files"][format!("fpm-{plat}")].is_null() {
                return None;
            }
            let version = rel["version"].as_str()?.to_string();
            Some((m.to_string(), (version, rel["preview"] == true)))
        })
        .collect()
}

/// `new` is a later patch release than `have`: `8.4.27` after `8.4.9`, `8.6.0` after
/// `8.6.0RC4`, `8.6.0RC5` after that.
// ponytail: alpha/beta/RC count alike; the index only moves forward, so it doesn't matter.
fn newer_patch(have: &str, new: &str) -> bool {
    let key = |v: &str| {
        let last = v.rsplit('.').next()?;
        let (patch, pre) = last.split_at(
            last.find(|c: char| !c.is_ascii_digit())
                .unwrap_or(last.len()),
        );
        let n = match pre.trim_start_matches(|c: char| c.is_ascii_alphabetic()) {
            _ if pre.is_empty() => 0,
            n => n.parse::<u32>().ok()?,
        };
        Some((patch.parse::<u32>().ok()?, pre.is_empty(), n))
    };
    matches!((key(have), key(new)), (Some(a), Some(b)) if b > a)
}

/// The installed PHP versions `releases` has a newer patch of: (minor, installed, newest).
pub fn php_updates(releases: &PhpReleases) -> Vec<(String, String, String)> {
    installed_php()
        .into_iter()
        .filter_map(|minor| {
            let have = opcache::installed_full(&minor).ok()?;
            let new = releases.get(&minor)?.0.clone();
            newer_patch(&have, &new).then_some((minor, have, new))
        })
        .collect()
}

/// A new PHP folder's `php` and server start, or it doesn't replace the one that does.
fn php_runs(dir: &Path) -> Result<()> {
    for bin in [platform::exe("php"), PHP_SERVER.to_string()] {
        let ok = platform::command(dir.join(&bin))
            .args(["-n", "-v"])
            .output()
            .is_ok_and(|o| o.status.success());
        if !ok {
            bail!("the new {bin} doesn't start");
        }
    }
    Ok(())
}

pub fn php_install(cfg: &mut Config, minor: &str) -> Result<()> {
    php_offered(minor).map_err(anyhow::Error::msg)?;
    let index = php_index()?;
    if cfg!(windows) {
        return php_install_windows(cfg, minor, &index);
    }
    let (plat, _) = platform()?;
    let rel = &index[minor];
    let file = |kind: &str| {
        let f = &rel["files"][format!("{kind}-{plat}")];
        f["url"].as_str().zip(f["sha256"].as_str())
    };
    let full = rel["version"]
        .as_str()
        .filter(|_| file("fpm").is_some() && file("cli").is_some())
        .with_context(|| format!("no PHP {minor} build for {plat} yet"))?;
    let preview = if rel["preview"] == true {
        " (preview)"
    } else {
        ""
    };
    let dir = php_dir(minor);
    println!("Installing PHP {full}{preview}…");
    replace_dir(&dir, |new| {
        for kind in ["fpm", "cli"] {
            let (url, sum) = file(kind).context("no PHP build")?;
            fetch_tgz(url, new, false, Some(sum))?;
        }
        php_runs(new)?;
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
    *cfg = update(|c| {
        c.php.get_or_insert_with(|| minor.into());
        Ok(())
    })?
    .0;
    println!("PHP {full}{preview} installed in {}", dir.display());
    opcache::prune_sources();
    // Built in (Anvil's builds, 8.5's): nothing to build. The folder is new, so an
    // opcache.so built for the PHP it replaced is gone with it.
    if opcache::state(minor) == opcache::State::Missing
        && let Err(e) = opcache::build(minor)
    {
        println!("PHP {full} works without OPcache for now: {e}");
    }
    // A new patch release: the extensions turned on for it, for the version or by an
    // app on it, are built again for it.
    phpext::build_missing(cfg, minor);
    Ok(())
}

const PHP_WINDOWS: &str = "https://windows.php.net/downloads/releases/";

/// The extensions a Windows PHP loads: what static-php-cli builds into the Mac's, as far
/// as windows.php.net ships them.
const PHP_WINDOWS_EXT: [&str; 15] = [
    "bz2",
    "curl",
    "fileinfo",
    "gd",
    "gmp",
    "intl",
    "mbstring",
    "mysqli",
    "openssl",
    "pdo_mysql",
    "pdo_pgsql",
    "pdo_sqlite",
    "sodium",
    "sqlite3",
    "zip",
];

/// PHP from windows.php.net: the thread-safe-less x64 build (php-cgi), checked against
/// its published SHA-256, with a php.ini that loads the usual extensions and OPcache.
fn php_install_windows(cfg: &mut Config, minor: &str, list: &serde_json::Value) -> Result<()> {
    let rel = &list[minor];
    let build = ["nts-vs17-x64", "nts-vs16-x64"]
        .iter()
        .find_map(|b| rel[b]["zip"].as_object())
        .with_context(|| format!("windows.php.net has no PHP {minor} build"))?;
    let (file, sum) = (
        build["path"].as_str().context("no file in releases.json")?,
        build["sha256"]
            .as_str()
            .context("no checksum in releases.json")?,
    );
    let full = rel["version"].as_str().unwrap_or(minor).to_string();
    let dir = php_dir(minor);
    println!("Installing PHP {full}…");
    replace_dir(&dir, |new| {
        fetch_tgz(&format!("{PHP_WINDOWS}{file}"), new, false, Some(sum))?;
        php_runs(new)?;
        let ini = dir.join("php.ini");
        if ini.exists() {
            fs::copy(ini, new.join("php.ini"))?;
        } else {
            let ext: String = PHP_WINDOWS_EXT
                .iter()
                .map(|e| format!("extension={e}\n"))
                .collect();
            fs::write(
                new.join("php.ini"),
                format!(
                    "memory_limit = 512M\nupload_max_filesize = 100M\npost_max_size = 100M\nmax_execution_time = 120\n\
                     extension_dir = \"{}\"\n{ext}zend_extension=opcache\nopcache.enable = 1\n",
                    dir.join("ext").display()
                ),
            )?;
        }
        Ok(())
    })?;
    *cfg = update(|c| {
        c.php.get_or_insert_with(|| minor.into());
        Ok(())
    })?
    .0;
    println!("PHP {full} installed in {}", dir.display());
    Ok(())
}

/// Set the PHP version used by every site that isn't pinned.
pub fn set_default_php(cfg: &mut Config, version: &str) -> Result<()> {
    php_offered(version).map_err(anyhow::Error::msg)?;
    if !installed_php().iter().any(|v| v == version) {
        bail!("PHP {version} is not installed — run `anvil php install {version}`");
    }
    cfg.php = Some(version.into());
    save(cfg)
}

/// Pin a site to a PHP version, or back to the default with `None`.
/// A setting for an app Anvil doesn't know would only leave an empty entry behind.
fn known_site(cfg: &Config, site: &str) -> Result<()> {
    find_site(cfg, site).map(drop)
}

/// The app named `site`, or an error that names it.
pub fn find_site(cfg: &Config, site: &str) -> Result<Site> {
    let name = site_name(site);
    sites(cfg)
        .into_iter()
        .find(|s| s.name == name)
        .with_context(|| format!("No app named {site}"))
}

pub fn set_site_php(cfg: &mut Config, site: &str, version: Option<String>) -> Result<()> {
    if let Some(v) = &version {
        php_offered(v).map_err(anyhow::Error::msg)?;
    }
    known_site(cfg, site)?;
    if let Some(v) = &version
        && !installed_php().contains(v)
    {
        bail!("PHP {v} is not installed — run `anvil php install {v}`");
    }
    cfg.sites.entry(site_name(site)).or_default().php = version;
    save(cfg)
}

fn caddy() -> PathBuf {
    home().join("bin").join(platform::exe("caddy"))
}

pub fn ensure_caddy() -> Result<()> {
    if caddy().exists() {
        return Ok(());
    }
    let (_, plat) = platform()?;
    println!("Downloading Caddy {CADDY_VERSION}…");
    let base = format!("https://github.com/caddyserver/caddy/releases/download/v{CADDY_VERSION}/");
    let ext = if cfg!(windows) { "zip" } else { "tar.gz" };
    let file = format!("caddy_{CADDY_VERSION}_{plat}.{ext}");
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
    let mut c = crate::platform::command(caddy());
    c.env("XDG_DATA_HOME", home().join("data"))
        .env("XDG_CONFIG_HOME", home().join("data"));
    c
}

/// A path as the Caddyfile holds it: Windows' backslashes as slashes, which Caddy takes
/// and which can't escape the quotes around it.
pub(crate) fn caddy_path(p: &Path) -> String {
    let s = p.display().to_string();
    if cfg!(windows) {
        s.replace('\\', "/")
    } else {
        s
    }
}

/// Where Caddy sends a site's PHP requests: its pool's socket, or on Windows its
/// php-cgi processes.
fn php_upstream(site: &Site) -> Option<String> {
    if cfg!(windows) {
        let ports = cgi_ports(&php_pool(site)?);
        Some(
            ports
                .map(|p| format!("127.0.0.1:{p}"))
                .collect::<Vec<_>>()
                .join(" "),
        )
    } else {
        Some(format!("unix/{}", fpm_socket(site)?.display()))
    }
}

/// The pool serving a site: its own (`own_pool`), or its PHP version's.
fn php_pool(site: &Site) -> Option<String> {
    let v = site.php.as_ref()?;
    Some(if own_pool(site) {
        pool(site)
    } else {
        format!("php-{v}")
    })
}

/// php-cgi processes per pool. php-cgi answers one request at a time.
const CGI_WORKERS: u16 = 4;

/// A pool's php-cgi ports: a block of `CGI_WORKERS` picked by its name, so the Caddyfile
/// and the processes agree without keeping a table.
// ponytail: two pools whose names hash to one block share it; collisions are rare
// with a few pools and a restart with one of them renamed is the way out.
pub(crate) fn cgi_ports(pool: &str) -> std::ops::Range<u16> {
    let base = 20_000 + (stable_hash(pool) % 2_500) as u16 * CGI_WORKERS;
    base..base + CGI_WORKERS
}

/// Pool name for a site with its own settings (FPM pool names can't contain dots).
pub(crate) fn pool(site: &Site) -> String {
    format!("site-{}", site.name.replace('.', "_"))
}

/// A site with php.ini overrides, or a worktree (its `env[]` lines), gets a pool of its own;
/// one with a master of its own too.
pub(crate) fn own_pool(site: &Site) -> bool {
    !site.ini.is_empty() || site.worktree_of.is_some() || site.own_master
}

/// The socket a site's PHP requests go to: its own pool, or the version's shared pool.
fn fpm_socket(site: &Site) -> Option<PathBuf> {
    let v = site.php.as_ref()?;
    let name = if own_pool(site) {
        pool(site)
    } else {
        format!("php-{v}")
    };
    let sock = home().join("run").join(format!("{name}.sock"));
    // macOS keeps a socket path in 104 bytes; a longer one would stop PHP-FPM for every
    // site of the version. Such a pool listens on a (shorter) hashed name instead.
    let short = home()
        .join("run")
        .join(format!("site-{}.sock", short_hash(&name)));
    if own_pool(site)
        && sock.as_os_str().len() > 100
        && short.as_os_str().len() < sock.as_os_str().len()
    {
        return Some(short);
    }
    Some(sock)
}

/// PHP-FPM config for one version: the shared pool plus one pool per customised site,
/// except the sites with a master of their own (`fpm_site_conf`).
fn fpm_conf(cfg: &Config, v: &str, sites: &[Site]) -> String {
    let mut s = fpm_global(v);
    s += &fpm_pool("www", &home().join("run").join(format!("php-{v}.sock")), 16);
    s += &php_values(sites, cfg.ini.clone());
    for site in sites
        .iter()
        .filter(|s| s.php.as_deref() == Some(v) && own_pool(s) && !s.own_master)
    {
        s += &site_pool(cfg, site, sites);
    }
    s
}

/// PHP-FPM config of a site's own master: its pool only; `None` when it gets no pool.
fn fpm_site_conf(cfg: &Config, v: &str, site: &Site, sites: &[Site]) -> Option<String> {
    Some(fpm_global(v) + &Some(site_pool(cfg, site, sites)).filter(|p| !p.is_empty())?)
}

/// Where a site's own master's config goes.
fn fpm_site_conf_file(v: &str, site: &Site) -> PathBuf {
    home()
        .join("run")
        .join(format!("php-{v}-{}-fpm.conf", pool(site)))
}

fn fpm_global(v: &str) -> String {
    format!(
        "[global]\nerror_log = {}\ndaemonize = no\n",
        php_fpm_log(v).display()
    )
}

fn fpm_pool(name: &str, sock: &Path, max: u8) -> String {
    format!(
        "\n[{name}]\nlisten = {}\npm = ondemand\npm.max_children = {max}\ncatch_workers_output = yes\nclear_env = no\n",
        sock.display()
    )
}

/// A pool's php.ini lines. With the browser bridge on for any app, every pool prepends
/// Anvil's file (pool-wide: a per-request PHP_VALUE stays on the worker for the next
/// app's requests), which includes the user's own `auto_prepend_file` through `env`.
fn php_values(sites: &[Site], ini: BTreeMap<String, String>) -> String {
    let (ini, theirs) = browser::pool_ini(sites, ini);
    let mut s: String = ini
        .iter()
        .map(|(k, val)| format!("php_value[{k}] = {val}\n"))
        .collect();
    if let Some(p) = theirs.filter(|p| !p.contains('"')) {
        s += &format!("env[ANVIL_PREPEND] = \"{p}\"\n");
    }
    s
}

/// A site's own pool, with the version's and its own php.ini values and a worktree's
/// `env[]` lines; nothing for a worktree Anvil can't separate from main (Caddy shows why).
fn site_pool(cfg: &Config, site: &Site, sites: &[Site]) -> String {
    let Ok(env) = worktrees::web_env(cfg, site) else {
        return String::new();
    };
    let mut s = fpm_pool(&pool(site), &fpm_socket(site).unwrap(), 8);
    let mut ini = cfg.ini.clone();
    ini.extend(site.ini.clone());
    s += &php_values(sites, ini);
    // Quoted: URLs hold `?`, `=`, `&`. One bad line would stop every pool; `env`
    // already refuses a database value that isn't safe, so only a shared one is left out.
    for (k, val) in env.iter().filter(|(k, v)| worktrees::fpm_safe(k, v)) {
        s += &format!("env[{k}] = \"{val}\"\n");
    }
    s
}

/// Reject anything that could break out of a `php_value[key] = value` line.
pub(crate) fn check_ini(key: &str, value: &str) -> Result<()> {
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
/// HTTPS for new applications. The ones there now keep what they have, so turning
/// it on or off never moves an existing app.
pub fn set_https_default(cfg: &mut Config, on: bool) {
    for site in sites(cfg) {
        cfg.sites
            .entry(site.name)
            .or_default()
            .https
            .get_or_insert(site.secure);
    }
    cfg.https_default = Some(on);
}

/// The Packages tab's badges for a site, `None` back to the defaults (a worktree's: its app's).
pub fn set_packages_pinned(cfg: &mut Config, site: &str, pins: Option<Vec<String>>) -> Result<()> {
    known_site(cfg, site)?;
    cfg.sites
        .entry(site_name(site))
        .or_default()
        .packages_pinned = pins;
    Ok(())
}

pub fn set_secure(cfg: &mut Config, site: &str, on: bool) -> Result<()> {
    known_site(cfg, site)?;
    cfg.sites.entry(site_name(site)).or_default().https = Some(on);
    save(cfg)
}

/// A web root as stored: relative to the project folder, inside it, `.` for the folder
/// itself. It ends up quoted in the Caddyfile and in an error page there, so nothing that
/// could end the quote or read as a Caddy placeholder gets through.
pub fn web_root(dir: &str) -> Result<String> {
    let d = dir.trim();
    if d.is_empty() {
        bail!("Name a folder inside the project, such as public, or . for the project folder");
    }
    if d.contains(['"', '\\', '`', '{', '}', '\n', '\r']) {
        bail!("A web root can't hold quotes, backslashes, braces or line breaks");
    }
    if d.starts_with(['/', '~']) {
        bail!("The web root is relative to the project folder (public, web, dist), not {d}");
    }
    let mut parts = vec![];
    for c in Path::new(d).components() {
        match c {
            std::path::Component::Normal(p) => parts.push(p.to_string_lossy().into_owned()),
            std::path::Component::CurDir => {}
            _ => bail!("The web root has to be inside the project folder; {d} leaves it"),
        }
    }
    Ok(if parts.is_empty() {
        ".".into()
    } else {
        parts.join("/")
    })
}

/// Only apps Anvil serves from a folder (PHP, static) have a web root: not a proxied dev
/// server, a Compose project or a plain folder.
pub fn has_web_root(s: &Site) -> Result<()> {
    if apps::runs_process(&s.kind) || !apps::serves(&s.kind) {
        bail!(
            "{} is a {} app, which Anvil doesn't serve from a folder, so it has no web root",
            s.name,
            apps::label(&s.kind)
        );
    }
    Ok(())
}

/// Serve the app from `dir`, a folder inside its project (see `web_root`); `None` goes
/// back to `public/` when present, else the project folder. Only for apps Anvil serves
/// itself: a proxied dev server or Compose project has no web root of Anvil's.
pub fn set_site_root(cfg: &mut Config, site: &str, dir: Option<&str>) -> Result<()> {
    let name = site_name(site);
    let root = match dir {
        None => None,
        Some(d) => {
            let s = find_site(cfg, site)?;
            has_web_root(&s)?;
            let rel = web_root(d)?;
            if !s.path.join(&rel).is_dir() {
                bail!("{rel} isn't a folder in {}", s.path.display());
            }
            Some(rel)
        }
    };
    cfg.sites.entry(name).or_default().root = root;
    save(cfg)
}

pub fn set_site_ini(cfg: &mut Config, site: &str, key: &str, value: Option<&str>) -> Result<()> {
    known_site(cfg, site)?;
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
        // trusted_proxies: what comes from this computer keeps its X-Forwarded-*, so an app
        // shared through a tunnel (`tunnel`) learns the public host it is reached at.
        "{{\n\tdefault_bind 127.0.0.1 [::1]\n\thttp_port {}\n\thttps_port {}\n\tadmin off\n\tlocal_certs\n\tskip_install_trust\n\tauto_https disable_redirects\n\tservers {{\n\t\ttrusted_proxies static 127.0.0.1/32 ::1/128\n\t}}\n{}}}\n",
        cfg.http_port,
        cfg.https_port,
        // The browser bridge's logs, one per app with it on (`browser::site_lines`).
        sites
            .iter()
            .filter(|s| browser::on(s))
            .filter_map(|s| browser::logger(&s.name))
            .collect::<String>()
    );
    // A secure site's HTTP addresses redirect; 302, so turning HTTPS off isn't
    // undone by a redirect the browser remembers.
    let https_port = match public_https_port(cfg) {
        443 => String::new(),
        p => format!(":{p}"),
    };
    let redirect = |site: &Site, names: &[String]| {
        let http: Vec<_> = names.iter().map(|n| format!("http://{n}")).collect();
        format!(
            "\n{} {{\n{}\tredir https://{{host}}{https_port}{{uri}}\n}}\n",
            http.join(", "),
            access_log(cfg, &site.name)
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
    // A block's addresses and opening brace, then where it logs its requests.
    let open = |site: &Site, names: &[String]| {
        format!(
            "{} {{\n{}",
            addresses(site, names),
            access_log(cfg, &site.name)
        )
    };
    // Caddy refuses a whole Caddyfile that has one address twice, so each host goes to one
    // block: sites (with their aliases) first, then dev-server rows, then old-host redirects.
    let mut used = BTreeSet::new();
    let mut claim = |n: &String| {
        let fresh = used.insert(n.clone());
        if !fresh {
            static SAID: std::sync::Mutex<BTreeSet<String>> =
                std::sync::Mutex::new(BTreeSet::new());
            if lock(&SAID).insert(n.clone()) {
                eprintln!("Anvil: {n} is another site's address; left out");
            }
        }
        fresh
    };
    let own: Vec<Vec<String>> = sites
        .iter()
        .map(|site| {
            // Containers answer on their own ports; a plain folder has nothing to serve.
            if !apps::serves(&site.kind) {
                return vec![];
            }
            let mut names = vec![format!("{}.{}", site.name, cfg.tld)];
            names.extend(site.aliases.iter().cloned());
            // Dev servers that only trust localhost (Next.js) work at <name>.localhost too.
            if apps::runs_process(&site.kind) && site.port.is_some() && cfg.tld != "localhost" {
                names.push(format!("{}.localhost", site.name));
                for a in &site.aliases {
                    let stem = a.strip_suffix(&format!(".{}", cfg.tld)).unwrap_or(a);
                    names.push(format!("{stem}.localhost"));
                }
            }
            names.retain(&mut claim);
            names
        })
        .collect();
    let mut old_hosts = vec![];
    for (site, names) in sites.iter().zip(own) {
        if names.is_empty() {
            continue;
        }
        if let (true, Some(port)) = (apps::runs_process(&site.kind), site.port) {
            // The browser bridge: its collector and script, and page loads through Anvil.
            let bridge = |port: u16| match browser::on(site) {
                true => browser::site_lines(&site.name) + &browser::doc_route(port),
                false => String::new(),
            };
            s += &format!(
                "\n{}{}\treverse_proxy 127.0.0.1:{port}\n\thandle_errors {{\n\t\theader Content-Type \"text/html; charset=utf-8\"\n\t\trespond `{}` 502\n\t}}\n}}\n",
                open(site, &names),
                bridge(port),
                not_running_page(&site.name),
            );
            if site.secure {
                s += &redirect(site, &names);
            }
            // The app's other dev servers (a docs site) answer at <app>--<row>.<tld>.
            for row in processes::server_rows(cfg, site) {
                let mut names = vec![row.host.clone()];
                if cfg.tld != "localhost" {
                    let stem = row.host.strip_suffix(&format!(".{}", cfg.tld));
                    names.push(format!("{}.localhost", stem.unwrap_or(&row.host)));
                }
                // A site of that name keeps it (the spec's order: the app wins).
                names.retain(&mut claim);
                if names.is_empty() {
                    continue;
                }
                s += &format!(
                    "\n{}{}\treverse_proxy 127.0.0.1:{}\n\thandle_errors {{\n\t\theader Content-Type \"text/html; charset=utf-8\"\n\t\trespond `{}` 502\n\t}}\n}}\n",
                    open(site, &names),
                    bridge(row.port),
                    row.port,
                    not_running_page(&names[0]),
                );
                if site.secure {
                    s += &redirect(site, &names);
                }
                if site.worktree_of.is_none() {
                    // A worktree never had an old host.
                    old_hosts.push((site, row));
                }
            }
            continue;
        }
        // A quote, backslash or line break in the folder would end the quoted root and
        // break (or rewrite) the whole Caddyfile; such a folder isn't served.
        if caddy_path(&site.root).contains(['"', '\\', '\n', '\r']) {
            continue;
        }
        // A web root set by the user: one edited by hand to leave the project isn't served;
        // one that's gone says so rather than serving the project folder.
        if let Some(dir) = &site.root_custom {
            if web_root(dir).is_err() {
                continue;
            }
            if !site.root.is_dir() {
                s += &format!(
                    "\n{}\theader Content-Type \"text/html; charset=utf-8\"\n\trespond `{}` 502\n}}\n",
                    open(site, &names),
                    error_page(
                        &format!("{}'s web root {dir} doesn't exist", site.name),
                        &format!(
                            "Create the folder, or choose another web root in the app's Settings (anvil app root {}).",
                            site.name
                        ),
                    ),
                );
                if site.secure {
                    s += &redirect(site, &names);
                }
                continue;
            }
        }
        if let Err(reason) = worktrees::web_env(cfg, site) {
            s += &format!(
                "\n{}\theader Content-Type \"text/html; charset=utf-8\"\n\trespond `{}` 502\n}}\n",
                open(site, &names),
                error_page(
                    &format!("{} has no database of its own", site.name),
                    &format!(
                        "Anvil can't point it at its own database ({reason}), so nothing of it runs."
                    ),
                ),
            );
            if site.secure {
                s += &redirect(site, &names);
            }
            continue;
        }
        s += &format!(
            "\n{}\troot * \"{}\"\n\tencode\n{HIDDEN}",
            open(site, &names),
            caddy_path(&site.root)
        );
        // The mail catcher's env as FastCGI params: no pool of its own needed, and
        // PHP-FPM and Windows' php-cgi read them alike (Laravel: `$_SERVER` over `.env`).
        // So is the browser bridge's switch, which the pool-wide prepend reads.
        let mut params = mail::env(cfg, site);
        if browser::on(site) {
            s += &browser::site_lines(&site.name);
            params.push(("ANVIL_INJECT".into(), "1".into()));
        }
        let fpm = php_upstream(site).map(|upstream| match params.is_empty() {
            true => format!("php_fastcgi {upstream}\n"),
            false => format!(
                "php_fastcgi {upstream} {{\n{}\t}}\n",
                params
                    .iter()
                    .map(|(k, v)| format!("\t\tenv {k} \"{v}\"\n"))
                    .collect::<String>()
            ),
        });
        // Octane's row while it runs; PHP-FPM when it doesn't answer (stopped, crashed).
        // A file served for an error keeps the error's status unless told otherwise.
        if let Some(port) = processes::octane_row(cfg, site).and_then(|p| p.port) {
            // The browser bridge: Octane's pages through Anvil's proxy, which leaves a
            // stopped Octane's unanswered, so PHP-FPM and its prepend take them.
            if browser::on(site) {
                s += &browser::doc_route(port);
            }
            s += &format!("\treverse_proxy 127.0.0.1:{port}\n\thandle_errors 502 {{\n");
            if let Some(fpm) = &fpm {
                s += &format!("\t\t{fpm}");
            }
            s += "\t\tfile_server @hidden {\n\t\t\thide *\n\t\t}\n\t\tfile_server {\n\t\t\tstatus 200\n\t\t}\n\t}\n}\n";
        } else {
            if let Some(fpm) = &fpm {
                s += &format!("\t{fpm}");
            }
            s += "\tfile_server @hidden {\n\t\thide *\n\t}\n\tfile_server\n}\n";
        }
        if site.secure {
            s += &redirect(site, &names);
        }
    }
    // ponytail: remove the old-host redirect one release after 0.2
    for (site, row) in old_hosts {
        let old = row.old_host;
        if !claim(&old) {
            continue;
        }
        let to = site_url(
            cfg,
            &Site {
                host: row.host.clone(),
                ..site.clone()
            },
        );
        let mut from = vec![addresses(site, std::slice::from_ref(&old))];
        if site.secure {
            from.push(format!("http://{old}"));
        }
        for n in from {
            s += &format!("\n{n} {{\n\tredir {to}{{uri}} 308\n}}\n");
        }
    }
    // Settings › Features › Database viewer: last, so an app with its address keeps it.
    if let Some(v) = dbviewer::php(cfg)
        && claim(&dbviewer::host(cfg))
    {
        s += &dbviewer::caddy_block(cfg, &version_upstream(&v));
    }
    // Any other host on the HTTP port: a plain 404 rather than Caddy's empty 200. Plain
    // text, so the Host can't inject markup. HTTPS has no certificate for it: TLS fails.
    s += "\nhttp:// {\n\theader Content-Type \"text/plain; charset=utf-8\"\n\trespond \"No app at {host}\" 404\n}\n";
    s
}

/// Where a PHP version's shared pool takes requests: its socket, or Windows' php-cgi ports.
fn version_upstream(v: &str) -> String {
    let pool = format!("php-{v}");
    if cfg!(windows) {
        let ports: Vec<_> = cgi_ports(&pool).map(|p| format!("127.0.0.1:{p}")).collect();
        ports.join(" ")
    } else {
        format!(
            "unix/{}",
            home().join("run").join(format!("{pool}.sock")).display()
        )
    }
}

/// The PHP versions that run: the apps', and the Database viewer's.
fn php_versions(cfg: &Config, sites: &[Site]) -> BTreeSet<String> {
    sites
        .iter()
        .filter_map(|s| s.php.clone())
        .chain(dbviewer::php(cfg))
        .collect()
}

/// What a site's file server never serves: a file whose path under the web root has a
/// part starting with a dot (`.env`, `.git/config`), answered 404 by a file server that
/// hides everything. `hide .*` would also match the folders of the web root's own path
/// (`.output/public`, a project in `~/.sites`), hiding every file.
fn is_true(b: &bool) -> bool {
    *b
}

const HIDDEN: &str = "\t@hidden path */.*\n";

/// A site block's access log: JSON, one file per app (its server rows and redirects too),
/// rolled by Caddy at 2 MiB with one older copy kept a day, so an app holds at most ~4 MB.
/// Caddy writes Cookie, Set-Cookie, Authorization and Proxy-Authorization as REDACTED.
/// A home whose path would break out of the quotes gets no log, nor does any app while
/// Request log is off.
fn access_log(cfg: &Config, site: &str) -> String {
    let path = caddy_path(&requests::file(site));
    if !cfg.request_log || path.contains(['"', '\\', '\n', '\r']) {
        return String::new();
    }
    format!(
        "\tlog {{\n\t\toutput file \"{path}\" {{\n\t\t\troll_size 2MiB\n\t\t\troll_keep 1\n\t\t\troll_keep_for 24h\n\t\t}}\n\t\tformat json\n\t}}\n"
    )
}

/// Shown instead of a bare 502 when an app that runs its own server is stopped.
fn not_running_page(name: &str) -> String {
    error_page(
        &format!("{name} is not running"),
        "Start it in Anvil, then reload this page.",
    )
}

/// A plain page for Caddy's `respond`; `title` and `text` must hold no backtick.
fn error_page(title: &str, text: &str) -> String {
    format!(
        "<!doctype html><title>{title}</title><body style=\"font:15px -apple-system,system-ui;display:grid;place-items:center;height:90vh;color:#444\"><div><h2>{title}</h2><p>{text}</p></div>"
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
    known_site(cfg, site)?;
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
        if rt == "dotnet" {
            bail!(".NET picks its SDK from the project's global.json; Anvil doesn't pin it");
        }
        if !runtimes::installed(rt).contains(v) {
            bail!("{} {v} isn't installed", runtimes::name(rt));
        }
    }
    cfg.sites.entry(name).or_default().runtime = version;
    save(cfg)
}

/// True when the running server was started with a different set of sites, or its PHP
/// masters with other extensions (`anvil php ext add` in a terminal).
pub fn needs_restart(cfg: &Config) -> bool {
    let sites = sites(cfg);
    let run = home().join("run");
    let differs = |file: &str, now: String| fs::read_to_string(run.join(file)).ok() != Some(now);
    differs("Caddyfile", caddyfile(cfg, &sites)) || differs("php-masters", php_masters(cfg, &sites))
}

/// What each PHP master starts with (`phpext::php_args_with`), a line each: the
/// version's, then each app's own.
fn php_masters(cfg: &Config, sites: &[Site]) -> String {
    let versions = php_versions(cfg, sites);
    let mut out = String::new();
    for v in &versions {
        out += &format!(
            "PHP {v}: {}\n",
            phpext::php_args_with(cfg, v, None).join(" ")
        );
        for s in sites
            .iter()
            .filter(|s| s.own_master && s.php.as_deref() == Some(v))
        {
            let args = phpext::php_args_with(cfg, v, Some(s));
            out += &format!("PHP {v} ({}): {}\n", s.name, args.join(" "));
        }
    }
    out
}

pub fn trust() -> Result<()> {
    // Caddy creates this CA on its first start.
    let root = root_cert();
    if !root.exists() {
        bail!("No certificate authority yet — start Anvil once, then trust it");
    }
    if cfg!(windows) {
        // The user's own Root store: no administrator, Windows asks in a dialog.
        return run(platform::command("certutil")
            .args(["-user", "-addstore", "Root"])
            .arg(root));
    }
    // ponytail: Linux (update-ca-certificates / NSS) comes with the Linux port.
    if !cfg!(target_os = "macos") {
        bail!("{LINUX_NOT_YET}");
    }
    let keychain = user_home().join("Library/Keychains/login.keychain-db");
    run(crate::platform::command("security")
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
    /// `git` runs. macOS brings it with the Xcode tools; Windows doesn't (`install_git`).
    pub git: bool,
    /// A Node.js is installed, or no app needs one: none is a Node.js app or has a
    /// package.json (a Laravel app's Vite).
    pub node: bool,
}

/// Whether `git` runs here. Once it has, it is not asked again: every window refresh
/// asks (`setup_status`). Until then it is, so an install shows at once.
pub fn git_installed() -> bool {
    static FOUND: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    use std::sync::atomic::Ordering::Relaxed;
    FOUND.load(Relaxed) || {
        let ok = platform::command("git")
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success());
        FOUND.store(ok, Relaxed);
        ok
    }
}

/// Windows: Git for Windows, through winget (Windows asks once when its installer goes
/// into Program Files), then found on the PATH it added without a restart.
pub fn install_git() -> Result<()> {
    if !cfg!(windows) {
        bail!("Install Git with the Xcode command line tools: xcode-select --install");
    }
    let out = platform::command("winget")
        .args([
            "install",
            "--id",
            "Git.Git",
            "--exact",
            "--silent",
            "--accept-package-agreements",
            "--accept-source-agreements",
            "--disable-interactivity",
        ])
        .output();
    match out {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            bail!("winget isn't on this Windows: get Git from https://git-scm.com/download/win")
        }
        Err(e) => bail!("couldn't run winget: {e}"),
        Ok(o) if !o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout);
            let why = text
                .lines()
                .map(str::trim)
                .rfind(|l| !l.is_empty() && !l.chars().all(|c| "-\\|/ ".contains(c)))
                .unwrap_or("winget failed");
            bail!("Installing Git failed: {why}")
        }
        Ok(_) => {}
    }
    platform::refresh_path();
    if !git_installed() {
        bail!("Git is installed; open Anvil again to use it");
    }
    Ok(())
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
    // Windows, Linux: Caddy listens on 80 and 443 itself when they are its ports.
    if !cfg!(target_os = "macos") {
        return cfg.http_port == 80 && cfg.https_port == 443;
    }
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
    // ponytail: Linux can't forward 80/443 or trust the authority yet; offering those
    // steps would only refuse, so they count as done until it can.
    let linux = cfg!(target_os = "linux");
    Setup {
        resolver: !needs_resolver(cfg)
            || if cfg!(windows) {
                nrpt_ready(cfg)
            } else {
                fs::read_to_string(file)
                    .is_ok_and(|s| s.split_whitespace().eq(want.split_whitespace()))
            },
        trusted: linux || !root.exists() || cert_trusted(&root),
        php: !installed_php().is_empty(),
        ports: linux || ports_forwarded(cfg),
        shims: shims::ready(),
        git: git_installed(),
        node: !runtimes::installed("node").is_empty()
            || !sites(cfg).iter().any(|s| {
                apps::runtime(&s.kind) == Some("node") || s.path.join("package.json").is_file()
            }),
    }
}

/// Whether the system trusts `root`: the Mac's `security verify-cert`; on Windows, the
/// certificate is in the user's or the machine's Root store. A yes is kept for the same
/// file (its size and time): every window refresh asks, and on Windows that is a
/// PowerShell. A new certificate, or a no, is asked again.
// ponytail: trust removed by hand in Keychain Access shows after a relaunch.
fn cert_trusted(root: &Path) -> bool {
    static TRUSTED: std::sync::Mutex<Option<(PathBuf, u64, Option<std::time::SystemTime>)>> =
        std::sync::Mutex::new(None);
    let id = fs::metadata(root)
        .ok()
        .map(|m| (root.to_path_buf(), m.len(), m.modified().ok()));
    if id.is_some() && *lock(&TRUSTED) == id {
        return true;
    }
    let ok = cert_trusted_now(root);
    *lock(&TRUSTED) = if ok { id } else { None };
    ok
}

fn cert_trusted_now(root: &Path) -> bool {
    if cfg!(windows) {
        let script = "$c = New-Object Security.Cryptography.X509Certificates.X509Certificate2 $env:ANVIL_CERT; \
             if ((Test-Path \"Cert:\\CurrentUser\\Root\\$($c.Thumbprint)\") -or (Test-Path \"Cert:\\LocalMachine\\Root\\$($c.Thumbprint)\")) { exit 0 } else { exit 1 }";
        return platform::command("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .env("ANVIL_CERT", root)
            .output()
            .is_ok_and(|o| o.status.success());
    }
    if !cfg!(target_os = "macos") {
        return false;
    }
    platform::command("security")
        .arg("verify-cert")
        .arg("-c")
        .arg(root)
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Windows: a Name Resolution Policy rule sends `.<tld>` to Anvil's DNS server. NRPT
/// can't name a port, so that server listens on 53. The rules live in the registry,
/// one key per rule, `Name` the suffix (with its dot) and `GenericDNSServers` the server.
fn nrpt_ready(cfg: &Config) -> bool {
    let out = platform::command("reg")
        .args([
            "query",
            r"HKLM\SYSTEM\CurrentControlSet\Services\Dnscache\Parameters\DnsPolicyConfig",
            "/s",
        ])
        .output();
    let Ok(out) = out else {
        return false;
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let name = format!(".{}", cfg.tld);
    // A rule is a block of values under its key; ours names the suffix and 127.0.0.1.
    text.split("\r\n\r\n").any(|rule| {
        let value = |v: &str| {
            rule.lines()
                .filter_map(|l| l.trim().strip_prefix(v))
                .filter_map(|l| l.split_once("REG_"))
                .filter_map(|(_, r)| r.split_once(char::is_whitespace))
                .map(|(_, val)| val.trim().to_string())
                .next()
        };
        value("Name").is_some_and(|n| n.split("\\0").any(|n| n.eq_ignore_ascii_case(&name)))
            && value("GenericDNSServers").is_some_and(|s| s.split(';').any(|s| s == "127.0.0.1"))
            && cfg.dns_port == 53
    })
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
    if cfg!(windows) {
        return setup_script_windows(cfg, resolver, cert);
    }
    if !cfg!(target_os = "macos") {
        if (resolver && needs_resolver(cfg)) || ports_plist.is_some() || cert.is_some() {
            bail!("{LINUX_NOT_YET}");
        }
        return Ok(String::new());
    }
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

/// `setup_script` for Windows, in PowerShell for `run_as_admin`: the NRPT rule for the
/// suffix (an old one for it replaced) and the authority trusted machine-wide. Windows
/// needs no ports step.
fn setup_script_windows(cfg: &Config, resolver: bool, cert: Option<&Path>) -> Result<String> {
    let q = |s: &str| format!("'{}'", s.replace('\'', "''"));
    let mut steps = Vec::new();
    if resolver && needs_resolver(cfg) {
        clean_tld(&cfg.tld)?;
        let ns = q(&format!(".{}", cfg.tld));
        steps.push(format!(
            "Get-DnsClientNrptRule | Where-Object {{ $_.Namespace -contains {ns} }} | Remove-DnsClientNrptRule -Force; \
             Add-DnsClientNrptRule -Namespace {ns} -NameServers '127.0.0.1' | Out-Null; Clear-DnsClientCache"
        ));
    }
    if let Some(cert) = cert {
        steps.push(format!(
            "Import-Certificate -FilePath {} -CertStoreLocation Cert:\\LocalMachine\\Root | Out-Null",
            q(&cert.to_string_lossy())
        ));
    }
    Ok(steps.join("\r\n"))
}

/// Lock a mutex even if a thread panicked while holding it: the data is still there,
/// and one panic must not turn every later lock into another (the app would die and
/// Quit would break).
pub fn lock<T: ?Sized>(m: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

static DNS: Once = Once::new();
/// The suffix the DNS server answers for; every start sets it.
static DNS_TLD: std::sync::RwLock<String> = std::sync::RwLock::new(String::new());
/// Why the DNS server isn't answering: its port was taken, or its thread ended.
static DNS_ERROR: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

fn dns_failed(why: String) {
    eprintln!("{why}");
    *crate::lock(&DNS_ERROR) = Some(why);
}

/// Set once the DNS server can't answer; it isn't restarted within this process.
pub fn dns_error() -> Option<String> {
    crate::lock(&DNS_ERROR).clone()
}

fn pid_file() -> PathBuf {
    home().join("run").join("pids")
}

pub(crate) fn alive(pid: &str) -> bool {
    pid.parse().is_ok_and(platform::alive)
}

/// SIGTERM to one process, if it is still there.
pub fn sigterm(pid: u32) {
    platform::term(pid, false);
}

/// When a process started (`platform::started`). With the pid it names one process: a
/// reused pid has another start time.
pub(crate) fn started(pid: u32) -> Option<String> {
    platform::started(pid)
}

/// Clean up after an Anvil that died without stopping its children (crash, SIGKILL).
/// The pid file is `owner pid` followed by one line per child: `pid` or `pid started`.
/// Children are only stopped when the owner is gone and the process is still the one
/// Anvil started: the same start time when recorded, or a command line naming the
/// file's Anvil home (a dev shell's `anvil:<home>:<key>` tag; a start time recorded
/// before `started` fixed its locale reads differently). With `group`, child pids are
/// process-group leaders and the whole group stops, also when only the leader ended
/// (no other process can take a pid while its group lives). SIGTERM, SIGKILL after
/// `apps::GRACE`, and only then is the pid file removed.
pub(crate) fn kill_orphans(pid_file: &Path, group: bool) {
    let Ok(file) = fs::read_to_string(pid_file) else {
        return;
    };
    let mut pids = file.lines();
    if pids.next().is_none_or(owner_alive) {
        return;
    }
    // Every pid file is `<home>/run/<name>`.
    let ours = pid_file
        .parent()
        .and_then(Path::parent)
        .map(|h| h.display().to_string());
    let named = |pid: u32| {
        platform::command_line(pid).is_some_and(|c| ours.as_ref().is_some_and(|h| c.contains(h)))
    };
    let mut stopping = vec![];
    let pids: Vec<&str> = pids.collect();
    let table = platform::processes();
    for line in pids {
        let (pid, at) = line
            .split_once(' ')
            .map_or((line, None), |(p, at)| (p, Some(at)));
        let Ok(n) = pid.parse::<u32>() else {
            continue;
        };
        let same = if alive(pid) {
            match at {
                // Windows reuses pids at once, and `named` there is only the program's
                // path (any of Anvil's PHPs or servers): a start time, recorded to the
                // 100 ns, is the one sure sign.
                Some(at) if cfg!(windows) => started(n).as_deref() == Some(at),
                _ => at.is_some_and(|at| started(n).as_deref() == Some(at)) || named(n),
            }
        } else {
            group && platform::group_alive(n)
        };
        if same && platform::term(n, group) {
            stopping.push(n);
        }
    }
    let strays = if group {
        platform::left_groups(&table, &stopping)
    } else {
        vec![]
    };
    term_strays(&strays);
    finish_stopping(stopping, group);
    kill_strays(&strays);
    let _ = fs::remove_file(pid_file);
}

/// After SIGTERM: wait up to `apps::GRACE` for them to end, then SIGKILL what is left.
fn finish_stopping(stopping: Vec<u32>, group: bool) {
    let running = |p: u32| {
        if group {
            platform::group_alive(p)
        } else {
            platform::alive(p)
        }
    };
    let deadline = std::time::Instant::now() + apps::GRACE;
    while stopping.iter().any(|p| running(*p)) && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    for p in stopping {
        platform::kill(p, group);
    }
}

/// Stop the dev shells a killed Anvil left that no pid file names any more (an older
/// Anvil's sweep dropped the file, a later start wrote over it): orphaned group leaders
/// whose last argument is the `anvil:<home>:<key>` tag `spawn` gives them, where `ours`
/// accepts `<home>:<key>`. A live Anvil's children have a parent, so they are left alone.
pub(crate) fn kill_tagged_orphans(ours: impl Fn(&str) -> bool) {
    let table = platform::processes();
    let leaders: Vec<u32> = table
        .iter()
        .filter(|(pid, ppid, pgid, c)| {
            *ppid == 1 && pgid == pid && c.rsplit_once(" anvil:").is_some_and(|(_, t)| ours(t))
        })
        .map(|p| p.0)
        .collect();
    let strays = platform::left_groups(&table, &leaders);
    let stopping = leaders
        .into_iter()
        .filter(|p| platform::term(*p, true))
        .collect();
    term_strays(&strays);
    finish_stopping(stopping, true);
    kill_strays(&strays);
}

/// SIGTERM to what left a stopping group (`platform::left_groups`).
fn term_strays(strays: &[(u32, String)]) {
    for (p, _) in strays {
        platform::term(*p, false);
    }
}

/// After the grace: SIGKILL what is still there of `strays`, only while its pid still
/// runs the same command line (not a later process given that pid).
fn kill_strays(strays: &[(u32, String)]) {
    for (p, c) in strays {
        if platform::alive(*p) && platform::command_line(*p).is_some_and(|now| now.trim() == c) {
            platform::kill(*p, false);
        }
    }
}

/// A pid file's first line: this Anvil's pid and start time (`kill_orphans`).
pub(crate) fn owner_line() -> String {
    let me = std::process::id();
    format!("{me} {}", started(me).unwrap_or_default())
        .trim_end()
        .to_string()
}

/// Whether the Anvil a pid file's first line names still runs: its pid, and its start
/// time when recorded, as a reused pid (Windows hands them out again at once) has another.
fn owner_alive(line: &str) -> bool {
    let (pid, at) = line
        .split_once(' ')
        .map_or((line, None), |(p, at)| (p, Some(at)));
    alive(pid)
        && at.is_none_or(|at| {
            pid.parse()
                .ok()
                .and_then(started)
                .is_none_or(|now| now == at)
        })
}

/// Whether something listens on this Mac's `port`.
pub fn port_answers(port: u16) -> bool {
    // Bounded: on Windows a refused connection to loopback takes ~2 s (it retries the
    // SYN); one that is listening answers in well under this.
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(250)).is_ok()
}

/// Whether a program here holds `port` on loopback: it can't be bound there, or it
/// answers. Binding stays inside this Mac, so it can't hang the way a connection straight
/// to Caddy's own port behind pf's 80/443 forwarding can (`port_answers` then reads
/// "free" while Caddy listens).
fn port_held(port: u16) -> bool {
    // The probe's own listener is closed before asking, or it would answer itself.
    let in_use = std::net::TcpListener::bind(("127.0.0.1", port))
        .is_err_and(|e| e.kind() == std::io::ErrorKind::AddrInUse);
    in_use || port_answers(port)
}

/// The first of `ports` that neither a program here nor `taken` uses.
pub(crate) fn free_port(taken: &[u16], mut ports: std::ops::Range<u16>) -> Option<u16> {
    ports.find(|p| !taken.contains(p) && !port_answers(*p))
}

/// Remove a pid file only while it still names `pid`: a stop that finishes after a new
/// start must not drop the new process's file.
pub(crate) fn remove_pid_file(file: &Path, pid: u32) {
    let ours = fs::read_to_string(file).is_ok_and(|f| {
        f.lines()
            .nth(1)
            .and_then(|l| l.split(' ').next())
            .is_some_and(|p| p == pid.to_string())
    });
    if ours {
        let _ = fs::remove_file(file);
    }
}

/// Ask a child to shut down cleanly (SIGTERM), force it (SIGKILL) after `grace`, and
/// reap it. A child started as a process-group leader (`process_group(0)`) is stopped
/// with its whole group, so `npm run dev` leaves no node behind; any other child alone.
pub(crate) fn terminate(child: &mut Child, grace: std::time::Duration) {
    terminate_all(std::slice::from_mut(child), grace);
}

/// `terminate` for many at once: SIGTERM to all, one shared `grace` for them to exit,
/// then SIGKILL what is left. Stopping N takes one grace, not N.
pub(crate) fn terminate_all(children: &mut [Child], grace: std::time::Duration) {
    // What left the groups (Next's telemetry flusher) is found by its parents, so before
    // the groups end.
    let ids: Vec<u32> = children.iter().map(|c| c.id()).collect();
    let strays = platform::left_groups(&platform::processes(), &ids);
    term_strays(&strays);
    let groups: Vec<bool> = children
        .iter()
        .map(|c| {
            let group = platform::term(c.id(), true);
            if !group {
                platform::term(c.id(), false);
            }
            group
        })
        .collect();
    let deadline = std::time::Instant::now() + grace;
    let mut exited = vec![false; children.len()];
    while exited.contains(&false) && std::time::Instant::now() < deadline {
        for (c, e) in children.iter_mut().zip(&mut exited) {
            *e = *e || matches!(c.try_wait(), Ok(Some(_)));
        }
        if exited.contains(&false) {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
    for ((c, group), exited) in children.iter_mut().zip(groups).zip(exited) {
        // Anything in the group still alive gets killed, even after its leader ended.
        if group {
            platform::kill(c.id(), true);
        }
        if !exited {
            let _ = c.kill();
        }
        let _ = c.wait();
    }
    kill_strays(&strays);
}

/// A process taken out of `Apps`, `Services` or `Server` (`take`, `take_all`), still to
/// be stopped: a shutdown takes seconds, so it happens once their lock is released.
pub struct Stopping {
    child: Child,
    /// Removed once it has stopped, if they still name it (`remove_pid_file`).
    pid_files: Vec<PathBuf>,
    /// The app's folder: what its dev server leaves there is cleared once it has stopped
    /// (`apps::clear_vite_hot`).
    pub(crate) dir: Option<PathBuf>,
}

impl Stopping {
    pub fn wait(self, grace: std::time::Duration) {
        stop_all(vec![self], grace);
    }
}

/// Stop them all at once against one shared deadline (`terminate_all`), then remove
/// their pid files.
pub fn stop_all(list: Vec<Stopping>, grace: std::time::Duration) {
    let mut dirs = vec![];
    let (mut children, files): (Vec<Child>, Vec<Vec<PathBuf>>) = list
        .into_iter()
        .map(|s| {
            dirs.extend(s.dir);
            (s.child, s.pid_files)
        })
        .unzip();
    terminate_all(&mut children, grace);
    for (c, files) in children.iter().zip(files) {
        for f in files {
            remove_pid_file(&f, c.id());
        }
    }
    for d in dirs {
        apps::clear_vite_hot(&d);
    }
}

/// Running PHP-FPM pools and the web server.
/// The files Anvil generates in `run` for one app (its own PHP-FPM master's config, its
/// php.ini folders, Octane's Caddyfile) whose app is gone, or on another PHP version, and
/// the config of a PHP version that no longer runs.
fn prune_run(sites: &[Site], versions: &BTreeSet<String>) {
    let mut keep: BTreeSet<String> = versions
        .iter()
        .map(|v| format!("php-{v}-fpm.conf"))
        .collect();
    for s in sites {
        let p = pool(s);
        if let Some(v) = &s.php {
            keep.insert(format!("php-{v}-{p}-fpm.conf"));
            keep.insert(format!("php-{v}-{p}.d"));
        }
        keep.insert(format!("swoole-{p}.d"));
        keep.insert(format!("frankenphp-{p}.d"));
        keep.insert(format!("octane-{p}.Caddyfile"));
    }
    let run = home().join("run");
    for e in fs::read_dir(&run).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        // A version's own config too: the Database viewer's, once it's off.
        let ours = (name.starts_with("php-")
            && (name.ends_with("-fpm.conf") || name.contains("-site-") && name.ends_with(".d")))
            || ((name.starts_with("swoole-site-") || name.starts_with("frankenphp-site-"))
                && name.ends_with(".d"))
            || (name.starts_with("octane-site-") && name.ends_with(".Caddyfile"));
        if ours && !keep.contains(&name) {
            let path = e.path();
            let _ = if e.file_type().is_ok_and(|t| t.is_dir()) {
                fs::remove_dir_all(&path)
            } else {
                fs::remove_file(&path)
            };
        }
    }
}

pub struct Server {
    children: Vec<Child>,
    /// What each child is, parallel to `children`.
    labels: Vec<String>,
    /// Settings › Features › Mail catcher's SMTP listener, while it's on.
    mail: Option<mail::Listener>,
    /// Why the mail catcher isn't listening though it's on (its port is taken).
    pub mail_error: Option<String>,
    /// The browser bridge's proxy, while an app it serves has the switch on.
    browser: Option<browser::proxy::Proxy>,
}

impl Server {
    pub fn start(cfg: &Config) -> Result<Server> {
        kill_orphans(&pid_file(), false);
        if port_held(cfg.https_port) {
            bail!(
                "Port {} is already in use — is Anvil already running?",
                cfg.https_port
            );
        }
        ensure_caddy()?;
        let sites = sites(cfg);
        // A dev server that ended with an earlier Anvil may have left its address behind.
        for site in sites.iter().filter(|s| apps::is_php(&s.kind)) {
            apps::clear_vite_hot(&site.path);
        }
        let versions = php_versions(cfg, &sites);
        for v in &versions {
            if !php_dir(v).join(PHP_SERVER).exists() {
                bail!("PHP {v} is used but not installed — run `anvil php install {v}`");
            }
        }
        state_dirs()?;
        prune_run(&sites, &versions);
        // PHP and Caddy read them from the start; only while an app's bridge is on.
        if browser::any(&sites) {
            browser::write_files()?;
        }

        let mut server = Server {
            children: vec![],
            labels: vec![],
            mail: None,
            mail_error: None,
            browser: None,
        };
        for v in versions.iter().filter(|_| cfg!(windows)) {
            server.start_cgi(cfg, v, &sites)?;
        }
        for v in versions.iter().filter(|_| !cfg!(windows)) {
            for site in &sites {
                if let Some(sock) = fpm_socket(site) {
                    let _ = fs::remove_file(sock);
                }
            }
            let _ = fs::remove_file(home().join("run").join(format!("php-{v}.sock")));
            let conf = home().join("run").join(format!("php-{v}-fpm.conf"));
            fs::write(&conf, fpm_conf(cfg, v, &sites))?;
            // Started here, not by a PHP: `-d`s, no scan dir (`phpext::scan_env`).
            let fpm = |conf: &Path, site: Option<&Site>| {
                crate::platform::command(php_dir(v).join("php-fpm"))
                    .arg("-y")
                    .arg(conf)
                    .arg("-c")
                    .arg(php_dir(v).join("php.ini"))
                    .args(phpext::php_args_with(cfg, v, site))
                    .spawn()
                    .with_context(|| format!("failed to start PHP {v}"))
            };
            server.labels.push(format!("PHP {v} FPM"));
            server.children.push(fpm(&conf, None)?);
            for site in sites
                .iter()
                .filter(|s| s.own_master && s.php.as_deref() == Some(v))
            {
                let Some(text) = fpm_site_conf(cfg, v, site, &sites) else {
                    continue;
                };
                let conf = fpm_site_conf_file(v, site);
                fs::write(&conf, text)?;
                server.labels.push(format!("PHP {v} FPM ({})", site.name));
                server.children.push(fpm(&conf, Some(site))?);
            }
        }

        fs::write(
            home().join("run").join("php-masters"),
            php_masters(cfg, &sites),
        )?;
        // Listening before Caddy takes the first page load. Without it those apps' pages
        // get Caddy's 502 handling; the rest of the web server runs.
        if browser::proxied(cfg, &sites) {
            match browser::proxy::Proxy::start() {
                Ok(p) => server.browser = Some(p),
                Err(e) => eprintln!("Anvil: the browser bridge can't listen: {e}"),
            }
        }
        let file = home().join("run").join("Caddyfile");
        fs::write(&file, caddyfile(cfg, &sites))?;
        let log = fs::File::create(caddy_log())?;
        server.labels.push("Caddy".into());
        server.children.push(
            caddy_cmd()
                .args(["run", "--adapter", "caddyfile", "--config"])
                .arg(&file)
                .stderr(log)
                .spawn()?,
        );

        // Each child with its start time, so a later Anvil never mistakes another
        // process that got the pid for one of these (`kill_orphans`).
        let pids: Vec<String> = std::iter::once(owner_line())
            .chain(server.pids().into_iter().map(|(_, p)| {
                format!("{p} {}", started(p).unwrap_or_default())
                    .trim_end()
                    .to_string()
            }))
            .collect();
        fs::write(pid_file(), pids.join("\n"))?;

        // Adminer again after an upgrade pinned another version or its folder was cleaned;
        // the web server runs without it.
        if cfg.db_viewer {
            if let Err(e) = dbviewer::ensure() {
                eprintln!("Anvil: the Database viewer couldn't download Adminer: {e:#}");
            }
            let _ = dbviewer::write_allowed(cfg, &sites);
        }
        // The web server runs without it: the error shows in Settings and the Mail tab.
        if cfg.mail_catcher {
            let apps = sites.iter().map(|s| s.name.clone()).collect();
            match mail::listen(mail::port(cfg), mail::root(), apps) {
                Ok(l) => server.mail = Some(l),
                Err(e) => server.mail_error = Some(format!("{e:#}")),
            }
        }

        // ponytail: the DNS port is bound once per process; changing dns_port needs a restart of the process.
        *DNS_TLD.write().unwrap_or_else(|e| e.into_inner()) = cfg.tld.clone();
        // .localhost needs no DNS; switching to another suffix later starts it then.
        if !needs_resolver(cfg) {
            return Ok(server);
        }
        let port = cfg.dns_port;
        DNS.call_once(|| {
            // Bound here, so a taken port is reported at once rather than lost in a thread.
            let sock = match std::net::UdpSocket::bind(("127.0.0.1", port)) {
                Ok(sock) => sock,
                Err(e) => return dns_failed(format!("The DNS server can't use port {port}: {e}")),
            };
            std::thread::spawn(move || {
                let run = std::panic::catch_unwind(|| dns::serve(&sock, &DNS_TLD));
                dns_failed(match run {
                    Ok(e) => format!("The DNS server stopped: {e}"),
                    Err(_) => "The DNS server stopped unexpectedly.".into(),
                });
            });
        });
        Ok(server)
    }

    /// Windows: the php-cgi processes of version `v`'s pools, `CGI_WORKERS` per pool on
    /// its `cgi_ports`: the shared one with the global php.ini values, then one per site
    /// with values, extensions or a worktree's environment of its own.
    fn start_cgi(&mut self, cfg: &Config, v: &str, sites: &[Site]) -> Result<()> {
        let mut pools = vec![(format!("php-{v}"), cfg.ini.clone(), vec![], None)];
        for site in sites
            .iter()
            .filter(|s| s.php.as_deref() == Some(v) && own_pool(s))
        {
            // As with FPM: a worktree Anvil can't separate from main gets no pool.
            let Ok(env) = worktrees::web_env(cfg, site) else {
                continue;
            };
            let mut ini = cfg.ini.clone();
            ini.extend(site.ini.clone());
            pools.push((pool(site), ini, env, Some(site)));
        }
        let log = php_fpm_log(v);
        for (name, ini, mut env, site) in pools {
            // As FPM's `php_values`: Anvil's prepend for every pool while any app's bridge is on.
            let (ini, theirs) = browser::pool_ini(sites, ini);
            env.extend(theirs.map(|p| ("ANVIL_PREPEND".to_string(), p)));
            for port in cgi_ports(&name) {
                let err = fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&log)?;
                let mut cmd = platform::command(php_dir(v).join(PHP_SERVER));
                cmd.arg("-b")
                    .arg(format!("127.0.0.1:{port}"))
                    .arg("-c")
                    .arg(php_dir(v).join("php.ini"))
                    .args(phpext::php_args_with(cfg, v, site));
                for (k, val) in &ini {
                    cmd.arg("-d").arg(format!("{k}={val}"));
                }
                // 0: no request limit, so a worker never leaves its port unanswered.
                cmd.env("PHP_FCGI_MAX_REQUESTS", "0")
                    .envs(env.iter().map(|(k, v)| (k, v)))
                    .stdout(std::process::Stdio::null())
                    .stderr(err);
                self.labels.push(format!("PHP {v} ({name}, port {port})"));
                self.children.push(
                    platform::no_window(&mut cmd)
                        .spawn()
                        .with_context(|| format!("failed to start PHP {v}"))?,
                );
            }
        }
        Ok(())
    }

    /// Each child's label (`Caddy`, `PHP 8.4 FPM`) and pid; Caddy is last.
    pub fn pids(&self) -> Vec<(String, u32)> {
        self.labels
            .iter()
            .cloned()
            .zip(self.children.iter().map(Child::id))
            .collect()
    }

    /// Why the server is down when one of its children ended on its own (a crash, a bad
    /// Caddyfile, a port taken). That child is reaped and dropped from the list, so
    /// stopping the rest never signals its pid again.
    pub fn exited(&mut self) -> Option<String> {
        let i = self
            .children
            .iter_mut()
            .position(|c| !matches!(c.try_wait(), Ok(None)))?;
        let mut child = self.children.remove(i);
        let label = self.labels.remove(i);
        let how = match child.try_wait() {
            Ok(Some(st)) => format!(" ({st})"),
            _ => String::new(),
        };
        let log = if label == "Caddy" {
            format!(" — see {}", caddy_log().display())
        } else {
            String::new()
        };
        Some(format!("{label} stopped unexpectedly{how}{log}"))
    }

    /// Blocks until the web server exits.
    pub fn wait(&mut self) -> Result<()> {
        self.children.last_mut().unwrap().wait()?;
        Ok(())
    }

    pub fn stop(&mut self) {
        stop_all(self.take_all(), std::time::Duration::from_secs(3));
    }

    /// Hand back every process to stop with `stop_all`; the pid file goes with the
    /// first, the pid it names.
    pub fn take_all(&mut self) -> Vec<Stopping> {
        self.labels.clear();
        self.mail = None;
        self.browser = None;
        let mut files = vec![pid_file()];
        self.children
            .drain(..)
            .map(|child| Stopping {
                child,
                dir: None,
                pid_files: std::mem::take(&mut files),
            })
            .collect()
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

    /// The Caddyfile without its access logs (one test covers those), so the others read
    /// each block's directives as they follow its address.
    fn caddyfile(cfg: &Config, sites: &[Site]) -> String {
        let mut c = super::caddyfile(cfg, sites);
        while let Some(at) = c.find("\tlog {\n") {
            let end = "\t\tformat json\n\t}\n";
            let to = at + c[at..].find(end).unwrap() + end.len();
            c.replace_range(at..to, "");
        }
        c
    }

    /// A group leader is stopped with its group (a grandchild that ignores nothing
    /// dies too); a plain child alone, and one that ignores SIGTERM is killed.
    #[test]
    fn terminate_stops_a_group_or_a_single_child() {
        use std::os::unix::process::CommandExt;
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("grandchild");
        let mut leader = crate::platform::command("sh")
            .arg("-c")
            .arg(format!(
                "sleep 30 & echo $! > '{}'; wait",
                pid_file.display()
            ))
            .process_group(0)
            .spawn()
            .unwrap();
        let grandchild = (0..100)
            .find_map(|_| {
                std::thread::sleep(std::time::Duration::from_millis(20));
                fs::read_to_string(&pid_file)
                    .ok()
                    .filter(|s| s.ends_with('\n'))
            })
            .unwrap();
        let grandchild = grandchild.trim().to_string();
        assert!(alive(&grandchild));
        terminate(&mut leader, std::time::Duration::from_secs(5));
        assert!(matches!(leader.try_wait(), Ok(Some(_))));
        // The orphaned sleep is reaped by launchd; give it a moment.
        let gone = (0..50).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(20));
            !alive(&grandchild)
        });
        assert!(gone, "the group's other process survived");

        let start = std::time::Instant::now();
        let mut stubborn = crate::platform::command("sh")
            .args(["-c", "trap '' TERM; sleep 30"])
            .spawn()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));
        terminate(&mut stubborn, std::time::Duration::from_millis(300));
        assert!(matches!(stubborn.try_wait(), Ok(Some(_))));
        assert!(start.elapsed() < std::time::Duration::from_secs(5));
    }

    /// A process that left the app's group (Next's telemetry flusher starts in its own
    /// session) is stopped with it; it would outlive the group's kill.
    #[test]
    fn terminate_stops_what_left_the_group() {
        use std::os::unix::process::CommandExt;
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("flusher");
        let mut leader = crate::platform::command("sh")
            .arg("-c")
            .arg(format!(
                "perl -e 'setpgrp(0,0); exec @ARGV' sleep 30 & echo $! > '{}'; wait",
                pid_file.display()
            ))
            .process_group(0)
            .spawn()
            .unwrap();
        let flusher = (0..100)
            .find_map(|_| {
                std::thread::sleep(std::time::Duration::from_millis(20));
                fs::read_to_string(&pid_file)
                    .ok()
                    .filter(|s| s.ends_with('\n'))
            })
            .unwrap();
        let flusher: u32 = flusher.trim().parse().unwrap();
        // Out of the group once perl has run setpgrp.
        let left = (0..100).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(20));
            platform::group_alive(flusher)
        });
        assert!(left, "the test's process never left the group");
        terminate(&mut leader, std::time::Duration::from_secs(5));
        let gone = (0..50).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(20));
            !platform::alive(flusher)
        });
        if !gone {
            platform::kill(flusher, false);
        }
        assert!(gone, "the process that left the group survived");
    }

    /// Quit stops everything against one deadline: three groups that ignore SIGTERM and
    /// three that don't take one grace in total, not three, and none is left behind.
    #[test]
    fn stop_all_shares_one_deadline() {
        use std::os::unix::process::CommandExt;
        let spawn = |script: &str| {
            crate::platform::command("sh")
                .args(["-c", script])
                .process_group(0)
                .spawn()
                .unwrap()
        };
        let mut children: Vec<Child> = (0..3)
            .map(|_| spawn("trap '' TERM; sleep 30"))
            .chain((0..3).map(|_| spawn("sleep 30")))
            .collect();
        let pids: Vec<i32> = children.iter().map(|c| c.id() as i32).collect();
        std::thread::sleep(std::time::Duration::from_millis(200)); // let the traps be set
        let grace = std::time::Duration::from_secs(1);
        let start = std::time::Instant::now();
        terminate_all(&mut children, grace);
        let took = start.elapsed();
        eprintln!("stop_all: 6 groups, {grace:?} grace, took {took:?}");
        assert!(
            took >= grace,
            "the stubborn ones were killed early: {took:?}"
        );
        assert!(took < grace * 2, "not one shared deadline: {took:?}");
        for (c, pid) in children.iter_mut().zip(pids) {
            assert!(matches!(c.try_wait(), Ok(Some(_))));
            // The whole group is gone, its sleep too (launchd reaps it; give it a moment).
            let gone = (0..50).any(|_| {
                std::thread::sleep(std::time::Duration::from_millis(20));
                !platform::group_alive(pid as u32)
            });
            assert!(gone, "group {pid} survived");
        }
    }

    #[test]
    fn ports_paths_and_homes() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let p = l.local_addr().unwrap().port();
        assert!(port_answers(p) && port_held(p));
        assert_ne!(free_port(&[], p..p.saturating_add(50)), Some(p));
        assert_eq!(free_port(&[p + 1], p + 1..p + 2), None);
        drop(l);
        assert_eq!(
            prepend_path(Path::new("/a b"), "/usr/bin:/bin"),
            "/a b:/usr/bin:/bin"
        );
        assert!(user_home().is_absolute());
        let e = curl(&["-fsS", "--max-time", "2"], "http://127.0.0.1:1/").unwrap_err();
        assert!(
            e.to_string()
                .starts_with("couldn't fetch http://127.0.0.1:1/: "),
            "{e}"
        );
    }

    /// The admin path without the password dialog: quotes, `$`, backslashes and
    /// newlines in the script and the prompt arrive untouched.
    #[test]
    #[cfg(target_os = "macos")] // osascript / plutil
    fn a_setup_script_runs_from_its_file_as_written() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("it's \"out\"");
        let script = format!(
            "printf 'a\\\\b \"$X\"\\n' > {}\nprintf 'line2\\n' >> {}\n",
            shims::sh_quote(&out.to_string_lossy()),
            shims::sh_quote(&out.to_string_lossy())
        );
        let plain = ADMIN_SH.replace(" with administrator privileges", "");
        let plain = plain.replace(" with prompt (item 2 of argv)", "");
        run_sh_file(&script, "Anvil \"sets\" up", &plain).unwrap();
        assert_eq!(fs::read_to_string(&out).unwrap(), "a\\b \"$X\"\nline2\n");
        assert!(run_sh_file("exit 3", "x", &plain).is_err());
    }

    #[test]
    fn a_child_that_ends_is_noticed_and_dropped() {
        let spawn = |cmd: &str| {
            crate::platform::command("/bin/sh")
                .args(["-c", cmd])
                .spawn()
                .unwrap()
        };
        let mut server = Server {
            children: vec![spawn("sleep 30"), spawn("exit 3")],
            labels: vec!["PHP 8.4 FPM".into(), "Caddy".into()],
            mail: None,
            mail_error: None,
            browser: None,
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let why = loop {
            if let Some(why) = server.exited() {
                break why;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the exit went unnoticed"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        assert!(
            why.starts_with("Caddy stopped unexpectedly (exit status: 3)"),
            "{why}"
        );
        assert_eq!(server.labels, ["PHP 8.4 FPM"]);
        assert_eq!(server.exited(), None);
        // Stopped here, not through `stop`: that would remove the real run/pids.
        for mut c in server.children.drain(..) {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

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
            root_custom: None,
            root_detected: ".".into(),
            php: php.map(Into::into),
            php_pinned: false,
            ext: vec![],
            own_master: false,
            octane_ports: None,
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
            browser: false,
            packages_pinned: None,
            shared_deps: None,
            worktree_of: None,
            branch: None,
            aliases: vec![],
        }
    }

    #[test]
    fn a_web_root_is_a_folder_inside_the_project() {
        assert_eq!(web_root(" web/ ").unwrap(), "web");
        assert_eq!(web_root("./dist/app").unwrap(), "dist/app");
        assert_eq!(web_root(".").unwrap(), ".");
        for bad in [
            "",
            "/var/www",
            "~/site",
            "..",
            "../other",
            "web/../../x",
            "we\"b",
            "we\\b",
            "web\n}",
            "we`b",
            "{env.HOME}",
        ] {
            assert!(web_root(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_web_root_set_by_the_user_is_stored_relative_and_checked() {
        let dir = tempfile::tempdir().unwrap();
        let _env = crate::lock(&ENV_LOCK);
        // SAFETY: every test that sets ANVIL_HOME holds ENV_LOCK.
        unsafe { std::env::set_var("ANVIL_HOME", dir.path().join("home")) };
        let shop = dir.path().join("shop");
        fs::create_dir_all(shop.join("public")).unwrap();
        fs::create_dir_all(shop.join("web")).unwrap();
        fs::write(shop.join("index.php"), "").unwrap();
        let api = dir.path().join("api");
        fs::create_dir_all(api.join("web")).unwrap();
        fs::write(api.join("go.mod"), "module api\n").unwrap();
        let mut cfg = Config::default();
        cfg.links.insert("shop".into(), shop.clone());
        cfg.links.insert("api".into(), api);
        let get = |cfg: &Config| sites(cfg).into_iter().find(|s| s.name == "shop").unwrap();

        // Unset: public/ as before.
        let s = get(&cfg);
        assert_eq!((s.root.clone(), s.root_custom), (shop.join("public"), None));
        assert_eq!(s.root_detected, "public");

        set_site_root(&mut cfg, "shop", Some("./web/")).unwrap();
        assert_eq!(cfg.sites["shop"].root.as_deref(), Some("web"));
        assert_eq!(get(&cfg).root, shop.join("web"));
        set_site_root(&mut cfg, "shop", Some(".")).unwrap();
        assert_eq!(get(&cfg).root, shop);

        let err = set_site_root(&mut cfg, "shop", Some("dist")).unwrap_err();
        assert!(err.to_string().contains("dist isn't a folder"), "{err}");
        assert!(set_site_root(&mut cfg, "shop", Some("../api")).is_err());
        assert!(set_site_root(&mut cfg, "shop", Some(shop.to_str().unwrap())).is_err());
        assert_eq!(cfg.sites["shop"].root.as_deref(), Some("."));
        let err = set_site_root(&mut cfg, "api", Some("web")).unwrap_err();
        assert!(err.to_string().contains("no web root"), "{err}");

        set_site_root(&mut cfg, "shop", None).unwrap();
        assert_eq!(cfg.sites["shop"].root, None);
        assert_eq!(get(&cfg).root, shop.join("public"));
    }

    #[test]
    fn caddyfile_serves_a_set_web_root_and_says_when_it_is_gone() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config::default();
        let mut shop = site("shop", dir.path().to_str().unwrap(), Some("8.4"));
        shop.root = dir.path().join("web");
        shop.root_custom = Some("web".into());
        let out = caddyfile(&cfg, std::slice::from_ref(&shop));
        assert!(out.contains("shop's web root web doesn't exist"), "{out}");
        assert!(!out.contains("root *"), "never the project folder instead");

        fs::create_dir(dir.path().join("web")).unwrap();
        let out = caddyfile(&cfg, std::slice::from_ref(&shop));
        let want = format!("root * \"{}\"", dir.path().join("web").display());
        assert!(out.contains(&want), "{out}");

        // Edited by hand to leave the project: not served at all.
        shop.root_custom = Some("../web".into());
        assert!(!caddyfile(&cfg, &[shop]).contains("shop.test"));
    }

    #[test]
    fn a_worktree_serves_its_apps_web_root_inside_its_own_checkout() {
        let (root, mut cfg) = parked_worktree("web-root");
        for wt in ["shop", "shop--x"] {
            fs::create_dir_all(root.join("sites").join(wt).join("web")).unwrap();
        }
        cfg.sites.entry("shop".into()).or_default().root = Some("web".into());
        let all = sites(&cfg);
        let wt = all.iter().find(|s| s.name == "shop--x").unwrap();
        assert_eq!(wt.root, root.join("sites/shop--x/web"));
        assert_eq!(wt.root_custom.as_deref(), Some("web"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_worktree_pins_its_apps_packages_until_it_has_its_own() {
        let (root, mut cfg) = parked_worktree("pins");
        let pins = |cfg: &Config, n: &str| {
            sites(cfg)
                .into_iter()
                .find(|s| s.name == n)
                .unwrap()
                .packages_pinned
        };
        assert_eq!(pins(&cfg, "shop"), None, "unset: the defaults");
        set_packages_pinned(&mut cfg, "shop", Some(vec!["laravel/framework".into()])).unwrap();
        assert_eq!(
            pins(&cfg, "shop--x"),
            Some(vec!["laravel/framework".into()])
        );
        set_packages_pinned(&mut cfg, "shop--x", Some(vec![])).unwrap();
        assert_eq!(pins(&cfg, "shop--x"), Some(vec![]));
        set_packages_pinned(&mut cfg, "shop", None).unwrap();
        assert_eq!(cfg.sites["shop"].packages_pinned, None);
        assert_eq!(pins(&cfg, "shop--x"), Some(vec![]));
        assert!(set_packages_pinned(&mut cfg, "nope", None).is_err());
        assert!(!cfg.sites.contains_key("nope"));
        fs::remove_dir_all(root).unwrap();
    }

    /// A git repository at `dir` with one commit holding `index.php`.
    fn app_repo(dir: &Path) {
        use crate::worktrees::tests::git;
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("index.php"), "").unwrap();
        git(dir, &["init", "-q", "-b", "main"]);
        git(dir, &["add", "."]);
        git(dir, &["commit", "-q", "-m", "i"]);
    }

    fn add_worktree(main: &Path, at: &Path, branch: &str) {
        let at = at.to_str().unwrap();
        crate::worktrees::tests::git(main, &["worktree", "add", "-q", at, "-b", branch]);
    }

    #[test]
    fn run_keeps_generated_files_of_apps_that_are_there() {
        let _env = lock(&ENV_LOCK);
        let dir = tempfile::tempdir().unwrap();
        // SAFETY: every test that sets ANVIL_HOME holds ENV_LOCK.
        unsafe { std::env::set_var("ANVIL_HOME", dir.path()) };
        let run = dir.path().join("run");
        let files = [
            "php-8.4-site-shop-fpm.conf",
            "php-8.3-site-shop-fpm.conf",
            "php-8.4-site-gone-fpm.conf",
            "octane-site-shop.Caddyfile",
            "octane-site-gone.Caddyfile",
            "php-8.4-fpm.conf",
            "php-8.5-fpm.conf",
            "Caddyfile",
            "site-gone.sock",
        ];
        let dirs = [
            "php-8.4-site-shop.d",
            "php-8.4-site-gone.d",
            "swoole-site-gone.d",
            "frankenphp-site-shop.d",
            "frankenphp-site-gone.d",
        ];
        for d in dirs {
            fs::create_dir_all(run.join(d)).unwrap();
            fs::write(run.join(d).join("anvil.ini"), "").unwrap();
        }
        for f in files {
            fs::write(run.join(f), "").unwrap();
        }
        let versions = BTreeSet::from(["8.4".to_string()]);
        prune_run(&[site("shop", "/s/shop", Some("8.4"))], &versions);
        let mut left: Vec<String> = fs::read_dir(&run)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(
            left,
            [
                "Caddyfile",
                "frankenphp-site-shop.d",
                "octane-site-shop.Caddyfile",
                "php-8.4-fpm.conf",
                "php-8.4-site-shop-fpm.conf",
                "php-8.4-site-shop.d",
                "site-gone.sock",
            ]
        );
    }

    #[test]
    fn an_extension_changed_elsewhere_restarts_php() {
        let _env = lock(&ENV_LOCK);
        let dir = tempfile::tempdir().unwrap();
        // SAFETY: every test that sets ANVIL_HOME holds ENV_LOCK.
        unsafe { std::env::set_var("ANVIL_HOME", dir.path()) };
        phpext::tests::fake_php("8.4", "8.4.23");
        fs::write(php_dir("8.4").join(PHP_SERVER), "").unwrap();
        phpext::tests::fake_so("8.4", "8.4.23", "apcu");
        phpext::tests::fake_so("8.4", "8.4.23", "pcov");
        let app = dir.path().join("shop");
        fs::create_dir_all(&app).unwrap();
        fs::write(app.join("index.php"), "").unwrap();
        let mut cfg = Config {
            php: Some("8.4".into()),
            ..Default::default()
        };
        cfg.links.insert("shop".into(), app);
        // As `Server::start` leaves them.
        let served = |cfg: &Config| {
            state_dirs().unwrap();
            let run = home().join("run");
            fs::write(run.join("Caddyfile"), super::caddyfile(cfg, &sites(cfg))).unwrap();
            fs::write(run.join("php-masters"), php_masters(cfg, &sites(cfg))).unwrap();
        };
        served(&cfg);
        assert!(!needs_restart(&cfg));
        // `anvil php ext add apcu`: same Caddyfile, other PHP-FPM arguments.
        cfg.php_ext.insert("8.4".into(), vec!["apcu".into()]);
        assert!(needs_restart(&cfg));
        served(&cfg);
        // `anvil app ext shop add pcov` with apcu its own already: its own master's.
        cfg.sites.entry("shop".into()).or_default().ext = Some(vec!["apcu".into()]);
        served(&cfg);
        cfg.sites.get_mut("shop").unwrap().ext = Some(vec!["apcu".into(), "pcov".into()]);
        assert!(needs_restart(&cfg));
    }

    #[test]
    fn a_worktree_turns_off_an_extension_its_app_has() {
        let root = crate::worktrees::tests::tmp("ext");
        let shop = root.join("sites/shop");
        app_repo(&shop);
        add_worktree(&shop, &root.join("x"), "x");
        let _env = lock(&ENV_LOCK);
        let home_dir = tempfile::tempdir().unwrap();
        // SAFETY: every test that sets ANVIL_HOME holds ENV_LOCK.
        unsafe { std::env::set_var("ANVIL_HOME", home_dir.path()) };
        phpext::tests::fake_php("8.4", "8.4.23");
        fs::write(php_dir("8.4").join(PHP_SERVER), "").unwrap();
        phpext::tests::fake_so("8.4", "8.4.23", "xdebug");
        phpext::tests::fake_so("8.4", "8.4.23", "apcu");
        let mut cfg = Config {
            php: Some("8.4".into()),
            ..Default::default()
        };
        cfg.paths.push(root.join("sites"));
        cfg.sites.entry("shop".into()).or_default().ext = Some(vec!["xdebug".into()]);
        save(&cfg).unwrap();
        let ext = |cfg: &Config, n: &str| sites(cfg).into_iter().find(|s| s.name == n).unwrap().ext;
        assert_eq!(ext(&cfg, "shop--x"), ["xdebug"], "unset: its app's");

        // Off for the worktree: a list of its own, kept empty; its app keeps xdebug.
        let cfg = phpext::remove_site("shop--x", "xdebug").unwrap();
        assert_eq!(cfg.sites["shop--x"].ext, Some(vec![]));
        assert!(ext(&cfg, "shop--x").is_empty());
        assert_eq!(ext(&cfg, "shop"), ["xdebug"]);
        let text = fs::read_to_string(home().join("config.toml")).unwrap();
        assert!(text.contains("ext = []"), "{text}");
        assert_eq!(ext(&load().unwrap(), "shop--x"), Vec::<String>::new());

        // On again: its own list; the app's last one off leaves nothing stored.
        let cfg = phpext::add_site("shop--x", "apcu").unwrap();
        assert_eq!(ext(&cfg, "shop--x"), ["apcu"]);
        let cfg = phpext::remove_site("shop", "xdebug").unwrap();
        assert_eq!(cfg.sites["shop"].ext, None);
        assert_eq!(ext(&cfg, "shop--x"), ["apcu"]);
        // An older config's list reads as the site's own.
        let old: Config = toml::from_str("[sites.shop]\next = [\"xdebug\"]\n").unwrap();
        assert_eq!(old.sites["shop"].ext, Some(vec!["xdebug".into()]));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_worktree_is_a_site_named_app_double_dash_folder() {
        let root = crate::worktrees::tests::tmp("site");
        let shop = root.join("sites/shop");
        app_repo(&shop);
        add_worktree(&shop, &shop.join(".claude/worktrees/agent-1a2b"), "agent");
        let gone = root.join("gone");
        add_worktree(&shop, &gone, "gone");
        let mut cfg = Config::default();
        cfg.paths.push(root.join("sites"));
        let main = cfg.sites.entry("shop".into()).or_default();
        main.autostart = true;
        main.processes.insert(
            "queue".into(),
            ProcessConfig {
                command: Some("php artisan queue:work".into()),
                added: true,
                autostart: true,
                ..Default::default()
            },
        );
        let all = sites(&cfg);
        let w = all.iter().find(|s| s.name == "shop--agent-1a2b");
        let w = w.expect("the worktree is a site");
        assert_eq!(w.host, "shop--agent-1a2b.test");
        assert_eq!(w.worktree_of.as_deref(), Some("shop"));
        assert_eq!(w.branch.as_deref(), Some("agent"));
        assert!(!w.autostart);
        // The app's processes, never started with Anvil.
        let q = processes::detect_with(w, Some(&cfg))
            .into_iter()
            .find(|p| p.id == "queue")
            .expect("the app's own process");
        assert!(!q.autostart);
        assert!(all.iter().any(|s| s.name == "shop--gone"));
        // Deleted by hand: gone, though git still lists it until it is pruned.
        fs::remove_dir_all(&gone).unwrap();
        assert!(sites(&cfg).iter().all(|s| s.name != "shop--gone"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_parked_worktree_folder_is_listed_once_under_its_app() {
        let root = crate::worktrees::tests::tmp("parked");
        let shop = root.join("shop");
        app_repo(&shop);
        let wt = root.join("shop--feature-x");
        add_worktree(&shop, &wt, "feature-x");
        let mut cfg = Config::default();
        cfg.paths.push(root.clone());
        let all = sites(&cfg);
        assert_eq!(all.len(), 2);
        let at: Vec<_> = all.iter().filter(|s| s.path == wt).collect();
        assert_eq!(at.len(), 1);
        assert_eq!(at[0].name, "shop--feature-x");
        assert_eq!(at[0].worktree_of.as_deref(), Some("shop"));
        fs::remove_dir_all(root).unwrap();
    }

    /// `shop` parked in `<root>/sites` with a worktree `<root>/sites/shop--x`.
    fn parked_worktree(tag: &str) -> (PathBuf, Config) {
        let root = crate::worktrees::tests::tmp(tag);
        let shop = root.join("sites/shop");
        app_repo(&shop);
        add_worktree(&shop, &root.join("sites/shop--x"), "x");
        let mut cfg = Config::default();
        cfg.paths.push(root.join("sites"));
        (root, cfg)
    }

    #[test]
    fn without_git_a_parked_worktree_folder_is_still_its_apps() {
        let (root, _) = parked_worktree("no-git");
        // The check runs in a test process whose PATH has no git at all.
        let out = crate::platform::command(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "tests::no_git_child",
                "--ignored",
                "--test-threads=1",
            ])
            .env("PATH", "/nonexistent")
            .env("ANVIL_NO_GIT_ROOT", &root)
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success() && text.contains("1 passed"), "{text}");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "run by without_git_a_parked_worktree_folder_is_still_its_apps"]
    fn no_git_child() {
        assert!(
            crate::platform::command("git").output().is_err(),
            "git is gone"
        );
        let root = PathBuf::from(std::env::var_os("ANVIL_NO_GIT_ROOT").unwrap());
        let mut cfg = Config::default();
        cfg.paths.push(root.join("sites"));
        let all = sites(&cfg);
        let x = all.iter().find(|s| s.path.ends_with("shop--x")).unwrap();
        assert_eq!(x.worktree_of.as_deref(), Some("shop"));
        assert_eq!(x.branch.as_deref(), Some("x"));
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn a_hidden_apps_worktrees_are_hidden_with_it() {
        let (root, mut cfg) = parked_worktree("hidden-main");
        cfg.hidden.push("shop".into());
        assert!(sites(&cfg).is_empty(), "not a plain app on shop's database");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_link_to_a_worktree_folder_is_that_worktree() {
        let (root, mut cfg) = parked_worktree("link-wt");
        cfg.links.insert("demo".into(), root.join("sites/shop--x"));
        let all = sites(&cfg);
        assert_eq!(
            all.len(),
            2,
            "{:?}",
            all.iter().map(|s| &s.name).collect::<Vec<_>>()
        );
        let x = all.iter().find(|s| s.name == "shop--x").unwrap();
        assert_eq!(x.worktree_of.as_deref(), Some("shop"));
        assert_eq!(x.aliases, ["demo.test"]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_moved_worktree_of_an_app_is_not_an_app() {
        let (root, cfg) = parked_worktree("moved");
        // Moved by hand: git's record points at the old place, its `.git` still at shop.
        fs::rename(root.join("sites/shop--x"), root.join("sites/moved")).unwrap();
        let names: Vec<_> = sites(&cfg).into_iter().map(|s| s.name).collect();
        assert_eq!(names, ["shop"]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_long_worktree_name_is_capped_and_its_socket_fits() {
        let root = crate::worktrees::tests::tmp("long-name");
        let shop = root.join("sites/shop");
        app_repo(&shop);
        add_worktree(&shop, &root.join("sites").join("w".repeat(90)), "long");
        let mut cfg = Config::default();
        cfg.paths.push(root.join("sites"));
        let all = sites(&cfg);
        let w = all.iter().find(|s| s.worktree_of.is_some()).unwrap();
        assert_eq!(w.name.len(), 63);
        let mut php = w.clone();
        php.php = Some("8.4".into());
        let sock = fpm_socket(&php).unwrap();
        assert!(sock.as_os_str().len() <= 100, "{}", sock.display());
        assert_eq!(fpm_socket(&php), Some(sock), "stable");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_worktree_without_an_anvil_main_stays_an_app() {
        let root = crate::worktrees::tests::tmp("no-main");
        let shop = root.join("elsewhere/shop");
        app_repo(&shop);
        fs::create_dir(root.join("sites")).unwrap();
        add_worktree(&shop, &root.join("sites/shop--x"), "x");
        let mut cfg = Config::default();
        cfg.paths.push(root.join("sites"));
        let all = sites(&cfg);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name, "shop--x");
        assert_eq!(all[0].worktree_of, None);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn collisions_give_the_worktree_a_suffix() {
        let root = crate::worktrees::tests::tmp("clash");
        let shop = root.join("sites/shop");
        app_repo(&shop);
        fs::write(
            shop.join("package.json"),
            r#"{"scripts":{"dev":"vite","docs":"vitepress dev docs"}}"#,
        )
        .unwrap();
        fs::create_dir(root.join("sites/shop--x")).unwrap();
        add_worktree(&shop, &root.join("docs"), "docs");
        add_worktree(&shop, &root.join("x"), "x");
        let mut cfg = Config::default();
        cfg.paths.push(root.join("sites"));
        cfg.sites.entry("shop".into()).or_default().kind = Some("vite".into());
        let all = sites(&cfg);
        let of = |n: &str| {
            all.iter()
                .find(|s| s.name == n)
                .map(|s| s.worktree_of.clone())
        };
        assert_eq!(of("shop--docs-2"), Some(Some("shop".into())));
        assert_eq!(of("shop--docs"), None);
        assert_eq!(of("shop--x"), Some(None));
        assert_eq!(of("shop--x-2"), Some(Some("shop".into())));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn alias_addresses_are_derived() {
        let root = crate::worktrees::tests::tmp("alias");
        let shop = root.join("sites/shop");
        app_repo(&shop);
        add_worktree(&shop, &root.join("x"), "x");
        let mut cfg = Config::default();
        cfg.paths.push(root.join("sites"));
        cfg.links.insert("admin".into(), shop.clone());
        cfg.https_default = Some(true);
        let all = sites(&cfg);
        let wts: Vec<_> = all.iter().filter(|s| s.worktree_of.is_some()).collect();
        assert_eq!(wts.len(), 1, "an alias isn't another app");
        assert_eq!(wts[0].name, "shop--x");
        assert_eq!(wts[0].aliases, ["admin--x.test"]);
        assert!(caddyfile(&cfg, &all).contains("\nshop--x.test, admin--x.test {"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn derived_alias_addresses_yield_and_get_localhost() {
        let root = crate::worktrees::tests::tmp("alias-clash");
        let shop = root.join("sites/shop");
        app_repo(&shop);
        fs::write(
            shop.join("package.json"),
            r#"{"scripts":{"dev":"vite","docs":"vitepress dev docs"}}"#,
        )
        .unwrap();
        fs::create_dir(root.join("sites/admin--z")).unwrap();
        for w in ["x", "y", "z"] {
            add_worktree(&shop, &root.join(w), w);
        }
        let mut cfg = Config::default();
        cfg.paths.push(root.join("sites"));
        cfg.links.insert("admin".into(), shop.clone());
        cfg.sites.entry("shop".into()).or_default().kind = Some("vite".into());
        let admin = cfg.sites.entry("admin".into()).or_default();
        admin.kind = Some("vite".into());
        // The alias's docs server answers at admin--x.
        admin.processes.insert(
            "docs".into(),
            ProcessConfig {
                host: Some("x".into()),
                ..Default::default()
            },
        );
        let all = sites(&cfg);
        let aliases = |n: &str| all.iter().find(|s| s.name == n).unwrap().aliases.clone();
        assert!(aliases("shop--x").is_empty(), "a dev server keeps admin--x");
        assert!(
            aliases("shop--z").is_empty(),
            "a separate app keeps admin--z"
        );
        assert_eq!(aliases("shop--y"), ["admin--y.test"]);
        let c = caddyfile(&cfg, &all);
        let blocks = c
            .lines()
            .filter(|l| l.ends_with('{') && l.contains("admin--x.test"));
        assert_eq!(blocks.count(), 1, "{c}");
        assert!(c.contains("http://admin--y.localhost"), "{c}");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_worktree_pins_its_app_pin_then_its_anvil_toml() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("index.php"), "").unwrap();
        fs::write(dir.path().join(".anvil.toml"), "php = \"8.4\"\n").unwrap();
        let php = |main_pin: Option<&str>, own_pin: Option<&str>, installed: &[&str]| {
            let mut cfg = Config {
                php: Some("8.2".into()),
                ..Default::default()
            };
            cfg.sites.entry("shop".into()).or_default().php = main_pin.map(Into::into);
            if let Some(p) = own_pin {
                cfg.sites.entry("shop--x".into()).or_default().php = Some(p.into());
            }
            let own = worktree_settings(&cfg, "shop", "shop--x");
            let installed: Vec<String> = installed.iter().map(|v| v.to_string()).collect();
            let path = dir.path().to_path_buf();
            site_at(&cfg, &installed, "shop--x".into(), path, None, Some(&own)).php
        };
        let all = ["8.2", "8.3", "8.4", "8.5"];
        assert_eq!(php(Some("8.3"), None, &all).as_deref(), Some("8.3"));
        assert_eq!(php(Some("8.3"), Some("8.5"), &all).as_deref(), Some("8.5"));
        assert_eq!(php(None, None, &all).as_deref(), Some("8.4"));
        // The file's version isn't installed: the app's pin, else the default.
        assert_eq!(
            php(Some("8.3"), None, &["8.2", "8.3"]).as_deref(),
            Some("8.3")
        );
        assert_eq!(php(None, None, &["8.2", "8.3"]).as_deref(), Some("8.2"));
    }

    /// A version apps are pinned to stays until they are unpinned, for every language.
    #[test]
    fn uninstall_refuses_a_version_apps_are_pinned_to() {
        let dir = tempfile::tempdir().unwrap();
        let _env = crate::lock(&ENV_LOCK);
        // SAFETY: every test that sets ANVIL_HOME holds ENV_LOCK.
        unsafe { std::env::set_var("ANVIL_HOME", dir.path().join("home")) };
        let mut cfg = Config::default();
        for (name, file, pin) in [
            ("shop", "go.mod", Some("1.25.1")),
            ("blog", "go.mod", Some("1.25.1")),
            ("notes", "go.mod", None),
            ("web", "package.json", Some("22")),
        ] {
            let app = dir.path().join(name);
            fs::create_dir_all(&app).unwrap();
            fs::write(app.join(file), "{}").unwrap();
            cfg.links.insert(name.into(), app);
            cfg.sites.entry(name.into()).or_default().runtime = pin.map(Into::into);
        }
        assert_eq!(pinned_to(&cfg, "go", "1.25.1"), ["blog", "shop"]);
        assert_eq!(pinned_to(&cfg, "node", "22"), ["web"]);
        assert!(pinned_to(&cfg, "go", "22").is_empty());
        let err = runtime_uninstall(&cfg, "go", "1.25.1").unwrap_err();
        assert_eq!(
            err.to_string(),
            "blog, shop use Go 1.25.1 — switch them to another version first"
        );
        let err = runtime_uninstall(&cfg, "node", "22").unwrap_err();
        assert_eq!(
            err.to_string(),
            "web uses Node.js 22 — switch it to another version first"
        );
        // Not pinned: on to the uninstall itself.
        let err = runtime_uninstall(&cfg, "go", "1.24.0").unwrap_err();
        assert_eq!(err.to_string(), "Go 1.24.0 is not installed");

        cfg.sites.entry("shop".into()).or_default().php = Some("8.3".into());
        cfg.sites.entry("blog".into()).or_default().php = Some("8.3".into());
        assert_eq!(pinned_to(&cfg, "php", "8.3"), ["blog", "shop"]);
        let err = php_uninstall(&cfg, "8.3").unwrap_err();
        assert_eq!(
            err.to_string(),
            "blog, shop use PHP 8.3 — switch them to another version first"
        );
    }

    /// PHP 8.0 is end of life: install, default and pin say so, before any download.
    #[test]
    fn settings_for_an_unknown_app_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let _env = crate::lock(&ENV_LOCK);
        // SAFETY: every test that sets ANVIL_HOME holds ENV_LOCK.
        unsafe { std::env::set_var("ANVIL_HOME", dir.path().join("home")) };
        let mut cfg = Config::default();
        let no = "No app named ghost";
        assert_eq!(
            set_site_php(&mut cfg, "ghost", None)
                .unwrap_err()
                .to_string(),
            no
        );
        assert_eq!(
            set_site_ini(&mut cfg, "ghost", "memory_limit", Some("1G"))
                .unwrap_err()
                .to_string(),
            no
        );
        assert_eq!(
            set_secure(&mut cfg, "ghost", true).unwrap_err().to_string(),
            no
        );
        assert_eq!(
            set_site_app(&mut cfg, "ghost", None, None, None, Some(true))
                .unwrap_err()
                .to_string(),
            no
        );
        assert!(cfg.sites.is_empty());
    }

    #[test]
    fn php_newest_patch_comes_from_the_index() {
        let index = serde_json::json!({
            "8.6": {"version": "8.6.0RC4", "preview": true, "files": {
                "fpm-macos-aarch64": {"url": "u", "sha256": "s"}}},
            "8.4": {"version": "8.4.27", "preview": false, "files": {
                "fpm-macos-aarch64": {"url": "u", "sha256": "s"}}},
            "8.3": {"version": "8.3.30", "files": {
                "fpm-linux-x86_64": {"url": "u", "sha256": "s"}}},
            "8.0": {"version": "8.0.30", "files": {
                "fpm-macos-aarch64": {"url": "u", "sha256": "s"}}},
        });
        let got = releases_from(&index, "macos-aarch64");
        assert_eq!(got.len(), 2, "{got:?}");
        assert_eq!(got["8.6"], ("8.6.0RC4".into(), true));
        assert_eq!(got["8.4"], ("8.4.27".into(), false));
        // windows.php.net's releases.json: one build per version, no previews.
        let win = serde_json::json!({"8.4": {"version": "8.4.27"}});
        assert_eq!(releases_from(&win, "")["8.4"], ("8.4.27".into(), false));
        assert!(newer_patch("8.4.9", "8.4.27"));
        assert!(!newer_patch("8.4.27", "8.4.27"));
        assert!(!newer_patch("8.4.27", "8.4.9"));
        assert!(!newer_patch("8.4.27", "8.4.x"));
        assert!(newer_patch("8.6.0RC4", "8.6.0RC5"));
        assert!(newer_patch("8.6.0RC5", "8.6.0"));
        assert!(!newer_patch("8.6.0", "8.6.0RC5"));
    }

    #[test]
    fn php_8_0_is_not_offered() {
        assert_eq!(PHP_VERSIONS, ["8.6", "8.5", "8.4", "8.3", "8.2", "8.1"]);
        let dir = tempfile::tempdir().unwrap();
        let _env = crate::lock(&ENV_LOCK);
        // SAFETY: every test that sets ANVIL_HOME holds ENV_LOCK.
        unsafe { std::env::set_var("ANVIL_HOME", dir.path().join("home")) };
        // A folder left from before isn't an installed version, so templates don't offer it.
        fs::create_dir_all(php_dir("8.0")).unwrap();
        fs::write(php_dir("8.0").join("php-fpm"), "").unwrap();
        assert!(installed_php().is_empty());
        let eol = "PHP 8.0 is end of life — Anvil offers PHP 8.6, 8.5, 8.4, 8.3, 8.2, 8.1";
        let mut cfg = Config::default();
        assert_eq!(php_install(&mut cfg, "8.0").unwrap_err().to_string(), eol);
        assert_eq!(
            set_default_php(&mut cfg, "8.0").unwrap_err().to_string(),
            eol
        );
        let pin = set_site_php(&mut cfg, "shop", Some("8.0".into())).unwrap_err();
        assert_eq!(pin.to_string(), eol);
        assert_eq!(
            php_install(&mut cfg, "7.4").unwrap_err().to_string(),
            "PHP 7.4 isn't offered — Anvil offers PHP 8.6, 8.5, 8.4, 8.3, 8.2, 8.1"
        );
    }

    /// dotnet picks its SDK from global.json, so a .NET app has no version to pin.
    #[test]
    fn https_for_new_apps_leaves_existing_ones_as_they_are() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        cfg.links.insert("shop".into(), dir.path().to_path_buf());
        set_https_default(&mut cfg, true);
        assert_eq!(cfg.https_default, Some(true));
        assert!(!sites(&cfg).iter().any(|s| s.secure));
        cfg.links.insert("blog".into(), dir.path().to_path_buf());
        assert!(sites(&cfg).iter().any(|s| s.name == "blog" && s.secure));
    }

    #[test]
    fn a_dotnet_app_is_never_pinned() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("Api.csproj"), "").unwrap();
        let _env = crate::lock(&ENV_LOCK);
        // SAFETY: every test that sets ANVIL_HOME holds ENV_LOCK.
        unsafe { std::env::set_var("ANVIL_HOME", dir.path().join("home")) };
        let mut cfg = Config::default();
        cfg.links.insert("api".into(), dir.path().to_path_buf());
        let err = set_site_runtime(&mut cfg, "api", Some("10.0".into())).unwrap_err();
        assert!(err.to_string().contains("global.json"), "{err}");
        // A pin an older Anvil saved is not in effect.
        cfg.sites.entry("api".into()).or_default().runtime = Some("10.0".into());
        let own = cfg.sites.get("api");
        let site = site_at(&cfg, &[], "api".into(), dir.path().into(), None, own);
        assert!(!site.runtime_pinned);
        assert_ne!(site.runtime_version.as_deref(), Some("10.0"));
    }

    #[test]
    fn a_worktree_with_its_own_entry_still_follows_its_app() {
        let root = crate::worktrees::tests::tmp("own-entry");
        let shop = root.join("sites/shop");
        app_repo(&shop);
        add_worktree(&shop, &root.join("x"), "x");
        let mut cfg = Config::default();
        cfg.paths.push(root.join("sites"));
        let main = cfg.sites.entry("shop".into()).or_default();
        main.autostart = true;
        main.php = Some("8.3".into());
        main.processes.insert(
            "queue".into(),
            ProcessConfig {
                command: Some("php artisan queue:work".into()),
                added: true,
                autostart: true,
                ..Default::default()
            },
        );
        let own = cfg.sites.entry("shop--x".into()).or_default();
        own.https = Some(true);
        own.autostart = true;
        let all = sites(&cfg);
        let w = all.iter().find(|s| s.name == "shop--x").unwrap();
        assert!(w.secure && !w.autostart);
        let own = worktree_settings(&cfg, "shop", "shop--x");
        assert_eq!(own.php.as_deref(), Some("8.3"));
        let q = processes::detect_with(w, Some(&cfg))
            .into_iter()
            .find(|p| p.id == "queue")
            .expect("the app's own process");
        assert!(!q.autostart);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn odd_paths_are_quoted_or_skipped() {
        use crate::worktrees::worktree_name;
        assert_eq!(worktree_name("my-app", Path::new("/tmp/my app--x")), "x");
        assert_eq!(
            worktree_name("shop", Path::new("/s/shop--feature-x")),
            "feature-x"
        );
        assert_eq!(
            worktree_name("shop", Path::new("/s/fix--typo")),
            "fix--typo"
        );
        assert_eq!(
            worktree_name("my--app", Path::new("/s/my--app--feat")),
            "feat"
        );
        assert_eq!(worktree_name("shop", Path::new("/s/shop--")), "shop--");
        assert_eq!(
            worktree_name("shop", Path::new("/s/.claude/worktrees/agent-1a2b")),
            "agent-1a2b"
        );
        // A real worktree (in a folder with a space) whose main has no `.env`.
        let (_, x) = worktrees::tests::repo("my app");
        let mut w = site("m--x", x.to_str().unwrap(), None);
        w.worktree_of = Some("m".into());
        let mut bad = site("m--y", "/tmp/a\"b", None);
        bad.worktree_of = Some("m".into());
        let cfg = Config::default();
        let c = caddyfile(&cfg, &[w, bad]);
        assert!(
            c.contains(&format!("m--x.test {{\n\troot * \"{}\"", x.display())),
            "{c}"
        );
        assert!(!c.contains("m--y"));
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
        // WordPress's latest.tar.gz.sha1: a bare SHA-1.
        let sha1 = "491a78c7676b15dffcc69130c6054b8f49228a91";
        assert_eq!(sum_in(sha1, "latest.tar.gz").unwrap(), sha1);
        let redis = format!(
            "hash redis-8.10.2.tar.gz sha256 {h} http://download.redis.io/releases/redis-8.10.2.tar.gz\n"
        );
        assert_eq!(sum_in(&redis, "redis-8.10.2.tar.gz").unwrap(), h);
        assert_eq!(sum_in("<html>not found</html>", "composer.phar"), None);
        // certutil's form, as theseus-rs publishes it for Windows.
        let win = format!(
            "SHA256 hash of postgresql-18.6.0-x86_64-pc-windows-msvc.zip:\r\n{h}\r\nCertUtil: -hashfile command completed successfully.\r\n"
        );
        assert_eq!(
            sum_in(&win, "postgresql-18.6.0-x86_64-pc-windows-msvc.zip").unwrap(),
            h
        );
    }

    /// A PHP build is unpacked only when it matches the index's SHA-256.
    #[cfg(unix)]
    #[test]
    fn a_download_with_the_wrong_sum_is_not_unpacked() {
        let dir = tempfile::tempdir().unwrap();
        let _env = crate::lock(&ENV_LOCK);
        // SAFETY: every test that sets ANVIL_HOME holds ENV_LOCK.
        unsafe { std::env::set_var("ANVIL_HOME", dir.path().join("home")) };
        fs::write(dir.path().join("php"), "php").unwrap();
        let tgz = dir.path().join("php.tar.gz");
        run(Command::new("tar")
            .arg("-czf")
            .arg(&tgz)
            .arg("-C")
            .arg(dir.path())
            .arg("php"))
        .unwrap();
        let url = format!("file://{}", tgz.display());
        let out = dir.path().join("out");
        let wrong = "0".repeat(64);
        assert!(fetch_tgz(&url, &out, false, Some(&wrong)).is_err());
        assert!(!out.join("php").exists());
        let out_sum = Command::new("openssl")
            .args(["dgst", "-sha256", "-r"])
            .arg(&tgz)
            .output()
            .unwrap();
        let sum = String::from_utf8_lossy(&out_sum.stdout)
            .split_whitespace()
            .next()
            .unwrap()
            .to_string();
        fetch_tgz(&url, &out, false, Some(&sum)).unwrap();
        assert_eq!(fs::read_to_string(out.join("php")).unwrap(), "php");
    }

    #[test]
    fn verify_compares_digests() {
        let f = std::env::temp_dir().join(format!("anvil-verify-{}", std::process::id()));
        fs::write(&f, "hello\n").unwrap();
        let sha256 = "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03";
        verify(&f, sha256, "hello").unwrap();
        verify(&f, &sha256.to_uppercase(), "hello").unwrap();
        verify(&f, "b1946ac92492d2347c6235b4d2611184", "hello").unwrap(); // md5
        verify(&f, "f572d396fae9206628714fb2ce00f72e94f2258f", "hello").unwrap(); // sha1
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
        let _env = crate::lock(&ENV_LOCK);
        // SAFETY: every test that sets ANVIL_HOME holds ENV_LOCK.
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
        let hidden = hidden_sites(&cfg);
        assert_eq!(hidden.len(), 1);
        assert_eq!(hidden[0].name, "shop");
        unhide(&mut cfg, "shop").unwrap();
        assert!(hidden_sites(&cfg).is_empty());
        assert!(sites(&cfg).iter().any(|s| s.name == "shop"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn own_master_for_every_site_on_that_path() {
        let _env = lock(&ENV_LOCK);
        let dir = tempfile::tempdir().unwrap();
        // SAFETY: every test that sets ANVIL_HOME holds ENV_LOCK.
        unsafe { std::env::set_var("ANVIL_HOME", dir.path()) };
        let app = dir.path().join("shop");
        fs::create_dir_all(&app).unwrap();
        let mut cfg = Config::default();
        cfg.links.insert("shop".into(), app.clone());
        cfg.links.insert("alias".into(), app.clone());
        cfg.links.insert("blog".into(), dir.path().into());
        cfg.sites.entry("shop".into()).or_default().ext = Some(vec!["xdebug".into()]);
        let all = sites(&cfg);
        let master = |n: &str| all.iter().find(|s| s.name == n).unwrap().own_master;
        assert!(master("shop") && master("alias"), "same folder, same rule");
        assert!(!master("blog"));

        // Neither is in the version's master; each has one of its own.
        let mut sites = [
            site("shop", "/s/shop", Some("8.4")),
            site("alias", "/s/shop", Some("8.4")),
            site("blog", "/s/blog", Some("8.4")),
        ];
        sites[0].ext = vec!["xdebug".into()];
        mark_own_masters(&mut sites);
        assert!(sites[1].own_master && !sites[2].own_master);
        let conf = fpm_conf(&cfg, "8.4", &sites);
        assert!(conf.contains("[www]"));
        assert!(
            !conf.contains("[site-shop]") && !conf.contains("[site-alias]"),
            "{conf}"
        );
        let sock = fpm_socket(&sites[1]).unwrap();
        assert!(sock.ends_with("site-alias.sock"), "{sock:?}");
    }

    #[test]
    fn site_conf_has_only_its_pool() {
        let _env = lock(&ENV_LOCK);
        let dir = tempfile::tempdir().unwrap();
        // SAFETY: every test that sets ANVIL_HOME holds ENV_LOCK.
        unsafe { std::env::set_var("ANVIL_HOME", dir.path()) };
        let mut cfg = Config::default();
        cfg.ini.insert("memory_limit".into(), "1G".into());
        let mut shop = site("shop", "/s/shop", Some("8.4"));
        shop.ext = vec!["xdebug".into()];
        shop.ini.insert("display_errors".into(), "On".into());
        let mut sites = [shop, site("blog", "/s/blog", Some("8.4"))];
        mark_own_masters(&mut sites);
        let conf = fpm_site_conf(&cfg, "8.4", &sites[0], &sites).unwrap();
        assert_eq!(conf.matches("\n[").count(), 1, "one pool: {conf}");
        assert!(conf.starts_with("[global]\n"));
        assert!(conf.contains("\n[site-shop]\n"));
        assert!(conf.contains(&format!(
            "listen = {}",
            fpm_socket(&sites[0]).unwrap().display()
        )));
        assert!(conf.contains("php_value[memory_limit] = 1G"));
        assert!(conf.contains("php_value[display_errors] = On"));
        assert!(!conf.contains("[www]") && !conf.contains("[site-blog]"));
        assert!(fpm_site_conf_file("8.4", &sites[0]).ends_with("run/php-8.4-site-shop-fpm.conf"));
    }

    #[test]
    fn fpm_gives_a_worktree_its_own_pool_with_env() {
        // Its socket's path is in ANVIL_HOME.
        let _env = lock(&ENV_LOCK);
        let (cfg, wt) = crate::worktrees::tests::worktree_site(
            "fpm",
            "DB_CONNECTION=mysql\nDB_DATABASE=shop\n",
        );
        let sites = [site("shop", "/s/shop", Some("8.4")), wt.clone()];
        let conf = fpm_conf(&cfg, "8.4", &sites);
        let own = &conf[conf.find("[site-shop--feature-x]").expect("its own pool")..];
        assert!(
            own.contains("env[DB_DATABASE] = \"shop_feature_x\"\n"),
            "{own}"
        );
        assert!(own.contains("env[REDIS_PREFIX] = \"shop_feature_x_\"\n"));
        assert!(
            !conf.contains("site-shop]"),
            "the main app stays on the shared pool"
        );
        let sock = fpm_socket(&wt).unwrap();
        assert!(sock.ends_with("site-shop--feature-x.sock"));
        assert_ne!(Some(sock), fpm_socket(&sites[0]));
    }

    #[test]
    fn a_worktree_without_its_own_database_gets_no_pool_and_an_error_page() {
        let (cfg, wt) =
            crate::worktrees::tests::worktree_site("fpm-refuse", "DATABASE_URL=redis://h/0\n");
        let sites = [wt];
        assert!(!fpm_conf(&cfg, "8.4", &sites).contains("[site-shop--feature-x]"));
        let caddy = caddyfile(&cfg, &sites);
        let block = &caddy[caddy.find("http://shop--feature-x.test").expect("its host")..];
        assert!(!block.contains("php_fastcgi"), "{block}");
        assert!(block.contains("DATABASE_URL uses a kind of database Anvil can't rewrite"));
        assert!(block.contains("respond `"));
    }

    #[test]
    fn a_worktree_gets_no_pool_until_its_database_is_made() {
        // Its name may be there already, not Anvil's (kept after a Remove, say).
        let (cfg, wt) = crate::worktrees::tests::worktree_site(
            "fpm-pending",
            "DB_CONNECTION=pgsql\nDB_PORT=5433\nDB_DATABASE=shop\n",
        );
        let mut cfg = crate::worktrees::tests::with_postgres(cfg);
        let sites = [wt.clone()];
        assert!(!fpm_conf(&cfg, "8.4", &sites).contains("[site-shop--feature-x]"));
        let caddy = caddyfile(&cfg, &sites);
        let block = &caddy[caddy.find("http://shop--feature-x.test").expect("its host")..];
        assert!(!block.contains("php_fastcgi"), "{block}");
        assert!(block.contains("shop_feature_x isn't made yet"), "{block}");
        // Made (and recorded): served, on the database recorded.
        cfg.worktrees.entry(wt.name.clone()).or_default().copies =
            vec![("postgres-x".into(), "shop_x_older".into())];
        let conf = fpm_conf(&cfg, "8.4", &sites);
        assert!(
            conf.contains("env[DB_DATABASE] = \"shop_x_older\"\n"),
            "{conf}"
        );
        assert!(caddyfile(&cfg, &sites).contains("php_fastcgi"));
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
        let cfg = Config {
            ini: global,
            ..Config::default()
        };
        let conf = fpm_conf(&cfg, "8.4", &sites);
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
    fn a_config_without_https_default_serves_http() {
        let cfg: Config = toml::from_str("tld = \"test\"\n").unwrap();
        assert_eq!(cfg.https_default, None);
        assert!(cfg.sites.values().all(|s| s.https.is_none()));
    }

    #[test]
    #[cfg(target_os = "macos")]
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
    #[cfg(target_os = "linux")]
    fn linux_refuses_system_steps_it_cant_do_yet() {
        let cfg = Config::default();
        assert!(setup_script(&cfg, true, None, None).is_err());
        assert!(setup_script(&cfg, false, None, Some(Path::new("/h/root.crt"))).is_err());
        let local = Config {
            tld: "localhost".into(),
            ..Config::default()
        };
        assert_eq!(setup_script(&local, true, None, None).unwrap(), "");
    }

    #[test]
    #[cfg(target_os = "macos")] // osascript / plutil
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
        let lint = crate::platform::command("plutil")
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

    /// An Octane app's address goes to its Octane row's port, and to PHP-FPM (pages and
    /// files, answered 200) whenever Octane doesn't answer: stopped or crashed, never a 502.
    #[test]
    fn caddy_falls_back_to_fpm() {
        let dir = tempfile::tempdir().unwrap();
        // PHP-FPM's socket is in Anvil's home, which other tests change.
        let _env = lock(&ENV_LOCK);
        let path = dir.path().to_str().unwrap();
        fs::write(
            dir.path().join("composer.json"),
            r#"{"require": {"laravel/octane": "^2.22"}}"#,
        )
        .unwrap();
        let mut app = site("shop", path, Some("8.4"));
        app.kind = "laravel".into();
        let mut cfg = Config::default();
        let port = processes::octane_row(&cfg, &app).unwrap().port.unwrap();
        let out = caddyfile(&cfg, std::slice::from_ref(&app));
        let sock = fpm_socket(&app).unwrap();
        let block = format!(
            "\nshop.test {{\n\troot * \"{path}\"\n\tencode\n\t@hidden path */.*\n\treverse_proxy 127.0.0.1:{port}\n\thandle_errors 502 {{\n\t\tphp_fastcgi unix/{}\n\t\tfile_server @hidden {{\n\t\t\thide *\n\t\t}}\n\t\tfile_server {{\n\t\t\tstatus 200\n\t\t}}\n\t}}\n}}\n",
            sock.display()
        );
        assert!(out.contains(&block), "{out}");
        // The row hidden: PHP-FPM only, as before.
        cfg.sites
            .entry("shop".into())
            .or_default()
            .processes
            .insert(
                "octane".into(),
                ProcessConfig {
                    hidden: true,
                    ..Default::default()
                },
            );
        let out = caddyfile(&cfg, std::slice::from_ref(&app));
        assert!(!out.contains("reverse_proxy"), "{out}");
        assert!(out.contains(&format!(
            "\tphp_fastcgi unix/{}\n\tfile_server",
            sock.display()
        )));
    }

    #[test]
    fn the_database_viewer_is_served_by_the_default_php_unless_an_app_has_its_address() {
        let shop = site("shop", "/s/shop", Some("8.3"));
        let mut cfg = Config {
            php: Some("8.4".into()),
            ..Config::default()
        };
        let off = caddyfile(&cfg, std::slice::from_ref(&shop));
        assert!(!off.contains("db.test"));
        // db.test, as any host no app has: a 404, not Caddy's empty 200.
        assert!(off.ends_with(
            "\nhttp:// {\n\theader Content-Type \"text/plain; charset=utf-8\"\n\trespond \"No app at {host}\" 404\n}\n"
        ));
        assert!(!php_masters(&cfg, std::slice::from_ref(&shop)).contains("PHP 8.4"));
        cfg.db_viewer = true;
        let out = caddyfile(&cfg, std::slice::from_ref(&shop));
        let sock = home().join("run/php-8.4.sock");
        assert!(
            out.contains("\nhttp://db.test, https://db.test {\n"),
            "{out}"
        );
        assert!(out.contains(&format!("\tphp_fastcgi unix/{}\n}}\n", sock.display())));
        // Its PHP master runs though no app uses that version.
        assert!(php_masters(&cfg, std::slice::from_ref(&shop)).contains("PHP 8.4: "));
        let out = caddyfile(&cfg, &[shop, site("db", "/s/db", Some("8.4"))]);
        assert!(out.contains("\ndb.test {\n"), "{out}");
        assert!(!out.contains("http://db.test, https://db.test"));
    }

    #[test]
    fn php_gets_the_mail_catchers_env_from_caddy() {
        // FastCGI params: PHP-FPM's shared pool and Windows' php-cgi read them alike.
        let sites = [
            site("shop", "/s/shop/public", Some("8.4")),
            site("blog", "/s/blog", Some("8.4")),
        ];
        let mut cfg = Config::default();
        assert!(!caddyfile(&cfg, &sites).contains("MAIL_"));
        cfg.mail_catcher = true;
        cfg.sites.entry("blog".into()).or_default().mail_uncaught = true;
        let out = caddyfile(&cfg, &sites);
        let shop = &out[out.find("shop.test").unwrap()..out.find("blog.test").unwrap()];
        assert!(
            shop.contains(" {\n\t\tenv MAIL_MAILER \"smtp\"\n"),
            "{shop}"
        );
        assert!(shop.contains("\t\tenv MAIL_USERNAME \"shop\"\n"), "{shop}");
        assert!(!out[out.find("blog.test").unwrap()..].contains("MAIL_"));
        // A changed port re-serves.
        cfg.mail_port = Some(2600);
        assert!(caddyfile(&cfg, &sites).contains("env MAIL_PORT \"2600\""));
    }

    #[test]
    fn the_browser_bridge_is_in_only_its_apps_block_and_every_pool() {
        let dir = tempfile::tempdir().unwrap();
        let _env = lock(&ENV_LOCK);
        // SAFETY: every test that sets ANVIL_HOME holds ENV_LOCK.
        unsafe { std::env::set_var("ANVIL_HOME", dir.path().join("home")) };
        let mut shop = site("shop", "/s/shop/public", Some("8.4"));
        shop.kind = "laravel".into();
        let mut blog = site("blog", "/s/blog", Some("8.4"));
        blog.ini
            .insert("auto_prepend_file".into(), "/s/blog/pre.php".into());
        let mut cfg = Config::default();
        // Off for all: no trace of it.
        let off = caddyfile(&cfg, &[shop.clone(), blog.clone()]);
        assert!(
            !off.contains("__anvil") && !off.contains("ANVIL_INJECT"),
            "{off}"
        );
        assert!(!fpm_conf(&cfg, "8.4", &[shop.clone(), blog.clone()]).contains("inject"));
        shop.browser = true;
        let sites = [shop, blog];
        let out = caddyfile(&cfg, &sites);
        let (head, rest) = out.split_at(out.find("\nshop.test").unwrap());
        let block = &rest[..rest.find("\nblog.test").unwrap()];
        assert!(head.contains("\tlog anvil_browser_shop {\n"), "{head}");
        assert!(head.contains("browser/shop.log"), "{head}");
        assert!(
            block.contains(
                "\thandle /__anvil/e {\n\t\tlog_name anvil_browser_shop\n\t\trespond 204\n\t}\n"
            ),
            "{block}"
        );
        assert!(block.contains("\t\tenv ANVIL_INJECT \"1\"\n"), "{block}");
        // The collector goes before PHP gets the request.
        assert!(block.find("/__anvil/e").unwrap() < block.find("php_fastcgi").unwrap());
        let blog_block = &rest[rest.find("\nblog.test").unwrap()..];
        assert!(!blog_block.contains("__anvil") && !blog_block.contains("ANVIL_INJECT"));
        // Pool-wide: every pool prepends Anvil's file; the app's own is included by it.
        let prepend = browser::prepend_file().display().to_string();
        let conf = fpm_conf(&cfg, "8.4", &sites);
        let www = &conf[conf.find("[www]").unwrap()..conf.find("[site-blog]").unwrap()];
        assert!(
            www.contains(&format!("php_value[auto_prepend_file] = {prepend}\n")),
            "{www}"
        );
        assert!(!www.contains("ANVIL_PREPEND"));
        let own = &conf[conf.find("[site-blog]").unwrap()..];
        assert!(
            own.contains(&format!("php_value[auto_prepend_file] = {prepend}\n")),
            "{own}"
        );
        assert!(
            own.contains("env[ANVIL_PREPEND] = \"/s/blog/pre.php\"\n"),
            "{own}"
        );
        // A global one too.
        cfg.ini
            .insert("auto_prepend_file".into(), "/g/pre.php".into());
        let conf = fpm_conf(&cfg, "8.4", &sites);
        assert!(
            conf.contains("env[ANVIL_PREPEND] = \"/g/pre.php\"\n"),
            "{conf}"
        );
        // Caddy takes it, when this machine has Anvil's Caddy (CI has none).
        let bin = PathBuf::from(std::env::var("HOME").unwrap()).join(".anvil/bin/caddy");
        if bin.exists() {
            fs::write(dir.path().join("Caddyfile"), &out).unwrap();
            let adapt = crate::platform::command(&bin)
                .args(["adapt", "--adapter", "caddyfile", "--config"])
                .arg(dir.path().join("Caddyfile"))
                .env("XDG_DATA_HOME", dir.path())
                .env("XDG_CONFIG_HOME", dir.path())
                .output()
                .unwrap();
            let err = String::from_utf8_lossy(&adapt.stderr);
            assert!(adapt.status.success(), "{err}");
        }
    }

    /// A dev server, its server rows and Octane: page loads go through Anvil's proxy
    /// (with the app's port in a header), the rest straight to the app.
    #[test]
    fn the_browser_bridge_sends_only_page_loads_of_proxied_apps_through_anvil() {
        let home = tempfile::tempdir().unwrap();
        // The proxy's socket is in Anvil's home, which other tests change.
        let _env = lock(&ENV_LOCK);
        let cfg = Config::default();
        let (dir, mut vf) = vite_rows_site("bridge", "vuefinder");
        let row = processes::server_rows(&cfg, &vf)[0].port;
        let oct = tempfile::tempdir().unwrap();
        fs::write(
            oct.path().join("composer.json"),
            r#"{"require": {"laravel/octane": "^2.22"}}"#,
        )
        .unwrap();
        let mut shop = site("shop", oct.path().to_str().unwrap(), Some("8.4"));
        shop.kind = "laravel".into();
        let octane = processes::octane_row(&cfg, &shop).unwrap().port.unwrap();
        let mut plain = site("plain", "/s/plain", None);
        plain.kind = "static".into();
        plain.browser = true;
        let off = super::caddyfile(&cfg, &[vf.clone(), shop.clone(), plain.clone()]);
        assert!(
            !off.contains("anvil_doc") && !off.contains("__anvil"),
            "{off}"
        );
        assert!(!browser::proxied(
            &cfg,
            &[vf.clone(), shop.clone(), plain.clone()]
        ));
        assert!(!browser::proxied(&cfg, std::slice::from_ref(&plain)));

        vf.browser = true;
        shop.browser = true;
        let sites = [vf, shop, plain];
        assert!(browser::proxied(&cfg, &sites));
        let out = caddyfile(&cfg, &sites);
        let addr = browser::proxy::address();
        let route = |port: u16| {
            format!(
                "\t@anvil_doc {{\n\t\tmethod GET POST\n\t\theader_regexp Accept ^text/html\n\t\tnot header Upgrade *\n\t\tnot header Sec-Fetch-Dest empty\n\t}}\n\treverse_proxy @anvil_doc \"{addr}\" {{\n\t\theader_up X-Anvil-Upstream 127.0.0.1:{port}\n\t}}\n\treverse_proxy 127.0.0.1:{port}\n"
            )
        };
        let block = |host: &str| {
            let at = out.find(&format!("\n{host}")).unwrap();
            let end = out[at + 1..].find("\n}\n").unwrap();
            out[at..at + end].to_string()
        };
        for (host, port) in [
            ("vuefinder.test", 41002),
            ("vuefinder--docs.test", row),
            ("shop.test", octane),
        ] {
            let b = block(host);
            assert!(b.contains(&route(port)), "{host}: {b}");
            // The collector first, and into the app's own log.
            assert!(b.contains("\t\tlog_name anvil_browser_"), "{b}");
            assert!(b.find("/__anvil/e").unwrap() < b.find("@anvil_doc").unwrap());
        }
        assert!(block("vuefinder--docs.test").contains("log_name anvil_browser_vuefinder\n"));
        // Octane stopped: PHP-FPM gets the page, and its prepend adds the script.
        let shop = block("shop.test");
        let fallback = &shop[shop.find("handle_errors 502").unwrap()..];
        assert!(fallback.contains("env ANVIL_INJECT \"1\""), "{fallback}");
        assert!(out.contains("\tlog anvil_browser_vuefinder {\n"));
        assert!(!out.contains("anvil_browser_plain"));
        fs::remove_dir_all(dir).unwrap();

        // Caddy takes it, when this machine has Anvil's Caddy (CI has none).
        let bin = PathBuf::from(std::env::var("HOME").unwrap()).join(".anvil/bin/caddy");
        if bin.exists() {
            fs::write(
                home.path().join("Caddyfile"),
                super::caddyfile(&cfg, &sites),
            )
            .unwrap();
            let adapt = crate::platform::command(&bin)
                .args(["adapt", "--adapter", "caddyfile", "--config"])
                .arg(home.path().join("Caddyfile"))
                .env("XDG_DATA_HOME", home.path())
                .env("XDG_CONFIG_HOME", home.path())
                .output()
                .unwrap();
            let err = String::from_utf8_lossy(&adapt.stderr);
            assert!(adapt.status.success(), "{err}");
        }
    }

    #[test]
    fn a_web_root_in_a_dot_folder_hides_only_dotfiles_under_it() {
        let dir = tempfile::tempdir().unwrap();
        let public = dir.path().join(".output/public");
        fs::create_dir_all(&public).unwrap();
        let mut s = site("shop", dir.path().to_str().unwrap(), None);
        s.kind = "static".into();
        s.root = public.clone();
        s.root_custom = Some(".output/public".into());
        let out = caddyfile(&Config::default(), &[s]);
        let block = format!(
            "\troot * \"{}\"\n\tencode\n\t@hidden path */.*\n\tfile_server @hidden {{\n\t\thide *\n\t}}\n\tfile_server\n}}",
            public.display()
        );
        assert!(out.contains(&block), "{out}");
        // Caddy takes it, when this machine has Anvil's Caddy (CI has none).
        let bin = PathBuf::from(std::env::var("HOME").unwrap()).join(".anvil/bin/caddy");
        if bin.exists() {
            fs::write(dir.path().join("Caddyfile"), &out).unwrap();
            let adapt = crate::platform::command(&bin)
                .args(["adapt", "--adapter", "caddyfile", "--config"])
                .arg(dir.path().join("Caddyfile"))
                .env("XDG_DATA_HOME", dir.path())
                .env("XDG_CONFIG_HOME", dir.path())
                .output()
                .unwrap();
            let err = String::from_utf8_lossy(&adapt.stderr);
            assert!(adapt.status.success(), "{err}");
        }
    }

    /// Every block that answers for an app (its redirects and extra server rows too) logs
    /// to the app's one access log, which Caddy rolls at 2 MiB.
    #[test]
    fn every_block_of_an_app_logs_its_requests_to_the_apps_file() {
        let dir = tempfile::tempdir().unwrap();
        let _env = lock(&ENV_LOCK);
        // SAFETY: every test that sets ANVIL_HOME holds ENV_LOCK.
        unsafe { std::env::set_var("ANVIL_HOME", dir.path().join("home")) };
        let app_dir = dir.path().join("vuefinder");
        fs::create_dir_all(&app_dir).unwrap();
        fs::write(
            app_dir.join("package.json"),
            r#"{"scripts":{"dev":"vite","docs:dev":"vitepress dev docs"}}"#,
        )
        .unwrap();
        let mut vf = site("vuefinder", app_dir.to_str().unwrap(), None);
        vf.kind = "vite".into();
        vf.port = Some(41001);
        let shop = site("shop", dir.path().to_str().unwrap(), Some("8.4"));
        // Request log off: no app gets a log.
        let off = Config {
            request_log: false,
            ..Config::default()
        };
        let quiet = super::caddyfile(&off, &[vf.clone(), shop.clone()]);
        assert!(!quiet.contains("\tlog {"), "{quiet}");
        let out = super::caddyfile(&Config::default(), &[vf, shop]);
        let log = |name: &str| {
            format!(
                "\tlog {{\n\t\toutput file \"{}\" {{\n\t\t\troll_size 2MiB\n\t\t\troll_keep 1\n\t\t\troll_keep_for 24h\n\t\t}}\n\t\tformat json\n\t}}\n",
                caddy_path(&requests::file(name))
            )
        };
        // The app, its redirect, the docs row and its redirect.
        assert_eq!(out.matches(&log("vuefinder")).count(), 4, "{out}");
        assert_eq!(out.matches(&log("shop")).count(), 2, "{out}");
        assert_eq!(out.matches("\tlog {").count(), 6, "{out}");
        // Caddy takes it, when this machine has Anvil's Caddy (CI has none). `adapt`
        // opens no log file.
        let bin = PathBuf::from(std::env::var("HOME").unwrap()).join(".anvil/bin/caddy");
        if bin.exists() {
            fs::write(dir.path().join("Caddyfile"), &out).unwrap();
            let adapt = crate::platform::command(&bin)
                .args(["adapt", "--adapter", "caddyfile", "--config"])
                .arg(dir.path().join("Caddyfile"))
                .env("XDG_DATA_HOME", dir.path())
                .env("XDG_CONFIG_HOME", dir.path())
                .output()
                .unwrap();
            assert!(
                adapt.status.success(),
                "{}",
                String::from_utf8_lossy(&adapt.stderr)
            );
            let json = String::from_utf8_lossy(&adapt.stdout);
            assert!(json.contains(r#""roll_keep_days":1"#), "{json}");
        }
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
        // A tunnel (cloudflared, on this computer) says the public host the app should link to.
        assert!(
            out.contains("\tservers {\n\t\ttrusted_proxies static 127.0.0.1/32 ::1/128\n\t}\n")
        );
        assert_eq!(out.matches("@hidden path */.*").count(), 2);
        assert_eq!(
            out.matches("file_server @hidden {\n\t\thide *\n\t}\n\tfile_server\n}")
                .count(),
            2
        );
        assert!(
            !out.contains("hide .*"),
            "a dot in the root's own path hid every file"
        );
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
            "\nvuefinder--docs.test, vuefinder--docs.localhost {{\n\treverse_proxy 127.0.0.1:{port}"
        )));
        assert!(out.contains("\nhttp://vuefinder--docs.test, http://vuefinder--docs.localhost {\n\tredir https://{host}"));
        vf.secure = false;
        let out = caddyfile(&cfg, &[vf.clone()]);
        assert!(out.contains(&format!(
            "\nhttp://vuefinder--docs.test, http://vuefinder--docs.localhost {{\n\treverse_proxy 127.0.0.1:{port}"
        )));
        let to = site_url(
            &cfg,
            &Site {
                host: "vuefinder--docs.test".into(),
                secure: false,
                ..vf
            },
        );
        assert!(to.starts_with("http://"));
        assert!(out.contains(&format!(
            "\nhttp://docs.vuefinder.test {{\n\tredir {to}{{uri}} 308\n}}\n"
        )));
        assert_eq!(out.matches("\tredir ").count(), 1);
        fs::remove_dir_all(dir).unwrap();
    }

    fn vite_rows_site(tag: &str, name: &str) -> (PathBuf, Site) {
        let dir = std::env::temp_dir().join(format!("anvil-caddy-{tag}-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("package.json"),
            r#"{"scripts":{"dev":"vite","docs:dev":"vitepress dev docs"}}"#,
        )
        .unwrap();
        let mut s = site(name, dir.to_str().unwrap(), None);
        s.kind = "vite".into();
        s.port = Some(41002);
        (dir, s)
    }

    #[test]
    fn old_row_hosts_redirect_for_one_release() {
        let cfg = Config::default();
        let (dir, mut vf) = vite_rows_site("old", "vuefinder");
        let port = processes::server_rows(&cfg, &vf)[0].port;
        for secure in [true, false] {
            vf.secure = secure;
            let to = site_url(
                &cfg,
                &Site {
                    host: "vuefinder--docs.test".into(),
                    ..vf.clone()
                },
            );
            assert_eq!(to.starts_with("https://"), secure);
            let out = caddyfile(&cfg, &[vf.clone()]);
            let redir = format!("\n\tredir {to}{{uri}} 308\n}}\n");
            let scheme = if secure { "" } else { "http://" };
            assert!(out.contains(&format!("\n{scheme}docs.vuefinder.test {{{redir}")));
            if secure {
                assert!(out.contains(&format!("\nhttp://docs.vuefinder.test {{{redir}")));
            }
            assert!(out.contains(&format!(
                "\n{scheme}vuefinder--docs.test, {scheme}vuefinder--docs.localhost {{\n\treverse_proxy 127.0.0.1:{port}"
            )));
        }
        fs::remove_dir_all(dir).unwrap();
    }

    /// Every address of a Caddyfile, once per block it opens.
    fn addresses_of(caddy: &str) -> Vec<String> {
        caddy
            .lines()
            .filter(|l| l.ends_with(" {") && !l.starts_with(['\t', ' ']))
            .flat_map(|l| l.trim_end_matches(" {").split(", ").map(String::from))
            .collect()
    }

    #[test]
    fn caddyfile_never_has_one_host_twice() {
        let cfg = Config::default();
        let plain = |name: &str| {
            let mut s = site(name, "/s/plain", None);
            s.kind = "static".into();
            s
        };
        // An app folder named like a dev-server row; a worktree's own row next to a
        // worktree of that name; an alias named like an old row host.
        let (d1, vf) = vite_rows_site("dup-row", "vuefinder");
        let (d2, mut wt) = vite_rows_site("dup-wt", "shop--x");
        wt.worktree_of = Some("shop".into());
        let mut alias = plain("other");
        alias.aliases = vec!["docs.vuefinder.test".into()];
        let cases = [
            (
                vec![vf.clone(), plain("vuefinder--docs")],
                "vuefinder--docs.test",
            ),
            (vec![wt, plain("shop--x--docs")], "shop--x--docs.test"),
            (vec![vf, alias], "docs.vuefinder.test"),
        ];
        for (sites, host) in cases {
            for secure in [false, true] {
                let sites: Vec<Site> = sites
                    .iter()
                    .map(|s| Site {
                        secure,
                        ..s.clone()
                    })
                    .collect();
                let caddy = caddyfile(&cfg, &sites);
                let mut all = addresses_of(&caddy);
                let n = all.len();
                all.sort();
                all.dedup();
                assert_eq!(all.len(), n, "{caddy}");
                // The site keeps the name: its block serves files, not the row's proxy.
                let at = caddy.find(host).unwrap();
                let block = &caddy[at..at + caddy[at..].find("\n}").unwrap()];
                assert!(block.contains("file_server"), "{host}: {block}");
                // Caddy itself takes it, when this machine has Anvil's Caddy (CI has none).
                let bin = PathBuf::from(std::env::var("HOME").unwrap()).join(".anvil/bin/caddy");
                if bin.exists() {
                    let dir = crate::worktrees::tests::tmp("caddy-adapt");
                    let file = dir.join("Caddyfile");
                    fs::write(&file, &caddy).unwrap();
                    let out = crate::platform::command(&bin)
                        .args(["adapt", "--adapter", "caddyfile", "--config"])
                        .arg(&file)
                        .env("XDG_DATA_HOME", &dir)
                        .env("XDG_CONFIG_HOME", &dir)
                        .output()
                        .unwrap();
                    let err = String::from_utf8_lossy(&out.stderr);
                    assert!(out.status.success(), "{host}: {err}");
                }
            }
        }
        fs::remove_dir_all(d1).unwrap();
        fs::remove_dir_all(d2).unwrap();
    }

    #[test]
    fn localhost_row_host_keeps_a_name_that_contains_the_tld() {
        let cfg = Config::default();
        let (dir, vf) = vite_rows_site("dot", "my.test.app");
        let out = caddyfile(&cfg, &[vf]);
        assert!(
            out.contains("\nmy.test.app--docs.test, my.test.app--docs.localhost {"),
            "{out}"
        );
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

/// The PATH a terminal would have. An app opened from Finder gets launchd's short
/// PATH (/usr/bin:/bin:…), so pnpm, Homebrew, nvm and the like aren't found by
/// the processes Anvil starts. This asks the user's login shell once.
pub fn login_path() -> Option<String> {
    // Windows programs get the user's PATH from the system, whatever started them.
    if cfg!(windows) {
        return None;
    }
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    // A shell that waits on something (a prompt, a slow plugin) must not hold Anvil up.
    let out = output_within(
        crate::platform::command(shell).args(["-lic", "printf '\\n__ANVIL_PATH__%s\\n' \"$PATH\""]),
        std::time::Duration::from_secs(3),
    )?;
    parse_login_path(&out)
}

/// The standard output of `cmd`, or `None` when it can't start or takes longer than `limit`.
pub fn output_within(cmd: &mut Command, limit: std::time::Duration) -> Option<String> {
    let mut child = cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let deadline = std::time::Instant::now() + limit;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(20))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    Some(read_lossy(child.stdout?))
}

/// All of `r` as text; invalid UTF-8 becomes U+FFFD instead of an empty string
/// (`read_to_string` returns an error and leaves its buffer empty).
pub fn read_lossy(mut r: impl std::io::Read) -> String {
    let mut buf = Vec::new();
    let _ = r.read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).into_owned()
}

/// The PATH printed after the marker; shells may print banners around it.
fn parse_login_path(out: &str) -> Option<String> {
    out.lines()
        .rev()
        .find_map(|l| l.strip_prefix("__ANVIL_PATH__"))
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(String::from)
}

/// `current` with the login shell's entries added after it, each directory once.
/// The web server's (Caddy's) log.
pub fn caddy_log() -> PathBuf {
    home().join("log").join("caddy.log")
}

/// One PHP version's FPM log.
pub fn php_fpm_log(v: &str) -> PathBuf {
    home().join("log").join(format!("php-{v}-fpm.log"))
}

/// The PATH this process runs with.
pub fn env_path() -> String {
    std::env::var("PATH").unwrap_or_default()
}

/// `dir` first on `path`.
pub fn prepend_path(dir: &Path, path: &str) -> String {
    format!("{}{}{path}", dir.display(), platform::PATH_SEP)
}

pub fn merge_path(current: &str, login: &str) -> String {
    let mut seen = std::collections::HashSet::new();
    current
        .split(platform::PATH_SEP)
        .chain(login.split(platform::PATH_SEP))
        .filter(|d| !d.is_empty() && seen.insert(*d))
        .collect::<Vec<_>>()
        .join(&platform::PATH_SEP.to_string())
}

#[cfg(test)]
mod path_tests {
    use super::*;

    #[test]
    fn a_failed_download_says_where_and_why() {
        // Port 1 on loopback: nothing listens, so curl fails without the network.
        let e = format!(
            "{:#}",
            fetch_json::<serde_json::Value>("http://127.0.0.1:1/index.json").unwrap_err()
        );
        assert!(
            e.starts_with("couldn't fetch http://127.0.0.1:1/index.json: "),
            "{e}"
        );
        assert!(!e.contains("EOF"), "{e}");
    }

    #[test]
    fn folders_with_one_name_each_get_their_own() {
        let root = std::env::temp_dir().join(format!("anvil-names-{}", std::process::id()));
        let (a, b) = (root.join("a"), root.join("b"));
        for d in [
            a.join("My App"),
            a.join("my-app"),
            a.join("shop"),
            b.join("shop"),
        ] {
            fs::create_dir_all(d).unwrap();
        }
        let mut cfg = Config {
            paths: vec![b.clone(), a.clone()],
            ..Default::default()
        };
        let (found, clashes) = parked(&cfg);
        // The folder named like its site keeps the name; the first path for equal names.
        assert_eq!(found["my-app"].0, a.join("my-app"));
        assert_eq!(found["shop"].0, a.join("shop"));
        let other = format!("my-app-{}", short_hash(&a.join("My App").to_string_lossy()));
        assert_eq!(found[&other].0, a.join("My App"));
        assert_eq!(found.len(), 4);
        assert_eq!(clashes.len(), 2);
        assert!(clashes.iter().any(|c| c.contains(&other)), "{clashes:?}");
        // Another order of parked folders changes nothing.
        cfg.paths.reverse();
        assert_eq!(parked(&cfg).0, found);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_late_stop_keeps_the_new_starts_pid_file() {
        let f = std::env::temp_dir().join(format!("anvil-pids-{}", std::process::id()));
        fs::write(&f, "1\n200 Sat Oct 18 10:02:03 2026").unwrap();
        remove_pid_file(&f, 100);
        assert!(f.exists());
        remove_pid_file(&f, 200);
        assert!(!f.exists());
    }

    #[test]
    fn read_lossy_keeps_text_around_invalid_bytes() {
        assert_eq!(read_lossy(&b"ok \xff done\n"[..]), "ok \u{fffd} done\n");
    }

    #[test]
    fn reads_the_path_after_banners() {
        let out = "Welcome!\n\n__ANVIL_PATH__/opt/homebrew/bin:/usr/bin\n";
        assert_eq!(
            parse_login_path(out).as_deref(),
            Some("/opt/homebrew/bin:/usr/bin")
        );
        assert_eq!(parse_login_path("no marker"), None);
        assert_eq!(parse_login_path("__ANVIL_PATH__\n"), None);
    }

    #[test]
    fn merges_without_duplicates_keeping_order() {
        assert_eq!(
            merge_path(
                "/usr/bin:/bin",
                "/opt/homebrew/bin:/usr/bin:/Users/x/Library/pnpm"
            ),
            "/usr/bin:/bin:/opt/homebrew/bin:/Users/x/Library/pnpm"
        );
    }
}
