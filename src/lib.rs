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
pub mod project;
pub mod resources;
pub mod runtimes;
pub mod services;
pub mod shims;
pub mod style;
pub mod uninstall;
pub mod validate;
pub mod worktrees;

use anyhow::{Context, Result, bail};
use nix::sys::signal::Signal;
use nix::unistd::Pid;
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
    /// Weekly, trim apps' big `storage/logs/*.log` and delete old daily logs (`logs::trim_app_logs`).
    pub trim_app_logs: bool,
    /// macOS notifications for crashes and finished New applications while Anvil isn't in front.
    pub notifications: bool,
    /// Service licenses the user accepted before installing: license id -> date.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub accepted_licenses: BTreeMap<String, String>,
    /// Settings › Database for new worktrees: what one opened outside Anvil uses, and the
    /// New Worktree dialog's preselection.
    pub worktree_db: worktrees::DbChoice,
    /// Per worktree site (`<app>--<name>`): its database choice and copies made.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub worktrees: BTreeMap<String, worktrees::WorktreeConfig>,
}

#[derive(Serialize, Deserialize, Default, Clone)]
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
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub manual: bool,
    /// Added in Anvil, not detected: `command` is what it runs and the id is its name.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub added: bool,
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
            trim_app_logs: true,
            notifications: true,
            accepted_licenses: BTreeMap::new(),
            worktree_db: worktrees::DbChoice::Copy,
            worktrees: BTreeMap::new(),
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
        .unwrap_or_else(|| user_home().join(".anvil"))
}

/// The user's home folder: `$HOME`, else the account's entry when HOME isn't set, so
/// a path is never silently made relative to the current folder.
pub fn user_home() -> PathBuf {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            nix::unistd::User::from_uid(nix::unistd::getuid())
                .ok()
                .flatten()
                .map(|u| u.dir)
        })
        .expect("no home folder: HOME is not set and the account has none")
}

/// The config, or the defaults when there is none yet. Any other read error is
/// reported: treating it as "no config" would let the next save wipe it.
pub fn load() -> Result<Config> {
    let (cfg, migrated) = read()?;
    if !migrated {
        return Ok(cfg);
    }
    // The migration is a change like any other: saved under the lock.
    update(|_| Ok(()))?;
    Ok(read()?.0)
}

/// The config as on disk, migrated in memory; `true` when the migration changed it.
fn read() -> Result<(Config, bool)> {
    match fs::read_to_string(home().join("config.toml")) {
        Ok(s) => {
            let mut cfg: Config = toml::from_str(&s).context("config.toml is invalid")?;
            let migrated = cfg.https_default.is_none();
            if migrated {
                // Every site used to be HTTPS: the ones there now keep it, new ones get HTTP.
                for site in sites(&cfg) {
                    cfg.sites
                        .entry(site.name)
                        .or_default()
                        .https
                        .get_or_insert(true);
                }
                cfg.https_default = Some(false);
            }
            Ok((cfg, migrated))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok((Config::default(), false)),
        Err(e) => Err(e).context("couldn't read config.toml"),
    }
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
    let (mut cfg, migrated) = read()?;
    if migrated {
        // Saved before `f`, so a `load` inside it sees the migrated file.
        save(&cfg)?;
    }
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
    let real = |p: &Path| fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    parked(cfg)
        .0
        .into_values()
        .map(|(p, _)| real(&p))
        .chain(cfg.links.values().map(|p| real(p)))
        .collect()
}

pub fn sites(cfg: &Config) -> Vec<Site> {
    let mut found: BTreeMap<String, (PathBuf, Option<PathBuf>)> = parked(cfg)
        .0
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
    let real = |p: &Path| fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
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
    // worktrees are hidden with it.
    let app_paths = app_folders(cfg);
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
    sites
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
        ini: o
            .filter(|s| !s.ini.is_empty())
            .or(m)
            .map(|s| s.ini.clone())
            .unwrap_or_default(),
        port: o.and_then(|s| s.port),
        autostart: false,
        processes: BTreeMap::new(),
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
    let public = path.join("public");
    let root = if public.is_dir() {
        public
    } else {
        path.clone()
    };
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
        worktree_of: None,
        branch: None,
        aliases: vec![],
        runtime_version,
        name,
        path,
        root,
        linked: parked_in.is_none(),
        parked_in,
    }
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

/// Runs a shell `script` as root, after macOS asks for the password with `prompt`. The
/// script goes to a private (0600) file and the file's path and the prompt reach
/// AppleScript as arguments, so nothing in them is ever parsed as AppleScript.
pub fn run_as_admin(script: &str, prompt: &str) -> Result<()> {
    run_sh_file(script, prompt, ADMIN_SH)
}

const ADMIN_SH: &str = "do shell script \"/bin/sh \" & quoted form of (item 1 of argv) with prompt (item 2 of argv) with administrator privileges";

fn run_sh_file(script: &str, prompt: &str, applescript: &str) -> Result<()> {
    // tempfile creates the file 0600; it is removed when `file` drops.
    let mut file = tempfile::Builder::new()
        .prefix("anvil-admin-")
        .suffix(".sh")
        .tempfile()?;
    std::io::Write::write_all(&mut file, script.as_bytes())?;
    run(Command::new("osascript")
        .args(["-e", "on run argv", "-e", applescript, "-e", "end run"])
        .arg(file.path())
        .arg(prompt))
}

/// `curl <args> <url>`'s body; an error, with curl's reason, when it fails (`-f` makes an
/// HTTP error a failure).
pub(crate) fn curl(args: &[&str], url: &str) -> Result<Vec<u8>> {
    let out = Command::new("curl")
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

pub fn php_dir(v: &str) -> PathBuf {
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
    let list: Vec<serde_json::Value> = fetch_json(&format!("{PHP_INDEX}?format=json"))?;
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
pub(crate) fn pool(site: &Site) -> String {
    format!("site-{}", site.name.replace('.', "_"))
}

/// A site with php.ini overrides, or a worktree (its `env[]` lines), gets a pool of its own.
pub(crate) fn own_pool(site: &Site) -> bool {
    !site.ini.is_empty() || site.worktree_of.is_some()
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

/// PHP-FPM config for one version: the shared pool plus one pool per customised site.
fn fpm_conf(cfg: &Config, v: &str, sites: &[Site]) -> String {
    let global = &cfg.ini;
    let pool_conf = |name: &str, sock: &Path, max: u8| {
        format!(
            "\n[{name}]\nlisten = {}\npm = ondemand\npm.max_children = {max}\ncatch_workers_output = yes\nclear_env = no\n",
            sock.display()
        )
    };
    let mut s = format!(
        "[global]\nerror_log = {}\ndaemonize = no\n",
        php_fpm_log(v).display()
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
        .filter(|s| s.php.as_deref() == Some(v) && own_pool(s))
    {
        // A worktree Anvil can't separate from main gets no pool (Caddy shows why).
        let Ok(env) = worktrees::run_env(cfg, site, false) else {
            continue;
        };
        s += &pool_conf(&pool(site), &fpm_socket(site).unwrap(), 8);
        let mut ini = global.clone();
        ini.extend(site.ini.clone());
        s += &values(&ini);
        // Quoted: URLs hold `?`, `=`, `&`. One bad line would stop every pool; `env`
        // already refuses a database value that isn't safe, so only a shared one is left out.
        for (k, val) in env.iter().filter(|(k, v)| worktrees::fpm_safe(k, v)) {
            s += &format!("env[{k}] = \"{val}\"\n");
        }
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
            s += &format!(
                "\n{} {{\n\treverse_proxy 127.0.0.1:{port}\n\thandle_errors {{\n\t\theader Content-Type \"text/html; charset=utf-8\"\n\t\trespond `{}` 502\n\t}}\n}}\n",
                addresses(site, &names),
                not_running_page(&site.name),
            );
            if site.secure {
                s += &redirect(&names);
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
                    "\n{} {{\n\treverse_proxy 127.0.0.1:{}\n\thandle_errors {{\n\t\theader Content-Type \"text/html; charset=utf-8\"\n\t\trespond `{}` 502\n\t}}\n}}\n",
                    addresses(site, &names),
                    row.port,
                    not_running_page(&names[0]),
                );
                if site.secure {
                    s += &redirect(&names);
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
        if site
            .root
            .to_string_lossy()
            .contains(['"', '\\', '\n', '\r'])
        {
            continue;
        }
        if let Err(reason) = worktrees::run_env(cfg, site, false) {
            s += &format!(
                "\n{} {{\n\theader Content-Type \"text/html; charset=utf-8\"\n\trespond `{}` 502\n}}\n",
                addresses(site, &names),
                error_page(
                    &format!("{} has no database of its own", site.name),
                    &format!(
                        "Anvil can't point it at its own database ({reason}), so nothing of it runs."
                    ),
                ),
            );
            if site.secure {
                s += &redirect(&names);
            }
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
    s
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
    let keychain = user_home().join("Library/Keychains/login.keychain-db");
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
    pid.parse()
        .is_ok_and(|p| nix::sys::signal::kill(Pid::from_raw(p), None).is_ok())
}

/// SIGTERM to one process, if it is still there.
pub fn sigterm(pid: u32) {
    let _ = nix::sys::signal::kill(Pid::from_raw(pid as i32), Signal::SIGTERM);
}

/// When a process started, as `ps` prints it (`Sat Oct 18 10:02:03 2026`). With the pid
/// it names one process: a reused pid has another start time.
pub(crate) fn started(pid: u32) -> Option<String> {
    let o = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "lstart="])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// Clean up after an Anvil that died without stopping its children (crash, SIGKILL).
/// The pid file is `owner pid` followed by one line per child: `pid` or `pid started`.
/// Children are only killed when the owner is gone and the process is still the one
/// Anvil started: the same start time when recorded (a dev server's shell `exec`s its
/// command, so its command line no longer names Anvil), else a command line that runs
/// one of our binaries. With `group`, child pids are process-group leaders and the
/// whole group is killed.
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
    for line in pids {
        let (pid, at) = line
            .split_once(' ')
            .map_or((line, None), |(p, at)| (p, Some(at)));
        let same = match (at, pid.parse()) {
            (Some(at), Ok(n)) => started(n).as_deref() == Some(at),
            (Some(_), Err(_)) => false,
            (None, _) => Command::new("ps")
                .args(["-p", pid, "-o", "command="])
                .output()
                .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains(&ours)),
        };
        if let (true, Ok(p)) = (same, pid.parse()) {
            let p = Pid::from_raw(p);
            let _ = if group {
                nix::sys::signal::killpg(p, Signal::SIGTERM)
            } else {
                nix::sys::signal::kill(p, Signal::SIGTERM)
            };
            killed = true;
        }
    }
    let _ = fs::remove_file(pid_file);
    if killed {
        std::thread::sleep(std::time::Duration::from_millis(500)); // let ports close
    }
}

/// Whether something listens on this Mac's `port`.
pub fn port_answers(port: u16) -> bool {
    TcpStream::connect(("127.0.0.1", port)).is_ok()
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
    use nix::sys::signal::{kill, killpg};
    let pid = |c: &Child| Pid::from_raw(c.id() as i32);
    let groups: Vec<bool> = children
        .iter()
        .map(|c| {
            let group = killpg(pid(c), Signal::SIGTERM).is_ok();
            if !group {
                let _ = kill(pid(c), Signal::SIGTERM);
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
            let _ = killpg(pid(c), Signal::SIGKILL);
        }
        if !exited {
            let _ = c.kill();
        }
        let _ = c.wait();
    }
}

/// A process taken out of `Apps`, `Services` or `Server` (`take`, `take_all`), still to
/// be stopped: a shutdown takes seconds, so it happens once their lock is released.
pub struct Stopping {
    child: Child,
    /// Removed once it has stopped, if they still name it (`remove_pid_file`).
    pid_files: Vec<PathBuf>,
}

impl Stopping {
    pub fn wait(self, grace: std::time::Duration) {
        stop_all(vec![self], grace);
    }
}

/// Stop them all at once against one shared deadline (`terminate_all`), then remove
/// their pid files.
pub fn stop_all(list: Vec<Stopping>, grace: std::time::Duration) {
    let (mut children, files): (Vec<Child>, Vec<Vec<PathBuf>>) =
        list.into_iter().map(|s| (s.child, s.pid_files)).unzip();
    terminate_all(&mut children, grace);
    for (c, files) in children.iter().zip(files) {
        for f in files {
            remove_pid_file(&f, c.id());
        }
    }
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
        if port_answers(cfg.https_port) {
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
            fs::write(&conf, fpm_conf(cfg, v, &sites))?;
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
        let log = fs::File::create(caddy_log())?;
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
        let mut files = vec![pid_file()];
        self.children
            .drain(..)
            .map(|child| Stopping {
                child,
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

    /// A group leader is stopped with its group (a grandchild that ignores nothing
    /// dies too); a plain child alone, and one that ignores SIGTERM is killed.
    #[test]
    fn terminate_stops_a_group_or_a_single_child() {
        use std::os::unix::process::CommandExt;
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("grandchild");
        let mut leader = Command::new("sh")
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
        let mut stubborn = Command::new("sh")
            .args(["-c", "trap '' TERM; sleep 30"])
            .spawn()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));
        terminate(&mut stubborn, std::time::Duration::from_millis(300));
        assert!(matches!(stubborn.try_wait(), Ok(Some(_))));
        assert!(start.elapsed() < std::time::Duration::from_secs(5));
    }

    /// Quit stops everything against one deadline: three groups that ignore SIGTERM and
    /// three that don't take one grace in total, not three, and none is left behind.
    #[test]
    fn stop_all_shares_one_deadline() {
        use std::os::unix::process::CommandExt;
        let spawn = |script: &str| {
            Command::new("sh")
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
                nix::sys::signal::killpg(Pid::from_raw(pid), None).is_err()
            });
            assert!(gone, "group {pid} survived");
        }
    }

    #[test]
    fn ports_paths_and_homes() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let p = l.local_addr().unwrap().port();
        assert!(port_answers(p));
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
        let spawn = |cmd: &str| Command::new("/bin/sh").args(["-c", cmd]).spawn().unwrap();
        let mut server = Server {
            children: vec![spawn("sleep 30"), spawn("exit 3")],
            labels: vec!["PHP 8.4 FPM".into(), "Caddy".into()],
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
            worktree_of: None,
            branch: None,
            aliases: vec![],
        }
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
        let out = std::process::Command::new(std::env::current_exe().unwrap())
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
            std::process::Command::new("git").output().is_err(),
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
        let hidden = hidden_sites(&cfg);
        assert_eq!(hidden.len(), 1);
        assert_eq!(hidden[0].name, "shop");
        unhide(&mut cfg, "shop").unwrap();
        assert!(hidden_sites(&cfg).is_empty());
        assert!(sites(&cfg).iter().any(|s| s.name == "shop"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn fpm_gives_a_worktree_its_own_pool_with_env() {
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
                    let out = std::process::Command::new(&bin)
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
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    // A shell that waits on something (a prompt, a slow plugin) must not hold Anvil up.
    let out = output_within(
        Command::new(shell).args(["-lic", "printf '\\n__ANVIL_PATH__%s\\n' \"$PATH\""]),
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
    format!("{}:{path}", dir.display())
}

pub fn merge_path(current: &str, login: &str) -> String {
    let mut seen = std::collections::HashSet::new();
    current
        .split(':')
        .chain(login.split(':'))
        .filter(|d| !d.is_empty() && seen.insert(*d))
        .collect::<Vec<_>>()
        .join(":")
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
