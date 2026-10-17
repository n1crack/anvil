//! The other processes an app runs while you work on it: a queue worker, a log
//! tailer, Vite. Read from `.anvil.toml`, else from the project's own dev script
//! (`composer run dev` with concurrently, or package.json's `dev`).

use crate::{Site, apps, home, runtimes};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Serialize, Debug, Clone, PartialEq, Default)]
pub struct Process {
    /// Key and file-name part (`process_id` of the name); `name` is what the UI shows.
    pub id: String,
    pub name: String,
    pub command: String,
    /// Where it was found: `.anvil.toml`, `artisan dev`, `composer.json` or `package.json`.
    pub source: &'static str,
    /// A long-running dev server, as opposed to a one-off command.
    pub server: bool,
    /// The port a package.json server row is told to listen on (`$PORT`) and is proxied from.
    pub port: Option<u16>,
    /// Started together with Anvil.
    pub autostart: bool,
    /// The command was overridden in the app's settings.
    pub edited: bool,
    /// A package.json command script the user pinned as a row.
    pub pinned: bool,
    /// Kept on top: listed first after the app's own row.
    pub top: bool,
    /// Left out of Dev start (the header's Start): started by hand only.
    pub manual: bool,
    /// Added in Anvil rather than detected.
    pub added: bool,
}

/// An edit of one process's settings; `None` leaves a field as it is.
#[derive(Default, Debug)]
pub struct ProcessChange {
    /// `Some(None)` goes back to the detected command.
    pub command: Option<Option<String>>,
    pub autostart: Option<bool>,
    pub hidden: Option<bool>,
    pub pinned: Option<bool>,
    pub top: Option<bool>,
    /// The address's subdomain label; `Some(None)` goes back to the automatic one.
    pub host: Option<Option<String>>,
    pub manual: Option<bool>,
}

/// Merge `change` into the site's settings for process `id`; an entry back at its
/// defaults is dropped. The app's own row (`""`) lives in `SiteConfig.command` instead.
pub fn apply_process_change(
    site: &mut crate::SiteConfig,
    id: &str,
    change: ProcessChange,
) -> anyhow::Result<()> {
    anyhow::ensure!(!id.is_empty(), "the app's own row has no process settings");
    let mut p = site.processes.get(id).cloned().unwrap_or_default();
    if let Some(command) = change.command {
        p.command = match command {
            Some(c) => {
                anyhow::ensure!(!c.trim().is_empty(), "the command can't be empty");
                Some(c)
            }
            None => None,
        };
    }
    p.autostart = change.autostart.unwrap_or(p.autostart);
    p.hidden = change.hidden.unwrap_or(p.hidden);
    p.pinned = change.pinned.unwrap_or(p.pinned);
    p.top = change.top.unwrap_or(p.top);
    p.manual = change.manual.unwrap_or(p.manual);
    if let Some(host) = change.host {
        p.host = match host {
            Some(h) => {
                let h = h.trim().to_lowercase();
                anyhow::ensure!(
                    (1..=63).contains(&h.len())
                        && h.bytes()
                            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                        && !h.starts_with('-')
                        && !h.ends_with('-'),
                    "“{h}” can't be an address: use 1–63 letters, digits and dashes, not starting or ending with a dash"
                );
                Some(h)
            }
            None => None,
        };
    }
    // Removing a process the user added forgets it; a detected one stays hidden.
    if p == crate::ProcessConfig::default() || (p.added && p.hidden) {
        site.processes.remove(id);
    } else {
        site.processes.insert(id.into(), p);
    }
    Ok(())
}

/// A process of the user's own: `name` is its id, and must not be one the app has.
fn add_entry(
    site: &mut crate::SiteConfig,
    taken: &[String],
    name: &str,
    command: &str,
) -> anyhow::Result<()> {
    let name = name.trim();
    anyhow::ensure!(
        valid_name(name),
        "“{name}” can't be a name: use up to 40 letters, digits, dashes and underscores"
    );
    anyhow::ensure!(
        !taken.iter().any(|t| t == name) && !site.processes.contains_key(name),
        "{name} is already a process of this app"
    );
    anyhow::ensure!(!command.trim().is_empty(), "the command can't be empty");
    site.processes.insert(
        name.into(),
        crate::ProcessConfig {
            command: Some(command.trim().into()),
            added: true,
            ..Default::default()
        },
    );
    Ok(())
}

/// Add a process of the user's own to `site` and save.
pub fn add_process(
    cfg: &mut crate::Config,
    site: &Site,
    name: &str,
    command: &str,
) -> anyhow::Result<()> {
    let taken: Vec<String> = detect_with(site, Some(cfg))
        .into_iter()
        .map(|p| p.id)
        .collect();
    add_entry(
        cfg.sites.entry(site.name.clone()).or_default(),
        &taken,
        name,
        command,
    )?;
    crate::save(cfg)
}

/// Back to what is detected: added processes go, hidden ones return, and commands,
/// Dev start, Start with Anvil and pins are the defaults again. Addresses and Keep on
/// Top stay: the Configure table doesn't show them.
fn reset_entries(site: &mut crate::SiteConfig) {
    site.processes.retain(|_, p| !p.added);
    for p in site.processes.values_mut() {
        *p = crate::ProcessConfig {
            host: p.host.take(),
            top: p.top,
            ..Default::default()
        };
    }
    site.processes
        .retain(|_, p| *p != crate::ProcessConfig::default());
}

/// `reset_entries` for `site`, saved.
pub fn reset_processes(cfg: &mut crate::Config, site: &Site) -> anyhow::Result<()> {
    if let Some(s) = cfg.sites.get_mut(&site.name) {
        reset_entries(s);
    }
    crate::save(cfg)
}

/// Change one process's settings of `site` and save.
pub fn set_process(
    cfg: &mut crate::Config,
    site: &Site,
    id: &str,
    change: ProcessChange,
) -> anyhow::Result<()> {
    change_process(cfg, site, id, change)?;
    crate::save(cfg)
}

/// `set_process` without saving. An address must be a row's and not another row's.
fn change_process(
    cfg: &mut crate::Config,
    site: &Site,
    id: &str,
    change: ProcessChange,
) -> anyhow::Result<()> {
    let check_host = change.host.is_some();
    // A new entry is only added once the change is accepted; a rejected address is undone.
    let existed = cfg.sites.contains_key(&site.name);
    let s = cfg.sites.entry(site.name.clone()).or_default();
    let before = s.processes.get(id).cloned();
    let result = apply_process_change(s, id, change).and_then(|()| match check_host {
        true => host_free(cfg, site, id),
        false => Ok(()),
    });
    if result.is_err() {
        let s = cfg.sites.get_mut(&site.name).expect("added above");
        match before {
            Some(b) => s.processes.insert(id.into(), b),
            None => s.processes.remove(id),
        };
        if !existed {
            cfg.sites.remove(&site.name);
        }
    }
    result
}

/// Row `id` has an address and no other row has the same one. Hidden rows count
/// too: they keep their address for when they come back.
fn host_free(cfg: &crate::Config, site: &Site, id: &str) -> anyhow::Result<()> {
    let all = all_rows(cfg, site);
    let (_, row) = all
        .iter()
        .find(|(p, _)| p.id == id)
        .ok_or_else(|| anyhow::anyhow!("{id} has no address"))?;
    if let Some((p, _)) = all.iter().find(|(p, r)| p.id != id && r.host == row.host) {
        anyhow::bail!("{} is already the address of {}", row.host, p.name);
    }
    Ok(())
}

/// Ids of the processes hidden in the app's settings, so they can be brought back.
pub fn hidden(site: &crate::SiteConfig) -> Vec<String> {
    site.processes
        .iter()
        .filter(|(_, p)| p.hidden)
        .map(|(id, _)| id.clone())
        .collect()
}

/// An extra dev server of a non-PHP app, answering at `host`.
pub struct ServerRow {
    pub id: String,
    pub host: String,
    pub port: u16,
}

#[derive(Deserialize, Default)]
struct ProjectFile {
    #[serde(default)]
    processes: std::collections::BTreeMap<String, String>,
}

/// `.anvil.toml` that can't be read as TOML, so it isn't silently ignored.
pub fn project_file_error(site: &Site) -> Option<String> {
    let text = fs::read_to_string(site.path.join(".anvil.toml")).ok()?;
    toml::from_str::<ProjectFile>(&text)
        .err()
        .map(|e| format!(".anvil.toml: {}", e.message()))
}

/// An app's processes: `.anvil.toml`'s by name, a dev script's in its order.
pub fn detect(site: &Site) -> Vec<Process> {
    // The config gives non-PHP apps their rows' ports (no config, no rows) and every app its settings.
    detect_with(site, crate::load().ok().as_ref())
}

/// `detect` with a config already loaded.
pub fn detect_with(site: &Site, cfg: Option<&crate::Config>) -> Vec<Process> {
    let php = home()
        .join("php")
        .join(site.php.as_deref().unwrap_or_default())
        .join("php");
    detect_in(site, &php, &home().join("cache"), cfg)
}

fn detect_in(site: &Site, php: &Path, cache: &Path, cfg: Option<&crate::Config>) -> Vec<Process> {
    apply_settings(site, cfg, detect_base(site, php, cache, cfg))
}

/// The app's per-process settings applied: hidden rows dropped, commands replaced,
/// autostart copied, and pinned package.json commands added as rows.
fn apply_settings(
    site: &Site,
    cfg: Option<&crate::Config>,
    mut procs: Vec<Process>,
) -> Vec<Process> {
    let Some(set) = cfg
        .and_then(|c| c.sites.get(&site.name))
        .map(|s| &s.processes)
        .filter(|p| !p.is_empty())
    else {
        return procs;
    };
    let pinned = |id: &str| set.get(id).is_some_and(|p| p.pinned);
    let extra: Vec<Process> = pinned_scripts(&site.path, &pinned)
        .into_iter()
        .map(|(p, _)| p)
        .filter(|p| !procs.iter().any(|d| d.id == p.id))
        .collect();
    procs.extend(extra);
    // The user's own, after the detected ones; a detected id wins over an added one.
    let added: Vec<Process> = set
        .iter()
        .filter(|(id, c)| c.added && valid_name(id) && !procs.iter().any(|p| &p.id == *id))
        .filter_map(|(id, c)| {
            Some(Process {
                id: id.clone(),
                name: id.clone(),
                command: c.command.clone()?,
                source: "Anvil",
                added: true,
                ..Default::default()
            })
        })
        .collect();
    procs.extend(added);
    procs.retain(|p| !set.get(&p.id).is_some_and(|c| c.hidden));
    for p in &mut procs {
        if let Some(c) = set.get(&p.id) {
            p.autostart = c.autostart;
            p.pinned = c.pinned;
            p.top = c.top;
            p.manual = c.manual;
            if let Some(command) = &c.command {
                p.command = command.clone();
                p.edited = !c.added;
            }
        }
    }
    procs
}

/// package.json's scripts by name, as written there.
pub fn script_texts(dir: &Path) -> std::collections::BTreeMap<String, String> {
    fs::read_to_string(dir.join("package.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|j| serde_json::from_value(j["scripts"].clone()).ok())
        .unwrap_or_default()
}

/// The package.json command scripts `keep` accepts, as rows in file order, each with
/// whether the script pins its own port.
fn pinned_scripts(dir: &Path, keep: &dyn Fn(&str) -> bool) -> Vec<(Process, bool)> {
    let json: serde_json::Value = fs::read_to_string(dir.join("package.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let empty = serde_json::Map::new();
    let scripts = json["scripts"].as_object().unwrap_or(&empty);
    let commands = package_scripts(dir).1;
    scripts
        .iter()
        .filter(|(n, _)| commands.contains(n) && keep(&process_id(n)))
        .filter_map(|(n, c)| Some((n, c.as_str()?)))
        .map(|(name, command)| {
            let p = Process {
                id: process_id(name),
                name: name.clone(),
                command: start_command(name, command, scripts),
                source: "package.json",
                server: is_server_script(name, command),
                ..Default::default()
            };
            (p, pin_flags(command).is_some())
        })
        .collect()
}

fn detect_base(site: &Site, php: &Path, cache: &Path, cfg: Option<&crate::Config>) -> Vec<Process> {
    let cfg = cfg.filter(|_| !apps::is_php(&site.kind));
    let own = fs::read_to_string(site.path.join(".anvil.toml"))
        .ok()
        .and_then(|t| toml::from_str::<ProjectFile>(&t).ok());
    if let Some(f) = own.filter(|f| !f.processes.is_empty()) {
        return f
            .processes
            .into_iter()
            .filter(|(n, c)| valid_name(n) && !c.trim().is_empty())
            .map(|(name, command)| Process {
                id: name.clone(),
                name,
                command,
                source: ".anvil.toml",
                server: false,
                port: None,
                ..Default::default()
            })
            .collect();
    }
    // Apps with their own server already run their dev command; the rest of their
    // dev servers (a docs site) are rows of their own.
    if !apps::is_php(&site.kind) {
        return cfg.map_or(vec![], |c| {
            rows(c, site).into_iter().map(|(p, _)| p).collect()
        });
    }
    if let Some(list) = laravel_dev_commands_cached(&site.path, php, cache) {
        return list
            .into_iter()
            .map(|(name, command)| Process {
                id: process_id(&name),
                name,
                command,
                source: "artisan dev",
                server: false,
                port: None,
                ..Default::default()
            })
            .collect();
    }
    let json = |file: &str| -> serde_json::Value {
        fs::read_to_string(site.path.join(file))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    };
    let composer = json("composer.json");
    let dev = &composer["scripts"]["dev"];
    let scripts: Vec<&str> = match dev {
        serde_json::Value::String(s) => vec![s.as_str()],
        serde_json::Value::Array(a) => a.iter().filter_map(|v| v.as_str()).collect(),
        _ => vec![],
    };
    let split: Vec<Process> = scripts
        .iter()
        .find_map(|s| concurrently(s))
        .unwrap_or_default()
        .into_iter()
        .map(|(name, command)| Process {
            id: name.clone(),
            name,
            command,
            source: "composer.json",
            server: false,
            port: None,
            ..Default::default()
        })
        .collect();
    if !split.is_empty() {
        return split;
    }
    // No dev script to split: Vite (or whatever `npm run dev` is) on its own.
    if json("package.json")["scripts"]["dev"].is_string() {
        return vec![Process {
            id: "vite".into(),
            name: "vite".into(),
            command: "npm run dev".into(),
            source: "package.json",
            server: false,
            port: None,
            ..Default::default()
        }];
    }
    vec![]
}

const DEV_COMMANDS: &str = "vendor/laravel/framework/src/Illuminate/Foundation/DevCommands.php";

/// What `composer run dev` / `php artisan dev` would start, asked of Laravel 13 itself
/// (minus `server`, which Anvil already runs). `None` when it can't be asked.
pub fn laravel_dev_commands(dir: &Path, php: &Path) -> Option<Vec<(String, String)>> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    if !dir.join(DEV_COMMANDS).is_file() {
        return None;
    }
    let script = r#"require 'vendor/autoload.php'; $app = require 'bootstrap/app.php'; $app->make(Illuminate\Contracts\Console\Kernel::class)->bootstrap(); echo json_encode(array_map(fn($c) => ['name' => $c['name'], 'command' => $c['command']], \Illuminate\Foundation\DevCommands::commands()));"#;
    let mut child = Command::new(php)
        // Warnings go to stderr, so they can't spoil the JSON on stdout.
        .args(["-d", "display_errors=stderr", "-r", script])
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        match child.try_wait().ok()? {
            Some(s) => break s,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    let mut out = String::new();
    child.stdout.take()?.read_to_string(&mut out).ok()?;
    if !status.success() {
        return None;
    }
    let list: Vec<serde_json::Value> = serde_json::from_str(out.trim()).ok()?;
    Some(
        list.iter()
            .filter_map(|c| Some((c["name"].as_str()?, c["command"].as_str()?)))
            .filter(|(n, _)| *n != "server")
            .map(|(n, c)| (n.to_string(), c.to_string()))
            .collect(),
    )
}

/// Seconds a failed `laravel_dev_commands` is remembered.
const RETRY_FAILED: u64 = 300;

/// `laravel_dev_commands`, remembered until composer.lock, a provider or PHP changes
/// (booting Laravel costs a few hundred ms); a failure only for `RETRY_FAILED`.
fn laravel_dev_commands_cached(
    dir: &Path,
    php: &Path,
    cache: &Path,
) -> Option<Vec<(String, String)>> {
    use std::hash::{Hash, Hasher};
    if !dir.join(DEV_COMMANDS).is_file() {
        return None;
    }
    let mtime = |p: PathBuf| {
        fs::metadata(p)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_nanos() as u64)
    };
    // Editing a file doesn't touch its folder's mtime: the newest provider file counts.
    let providers = fs::read_dir(dir.join("app/Providers"))
        .into_iter()
        .flatten()
        .filter_map(|e| Some(mtime(e.ok()?.path())))
        .chain([mtime(dir.join("bootstrap/providers.php"))])
        .max()
        .unwrap_or(0);
    let stamp = [
        mtime(dir.join("composer.lock")),
        providers,
        mtime(php.into()),
    ];
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let mut h = std::collections::hash_map::DefaultHasher::new();
    dir.hash(&mut h);
    let file = cache.join(format!("devcommands-{:x}.json", h.finish()));
    // A failed boot is remembered too (`commands: None`), so it isn't retried every
    // poll, but only for a while: it may have been a database that wasn't up yet.
    #[derive(Serialize, Deserialize)]
    struct Cached {
        stamp: [u64; 3],
        php: PathBuf,
        commands: Option<Vec<(String, String)>>,
        /// Unix seconds when PHP was asked.
        at: u64,
    }
    if let Some(c) = fs::read_to_string(&file)
        .ok()
        .and_then(|t| serde_json::from_str::<Cached>(&t).ok())
        .filter(|c| c.stamp == stamp && c.php == php)
        .filter(|c| c.commands.is_some() || now.saturating_sub(c.at) < RETRY_FAILED)
    {
        return c.commands;
    }
    let commands = laravel_dev_commands(dir, php);
    let _ = fs::create_dir_all(cache);
    if let Ok(t) = serde_json::to_string(&Cached {
        stamp,
        php: php.into(),
        commands: commands.clone(),
        at: now,
    }) {
        let _ = fs::write(&file, t);
    }
    commands
}

/// The extra dev servers of a non-PHP app (everything but its own `dev`) with a
/// stable port each and an address `<row>.<app host>`.
pub fn server_rows(cfg: &crate::Config, site: &Site) -> Vec<ServerRow> {
    rows(cfg, site).into_iter().map(|(_, r)| r).collect()
}

fn rows(cfg: &crate::Config, site: &Site) -> Vec<(Process, ServerRow)> {
    let hidden = |id: &str| {
        cfg.sites
            .get(&site.name)
            .is_some_and(|s| s.processes.get(id).is_some_and(|p| p.hidden))
    };
    // Dropped after ports and prefixes are given out, so the other rows keep theirs.
    all_rows(cfg, site)
        .into_iter()
        .filter(|(p, _)| !hidden(&p.id))
        .collect()
}

/// `rows` with the hidden ones.
fn all_rows(cfg: &crate::Config, site: &Site) -> Vec<(Process, ServerRow)> {
    // `.anvil.toml`'s processes replace everything detected.
    let own = fs::read_to_string(site.path.join(".anvil.toml"))
        .ok()
        .and_then(|t| toml::from_str::<ProjectFile>(&t).ok());
    if !apps::runs_process(&site.kind) || own.is_some_and(|f| !f.processes.is_empty()) {
        return vec![];
    }
    let mut taken: Vec<u16> = cfg.sites.values().filter_map(|s| s.port).collect();
    taken.extend(site.port);
    for id in crate::services::instances(cfg) {
        taken.push(crate::services::port(cfg, &id));
        taken.extend(crate::services::web_port(cfg, &id));
    }
    let mut prefixes: Vec<String> = vec![];
    // Pinned command scripts that pin their own port are rows like the servers.
    let set = |id: &str| cfg.sites.get(&site.name)?.processes.get(id);
    let pinned = |id: &str| set(id).is_some_and(|p| p.pinned);
    let mut procs = package_scripts(&site.path).0;
    procs.retain(|p| p.name != "dev");
    procs.extend(
        pinned_scripts(&site.path, &pinned)
            .into_iter()
            .filter_map(|(p, pins)| pins.then_some(p)),
    );
    procs
        .into_iter()
        .map(|mut p| {
            // ponytail: ports of other apps' rows aren't known here; the 4000-wide hash makes a clash rare.
            let port = apps::hashed_port(&format!("{}@{}", site.name, p.id), 45000, &taken, cfg);
            taken.push(port);
            // The first row to cut to a prefix keeps it; later ones use their full id.
            let cut = process_id(p.name.split(':').next().unwrap_or_default());
            let prefix = if cut.is_empty() || prefixes.contains(&cut) {
                p.id.clone()
            } else {
                cut
            };
            prefixes.push(prefix.clone());
            p.port = Some(port);
            // A label set in the app's settings wins; automatic ones stay as they were.
            let label = set(&p.id).and_then(|c| c.host.clone()).unwrap_or(prefix);
            let row = ServerRow {
                id: p.id.clone(),
                host: format!("{label}.{}", site.host),
                port,
            };
            (p, row)
        })
        .collect()
}

/// The flags that pin a script that is itself `vite`, `vp dev`, `vitepress` or `astro` (maybe through
/// npx/yarn/pnpm exec), so `-- --port …` reaches it. A wrapper like `run-s` would take them itself.
fn pin_flags(command: &str) -> Option<&'static str> {
    let words = shell_words(command);
    let mut w = words.iter().map(String::as_str);
    let first = match w.next() {
        Some("npx" | "yarn") => w.next(),
        Some("pnpm") => w.next().filter(|x| *x == "exec").and_then(|_| w.next()),
        p => p,
    };
    if words
        .iter()
        .any(|w| matches!(w.as_str(), "&&" | "||" | ";" | "|" | "&"))
    {
        return None;
    }
    match (first, w.next()) {
        // Vite+ (`vp dev`) is Vite: it takes the same flags.
        (Some("vite" | "vitepress"), _) | (Some("vp"), Some("dev")) => {
            Some("--port $PORT --strictPort --host 127.0.0.1")
        }
        (Some("astro"), _) => Some("--port $PORT --host 127.0.0.1"),
        _ => None,
    }
}

/// `npm run <name>` with the port flags when the script pins itself, or when it is a plain
/// `run-s a b c` chain whose last script does (`npm run a && npm run b && npm run c -- <flags>`).
fn start_command(
    name: &str,
    command: &str,
    scripts: &serde_json::Map<String, serde_json::Value>,
) -> String {
    let script = |n: &str| scripts.get(n).and_then(|v| v.as_str());
    if let Some(flags) = pin_flags(command) {
        return format!("npm run {name} -- {flags}");
    }
    let words = shell_words(command);
    let steps: Vec<&str> = match words.first().map(String::as_str) {
        Some("run-s" | "npm-run-all") => words[1..].iter().map(String::as_str).collect(),
        Some("npx")
            if matches!(
                words.get(1).map(String::as_str),
                Some("run-s" | "npm-run-all")
            ) =>
        {
            words[2..].iter().map(String::as_str).collect()
        }
        _ => vec![],
    };
    // Only plain script names: flags, patterns and arguments leave it as written.
    let chain = steps.len() > 1 && steps.iter().all(|s| script(s).is_some());
    let flags = chain
        .then(|| script(steps[steps.len() - 1]).and_then(pin_flags))
        .flatten();
    match flags {
        Some(flags) => {
            let (last, first) = steps.split_last().unwrap();
            let mut out: Vec<String> = first.iter().map(|s| format!("npm run {s}")).collect();
            out.push(format!("npm run {last} -- {flags}"));
            out.join(" && ")
        }
        None => format!("npm run {name}"),
    }
}

/// A script name as an id: every char outside letters, digits, `-` and `_` becomes `-`.
pub fn process_id(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// Whether a package.json script is a long-running dev server rather than a
/// one-off command.
pub fn is_server_script(name: &str, command: &str) -> bool {
    // Secondary: needs a build first, or is a step of a `:dev` script.
    if name == "preview" || name.ends_with(":preview") || name.ends_with(":serve") {
        return false;
    }
    if command.trim_start().starts_with("start-server-and-test") {
        return false;
    }
    if matches!(name, "dev" | "start" | "serve") || name.ends_with(":dev") {
        return true;
    }
    let words = shell_words(command);
    let mut w = words.iter().map(String::as_str).peekable();
    // Skip the runner in front of the program.
    match w.next() {
        Some("npx" | "yarn") => {}
        Some("pnpm") if w.peek() == Some(&"exec") => {
            w.next();
        }
        Some(p) => return program_serves(p, w.next()),
        None => return false,
    }
    w.next().is_some_and(|p| program_serves(p, w.next()))
}

fn program_serves(program: &str, arg: Option<&str>) -> bool {
    match program {
        "vite" => !matches!(arg, Some("build" | "preview")),
        "vp" | "vitepress" | "nuxt" | "next" | "astro" | "storybook" => arg == Some("dev"),
        _ => false,
    }
}

/// An app's package.json scripts: the dev servers as rows (in file order), and
/// the names of the rest, minus npm's lifecycle hooks.
pub fn package_scripts(dir: &Path) -> (Vec<Process>, Vec<String>) {
    let json: serde_json::Value = fs::read_to_string(dir.join("package.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let (mut servers, mut commands) = (vec![], vec![]);
    let empty = serde_json::Map::new();
    let scripts = json["scripts"].as_object().unwrap_or(&empty);
    for (name, command) in scripts {
        let Some(command) = command.as_str() else {
            continue;
        };
        // `pre<x>`/`post<x>` run around an existing script `<x>`; `preview` isn't one.
        let hook = ["pre", "post"].iter().any(|p| {
            name.strip_prefix(p)
                .is_some_and(|x| scripts.contains_key(x))
        }) || matches!(name.as_str(), "prepare" | "prepack" | "postinstall");
        if hook {
            continue;
        }
        if is_server_script(name, command) {
            servers.push(Process {
                id: process_id(name),
                name: name.clone(),
                command: start_command(name, command, scripts),
                source: "package.json",
                server: true,
                port: None,
                ..Default::default()
            });
        } else {
            commands.push(name.clone());
        }
    }
    (servers, commands)
}

/// A script of the app's run once, not a dev process: a package.json command or a
/// composer.json script.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Script {
    /// The script's name as written (`build:all`).
    pub name: String,
    /// Key and log name part: `process_id` of the name, `composer-` first for Composer's.
    pub id: String,
    /// The script as written in its file.
    pub text: String,
    /// `package.json` or `composer.json`.
    pub source: &'static str,
    /// What runs it.
    #[serde(skip)]
    pub run: String,
}

fn quoted(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// The app's one-off scripts: package.json's commands (see `package_scripts`), then a PHP
/// app's composer.json scripts other than `dev` and Composer's event hooks.
pub fn one_off_scripts(site: &Site) -> Vec<Script> {
    let texts = script_texts(&site.path);
    let mut out: Vec<Script> = package_scripts(&site.path)
        .1
        .into_iter()
        .map(|name| Script {
            id: process_id(&name),
            text: texts.get(&name).cloned().unwrap_or_default(),
            run: format!("npm run {}", quoted(&name)),
            source: "package.json",
            name,
        })
        .collect();
    if !apps::is_php(&site.kind) {
        return out;
    }
    let composer: serde_json::Value = fs::read_to_string(site.path.join("composer.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let Some(scripts) = composer["scripts"].as_object() else {
        return out;
    };
    let php = home()
        .join("php")
        .join(site.php.as_deref().unwrap_or_default())
        .join("php");
    let phar = home().join("bin/composer.phar");
    for (name, v) in scripts {
        // Composer's events are all `pre-…` and `post-…`; `dev` is what Dev runs.
        if name == "dev" || name.starts_with("pre-") || name.starts_with("post-") {
            continue;
        }
        let text = match v {
            serde_json::Value::String(t) => t.clone(),
            serde_json::Value::Array(a) => a
                .iter()
                .filter_map(|x| x.as_str())
                .collect::<Vec<_>>()
                .join(" && "),
            _ => continue,
        };
        out.push(Script {
            id: format!("composer-{}", process_id(name)),
            run: format!(
                "{} {} run-script {}",
                quoted(&php.display().to_string()),
                quoted(&phar.display().to_string()),
                quoted(name)
            ),
            text,
            source: "composer.json",
            name: name.clone(),
        });
    }
    out
}

/// Names become file names and ids: letters, digits, `-` and `_`.
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 40
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Concurrently's options that take a value as the next word.
const VALUE_FLAGS: [&str; 20] = [
    "-c",
    "--prefix-colors",
    "-n",
    "--names",
    "-p",
    "--prefix",
    "-l",
    "--prefix-length",
    "-t",
    "--timestamp-format",
    "-m",
    "--max-processes",
    "--restart-tries",
    "--restart-after",
    "-s",
    "--success",
    "--hide",
    "--name-separator",
    "--default-input-target",
    "--pad-prefix",
];

/// The named commands of `npx concurrently "a" "b" --names=x,y`, without
/// `php artisan serve`: Anvil already serves the site. `None` if it isn't one.
pub fn concurrently(script: &str) -> Option<Vec<(String, String)>> {
    let words = shell_words(script);
    let at = words.iter().position(|w| w == "concurrently")?;
    let (mut commands, mut names) = (vec![], vec![]);
    let mut rest = words[at + 1..].iter();
    while let Some(w) = rest.next() {
        // `concurrently … && echo done`: what follows isn't concurrently's.
        if matches!(w.as_str(), "&&" | "||" | ";" | "|" | "&") {
            break;
        }
        if let Some(flag) = w.strip_prefix('-').map(|_| w.as_str()) {
            let (flag, inline) = match flag.split_once('=') {
                Some((f, v)) => (f, Some(v.to_string())),
                None => (flag, None),
            };
            // `-kc red,blue`: grouped short flags take a value if the last one does.
            let grouped = !flag.starts_with("--") && flag.len() > 2;
            let takes = VALUE_FLAGS.contains(&flag)
                || (grouped && VALUE_FLAGS.contains(&&*format!("-{}", &flag[flag.len() - 1..])));
            let value = if takes {
                inline.or_else(|| rest.next().cloned())
            } else {
                None
            };
            if matches!(flag, "-n" | "--names") {
                names = value
                    .unwrap_or_default()
                    .split(',')
                    .map(str::to_string)
                    .collect();
            }
            continue;
        }
        // "npm:dev" is concurrently's shorthand for `npm run dev`.
        let cmd = match w.strip_prefix("npm:") {
            Some(script) => format!("npm run {script}"),
            None => w.trim_start_matches('@').to_string(),
        };
        commands.push(cmd);
    }
    let out: Vec<(String, String)> = commands
        .into_iter()
        .enumerate()
        .map(|(i, cmd)| {
            let name = names
                .get(i)
                .map(|n| n.trim().to_lowercase())
                .filter(|n| valid_name(n))
                .unwrap_or_else(|| format!("process-{}", i + 1));
            (name, cmd)
        })
        // Any form of `php … artisan serve`: Anvil is the server.
        .filter(|(_, cmd)| {
            !(cmd.starts_with("php")
                && cmd
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .windows(2)
                    .any(|w| w == ["artisan", "serve"]))
        })
        .collect();
    (!out.is_empty()).then_some(out)
}

/// Split like a POSIX shell: spaces separate words, quotes group, `\` escapes.
fn shell_words(s: &str) -> Vec<String> {
    let (mut words, mut cur, mut any) = (vec![], String::new(), false);
    let mut quote: Option<char> = None;
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('"'), '\\') => cur.extend(chars.next()),
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                any = true;
            }
            (None, '\\') => cur.extend(chars.next()),
            (None, c) if c.is_whitespace() => {
                if any || !cur.is_empty() {
                    words.push(std::mem::take(&mut cur));
                }
                any = false;
            }
            (None, c) => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        words.push(cur);
    }
    words
}

/// What makes a process without a terminal print colors anyway: chalk/picocolors
/// (Vite, concurrently), Symfony Console (artisan), and BSD/GNU tools. Truecolor so
/// hex colors stay hex.
pub const COLOR_ENV: [(&str, &str); 4] = [
    ("FORCE_COLOR", "1"),
    ("CLICOLOR_FORCE", "1"),
    ("COLORTERM", "truecolor"),
    ("TERM", "xterm-256color"),
];

/// `php artisan <cmd> …` with `--ansi` (a global Symfony Console option every command
/// takes), unless it says `--ansi`/`--no-ansi`, ends its options with `--`, or is more
/// than one command (the flag would land on the last one).
pub fn with_ansi(command: &str) -> String {
    let words = shell_words(command);
    let artisan =
        words.len() > 2 && words[0].rsplit('/').next() == Some("php") && words[1] == "artisan";
    let plain = !command.contains([';', '|', '&', '\n']);
    let free = !words
        .iter()
        .any(|w| matches!(w.as_str(), "--ansi" | "--no-ansi" | "--"));
    if artisan && plain && free {
        format!("{command} --ansi")
    } else {
        command.to_string()
    }
}

/// Key for the process manager, pid and log files: `<site>@<process>`. Site
/// names never contain `@`, so a key can't be another app's name.
pub fn key(site: &str, process: &str) -> String {
    format!("{site}@{process}")
}

pub fn log_file(site: &str, process: &str) -> PathBuf {
    apps::log_file(&key(site, process))
}

/// Key of a package.json command run once (`Apps::run_once`): `<site>#<id>`.
pub fn once_key(site: &str, script: &str) -> String {
    format!("{site}#{}", process_id(script))
}

/// Where an extra dev server row answers: its host, with the app's scheme and public port.
pub fn address(cfg: &crate::Config, site: &Site, id: &str) -> Option<String> {
    let row = server_rows(cfg, site).into_iter().find(|r| r.id == id)?;
    Some(crate::site_url(
        cfg,
        &Site {
            host: row.host,
            ..site.clone()
        },
    ))
}

/// Anvil's PHP for the app, its Node.js and the package managers installed from
/// Settings, first on PATH, with Node's caches kept in Anvil's folder.
pub fn env(site: &Site) -> Vec<(String, String)> {
    let mut env = vec![];
    let mut path = std::env::var("PATH").unwrap_or_default();
    // The project's .nvmrc / .node-version when installed, else the newest.
    if let Some(node) = apps::runtime_version("node", &site.path) {
        env = runtimes::env("node", &node);
        if let Some(i) = env.iter().position(|(k, _)| k == "PATH") {
            path = env.remove(i).1;
        }
    } else {
        // No Anvil Node.js: bun installed from Settings still works.
        path = runtimes::with_pms(&path);
    }
    if let Some(v) = &site.php {
        path = format!("{}:{path}", home().join("php").join(v).display());
    }
    env.push(("PATH".into(), path));
    env
}

#[cfg(test)]
mod tests {
    use super::*;

    const LARAVEL: &str = r##"npx concurrently -c "#93c5fd,#c4b5fd,#fb7185,#fdba74" "php artisan serve --host=localhost" "php artisan queue:listen --tries=1 --timeout=0" "php artisan pail --timeout=0" "npm run dev" --names=server,queue,logs,vite --kill-others"##;

    #[test]
    fn splits_laravels_dev_script() {
        let p = concurrently(LARAVEL).unwrap();
        assert_eq!(
            p,
            [
                (
                    "queue".into(),
                    "php artisan queue:listen --tries=1 --timeout=0".into()
                ),
                ("logs".into(), "php artisan pail --timeout=0".into()),
                ("vite".into(), "npm run dev".into()),
            ]
        );
    }

    #[test]
    fn names_flags_and_shorthands() {
        let p = concurrently(r#"concurrently -n "Web,Worker" -k "npm:dev" '@php artisan horizon'"#)
            .unwrap();
        assert_eq!(
            p,
            [
                ("web".into(), "npm run dev".into()),
                ("worker".into(), "php artisan horizon".into()),
            ]
        );
        // Missing or odd names get a numbered one.
        let p = concurrently(r#"concurrently "a" "b" --names=ok,"no way""#).unwrap();
        assert_eq!(p[0].0, "ok");
        assert_eq!(p[1].0, "process-2");
        assert!(concurrently("@php artisan dev").is_none());
        // Shell operators end the list; grouped flags take their value; any serve is dropped.
        let p = concurrently(r#"concurrently -kc red,blue "php -d x=1 artisan serve" "npm run dev" -n a,b && echo done"#).unwrap();
        assert_eq!(p, [("b".into(), "npm run dev".into())]);
        assert!(concurrently("concurrently \"php artisan serve\"").is_none());
    }

    #[test]
    fn shell_quoting() {
        assert_eq!(
            shell_words(r#"a "b c" 'd "e"' f\ g """#),
            ["a", "b c", r#"d "e""#, "f g", ""]
        );
    }

    #[test]
    fn package_scripts_split_servers_from_commands() {
        // vuefinder's real scripts.
        let pkg = r#"{"scripts": {
    "dev": "vite",
    "build": "run-s \"build-only {@}\" type-gen --",
    "preview": "vite preview",
    "test:unit": "vitest",
    "prepare": "cypress install",
    "prepack": "npm run build",
    "test:e2e": "start-server-and-test preview http://localhost:4173 'cypress run --e2e'",
    "test:e2e:dev": "start-server-and-test 'vite dev --port 4173' http://localhost:4173 'cypress open --e2e'",
    "build-only": "vite build",
    "type-check": "vue-tsc --noEmit -p tsconfig.app.json",
    "type-gen": "vue-tsc -p tsconfig.app.json",
    "lint": "oxlint",
    "lint:fix": "oxlint --fix",
    "format": "prettier --write .",
    "format:check": "prettier --check .",
    "docs:dev": "run-s build-only docs:serve",
    "docs:serve": "vitepress dev docs",
    "docs:build": "npm run build && vitepress build docs",
    "docs:preview": "vitepress preview docs"
  }}"#;
        let dir = std::env::temp_dir().join(format!("anvil-pkg-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("package.json"), pkg).unwrap();
        let (servers, commands) = package_scripts(&dir);
        fs::remove_dir_all(&dir).unwrap();
        let names: Vec<_> = servers.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["dev", "docs:dev"]);
        assert_eq!(servers[1].id, "docs-dev");
        assert_eq!(
            servers[1].command,
            "npm run build-only && npm run docs:serve -- --port $PORT --strictPort --host 127.0.0.1"
        );
        assert!(
            servers
                .iter()
                .all(|p| p.server && p.source == "package.json")
        );
        for c in [
            "build",
            "test:unit",
            "lint",
            "format",
            "type-check",
            "preview",
            "docs:serve",
            "docs:preview",
            "test:e2e",
            "test:e2e:dev",
        ] {
            assert!(commands.iter().any(|n| n == c), "{c} should be a command");
        }
        assert!(!commands.iter().any(|n| n == "prepare" || n == "prepack"));
        assert_eq!(process_id("test:e2e:dev"), "test-e2e-dev");
        let mut ids: Vec<_> = servers
            .iter()
            .map(|p| p.name.as_str())
            .chain(commands.iter().map(String::as_str))
            .map(process_id)
            .collect();
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n);
        assert!(is_server_script("web", "npx vite --port 3000"));
        assert!(!is_server_script("web", "vite build"));
        assert!(is_server_script("sb", "pnpm exec storybook dev -p 6006"));
    }

    fn site(dir: &std::path::Path, kind: &str) -> Site {
        Site {
            name: "shop".into(),
            host: String::new(),
            secure: false,
            path: dir.into(),
            root: dir.into(),
            php: Some("8.4".into()),
            php_pinned: false,
            ini: Default::default(),
            linked: false,
            parked_in: None,
            modified: 0,
            kind: kind.into(),
            kind_detected: kind.into(),
            command: None,
            command_custom: false,
            port: None,
            autostart: false,
            runtime_version: None,
            runtime_pinned: false,
        }
    }

    #[test]
    fn vite_plus_dev_is_pinned_like_vite() {
        let flags = Some("--port $PORT --strictPort --host 127.0.0.1");
        assert_eq!(pin_flags("vp dev"), flags);
        assert_eq!(pin_flags("vp dev docs"), flags);
        assert_eq!(pin_flags("pnpm exec vp dev"), flags);
        assert_eq!(pin_flags("vite"), flags);
        assert_eq!(pin_flags("vp build"), None);
        assert_eq!(pin_flags("vp dev && echo"), None);
        assert!(is_server_script("docs", "vp dev docs"));
        assert!(!is_server_script("check", "vp lint"));
        assert_eq!(
            start_command("docs", "vp dev docs", &serde_json::Map::new()),
            "npm run docs -- --port $PORT --strictPort --host 127.0.0.1"
        );
    }

    #[test]
    fn laravels_vite_keeps_its_port_and_shows_none() {
        // laravel-vite-plugin writes the URL vite really took to public/hot, so a
        // collision is harmless; Anvil neither forces a port nor claims one.
        let dir = std::env::temp_dir().join(format!("anvil-lvite-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let composer = serde_json::json!({ "scripts": { "dev": [LARAVEL] } });
        fs::write(dir.join("composer.json"), composer.to_string()).unwrap();
        fs::write(dir.join("package.json"), r#"{"scripts":{"dev":"vp dev"}}"#).unwrap();
        let rows = detect(&site(&dir, "laravel"));
        fs::remove_dir_all(&dir).unwrap();
        let vite = rows.iter().find(|p| p.name == "vite").unwrap();
        assert_eq!((vite.command.as_str(), vite.port), ("npm run dev", None));
    }

    #[test]
    fn project_file_comes_first_then_dev_scripts() {
        let dir = std::env::temp_dir().join(format!("anvil-proc-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let composer = serde_json::json!({ "scripts": { "dev": ["Composer\\Config::disableProcessTimeout", LARAVEL] } });
        fs::write(dir.join("composer.json"), composer.to_string()).unwrap();
        fs::write(dir.join("package.json"), r#"{"scripts":{"dev":"vite"}}"#).unwrap();
        let names = |s: &Site| detect(s).into_iter().map(|p| p.name).collect::<Vec<_>>();
        assert_eq!(names(&site(&dir, "laravel")), ["queue", "logs", "vite"]);
        // Not a PHP app: its own dev server is the app itself.
        assert!(detect(&site(&dir, "next")).is_empty());
        // `php artisan dev` can't be split: Vite from package.json.
        fs::write(
            dir.join("composer.json"),
            r#"{"scripts":{"dev":["@php artisan dev"]}}"#,
        )
        .unwrap();
        assert_eq!(detect(&site(&dir, "laravel"))[0].command, "npm run dev");
        // .anvil.toml wins, for any kind; bad names are skipped.
        fs::write(
            dir.join(".anvil.toml"),
            "[processes]\nworker = \"node worker.js\"\n\"bad name\" = \"x\"\n",
        )
        .unwrap();
        let p = detect(&site(&dir, "next"));
        assert_eq!((p.len(), p[0].source), (1, ".anvil.toml"));
        fs::remove_dir_all(dir).unwrap();
    }

    /// A `php` that ignores its arguments and runs `body`, in its own temp dir.
    fn fake_php(tag: &str, body: &str) -> (PathBuf, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("anvil-lv-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("app/vendor/laravel/framework/src/Illuminate/Foundation"))
            .unwrap();
        fs::write(root.join("app").join(DEV_COMMANDS), "<?php").unwrap();
        let php = root.join("php");
        fs::write(&php, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&php, fs::Permissions::from_mode(0o755)).unwrap();
        (root.join("app"), php)
    }

    const DEV_JSON: &str = r#"[{"name":"server","command":"php artisan serve"},{"name":"queue","command":"php artisan queue:listen --tries=1 --timeout=0"},{"name":"logs","command":"php artisan pail --timeout=0"},{"name":"vite","command":"pnpm run dev"}]"#;

    #[test]
    fn laravel_list_drops_server() {
        let (dir, php) = fake_php("ok", &format!("echo '{DEV_JSON}'"));
        let want: Vec<(String, String)> = vec![
            (
                "queue".into(),
                "php artisan queue:listen --tries=1 --timeout=0".into(),
            ),
            ("logs".into(), "php artisan pail --timeout=0".into()),
            ("vite".into(), "pnpm run dev".into()),
        ];
        assert_eq!(laravel_dev_commands(&dir, &php), Some(want.clone()));
        // Cached: a second ask doesn't need PHP, and detect uses it as the second source.
        let cache = dir.parent().unwrap().join("cache");
        let mut s = site(&dir, "laravel");
        s.php = Some("8.4".into());
        let names = |s: &Site| detect_in(s, &php, &cache, Some(&Default::default()));
        let p = names(&s);
        assert_eq!(
            p.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
            ["queue", "logs", "vite"]
        );
        assert_eq!(
            (p[0].source, p[0].server, p[0].id.as_str()),
            ("artisan dev", false, "queue")
        );
        // A PHP that would now fail (its mtime kept, so it counts as the same PHP).
        let modified = fs::metadata(&php).unwrap().modified().unwrap();
        fs::write(&php, "#!/bin/sh\nexit 1\n").unwrap();
        let f = fs::File::options().write(true).open(&php).unwrap();
        f.set_modified(modified).unwrap();
        drop(f);
        assert_eq!(laravel_dev_commands_cached(&dir, &php, &cache), Some(want));
        // .anvil.toml still comes first.
        fs::write(dir.join(".anvil.toml"), "[processes]\nw = \"x\"\n").unwrap();
        assert_eq!(
            detect_in(&s, &php, &cache, Some(&Default::default()))[0].source,
            ".anvil.toml"
        );
        fs::remove_dir_all(dir.parent().unwrap()).unwrap();
    }

    #[test]
    fn laravel_list_falls_back() {
        let (dir, php) = fake_php("bad", "exit 1");
        assert_eq!(laravel_dev_commands(&dir, &php), None);
        fs::write(&php, "#!/bin/sh\necho not json\n").unwrap();
        assert_eq!(laravel_dev_commands(&dir, &php), None);
        fs::write(&php, "#!/bin/sh\nexec sleep 10\n").unwrap();
        let t = std::time::Instant::now();
        assert_eq!(laravel_dev_commands(&dir, &php), None);
        assert!(t.elapsed().as_secs() < 6);
        // No DevCommands.php: PHP isn't run at all.
        let marker = dir.parent().unwrap().join("ran");
        fs::write(&php, format!("#!/bin/sh\ntouch {}\n", marker.display())).unwrap();
        fs::remove_file(dir.join(DEV_COMMANDS)).unwrap();
        assert_eq!(laravel_dev_commands(&dir, &php), None);
        assert!(!marker.exists());
        fs::remove_dir_all(dir.parent().unwrap()).unwrap();
    }

    #[test]
    fn laravel_failure_is_cached() {
        let (dir, php) = fake_php("cnt", "");
        let root = dir.parent().unwrap().to_path_buf();
        let log = root.join("runs");
        fs::write(
            &php,
            format!("#!/bin/sh\necho x >> {}\nexit 1\n", log.display()),
        )
        .unwrap();
        let cache = root.join("cache");
        let runs = || fs::read_to_string(&log).map_or(0, |t| t.lines().count());
        assert_eq!(laravel_dev_commands_cached(&dir, &php, &cache), None);
        assert_eq!(laravel_dev_commands_cached(&dir, &php, &cache), None);
        assert_eq!(runs(), 1);
        // Another PHP binary retries.
        let other = root.join("php2");
        fs::copy(&php, &other).unwrap();
        assert_eq!(laravel_dev_commands_cached(&dir, &other, &cache), None);
        assert_eq!(runs(), 2);
        // A failure older than RETRY_FAILED is asked again.
        let file = fs::read_dir(&cache)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let mut c: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        c["at"] = (c["at"].as_u64().unwrap() - RETRY_FAILED).into();
        fs::write(&file, c.to_string()).unwrap();
        assert_eq!(laravel_dev_commands_cached(&dir, &other, &cache), None);
        assert_eq!(runs(), 3);
        assert_eq!(laravel_dev_commands_cached(&dir, &other, &cache), None);
        assert_eq!(runs(), 3);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn laravel_success_is_cached_until_a_provider_changes() {
        let (dir, php) = fake_php("prov", "");
        let root = dir.parent().unwrap().to_path_buf();
        let log = root.join("runs");
        fs::write(
            &php,
            format!(
                "#!/bin/sh
echo x >> {}
echo '{DEV_JSON}'
",
                log.display()
            ),
        )
        .unwrap();
        let provider = dir.join("app/Providers/AppServiceProvider.php");
        fs::create_dir_all(provider.parent().unwrap()).unwrap();
        fs::write(&provider, "<?php").unwrap();
        let cache = root.join("cache");
        let runs = || fs::read_to_string(&log).map_or(0, |t| t.lines().count());
        assert!(laravel_dev_commands_cached(&dir, &php, &cache).is_some());
        // Even an old success is kept.
        let file = fs::read_dir(&cache)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let mut c: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        c["at"] = 0.into();
        fs::write(&file, c.to_string()).unwrap();
        assert!(laravel_dev_commands_cached(&dir, &php, &cache).is_some());
        assert_eq!(runs(), 1);
        // Editing a provider (the folder's own mtime stays) asks again.
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(60);
        fs::File::options()
            .write(true)
            .open(&provider)
            .unwrap()
            .set_modified(later)
            .unwrap();
        assert!(laravel_dev_commands_cached(&dir, &php, &cache).is_some());
        assert_eq!(runs(), 2);
        // So does bootstrap/providers.php.
        let bootstrap = dir.join("bootstrap/providers.php");
        fs::create_dir_all(bootstrap.parent().unwrap()).unwrap();
        fs::write(&bootstrap, "<?php").unwrap();
        fs::File::options()
            .write(true)
            .open(&bootstrap)
            .unwrap()
            .set_modified(later + std::time::Duration::from_secs(60))
            .unwrap();
        assert!(laravel_dev_commands_cached(&dir, &php, &cache).is_some());
        assert_eq!(runs(), 3);
        fs::remove_dir_all(root).unwrap();
    }

    fn vite_site(tag: &str, name: &str, scripts: &str) -> Site {
        let dir = std::env::temp_dir().join(format!("anvil-rows-{tag}-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("package.json"),
            format!(r#"{{"scripts":{scripts}}}"#),
        )
        .unwrap();
        let mut s = site(&dir, "vite");
        s.name = name.into();
        s.host = format!("{name}.test");
        s
    }

    #[test]
    fn extra_servers_get_stable_distinct_ports() {
        let scripts =
            r#"{"dev":"vite","docs:dev":"vitepress dev docs","docs:site":"vitepress dev docs"}"#;
        let (a, b) = (
            vite_site("a", "vuefinder", scripts),
            vite_site("b", "other", scripts),
        );
        let mut cfg = crate::Config::default();
        cfg.services.entry("redis".into()).or_default().port = Some(46000);
        let first = server_rows(&cfg, &a)[0].port;
        cfg.sites.entry("pinned".into()).or_default().port = Some(first);
        assert_ne!(server_rows(&cfg, &a)[0].port, first);
        cfg.sites.clear();
        let ra = server_rows(&cfg, &a);
        // `dev` is the app itself; the first `docs` keeps the short host.
        let hosts: Vec<_> = ra.iter().map(|r| r.host.as_str()).collect();
        assert_eq!(hosts, ["docs.vuefinder.test", "docs-site.vuefinder.test"]);
        assert_eq!(ra[0].id, "docs-dev");
        let again = server_rows(&cfg, &a);
        assert_eq!(
            ra.iter().map(|r| r.port).collect::<Vec<_>>(),
            again.iter().map(|r| r.port).collect::<Vec<_>>()
        );
        let rb = server_rows(&cfg, &b);
        let ports: Vec<u16> = ra.iter().chain(&rb).map(|r| r.port).collect();
        for p in &ports {
            assert!(![cfg.http_port, cfg.https_port, 46000].contains(p));
            assert!((45000..49000).contains(p));
        }
        // Rows of one app never share a port.
        assert_ne!(ra[0].port, ra[1].port);
        // A taken port is skipped, whatever the hash says.
        let want = apps::hashed_port("vuefinder@docs-dev", 45000, &[], &cfg);
        assert_eq!(want, ra[0].port);
        assert_ne!(
            apps::hashed_port("vuefinder@docs-dev", 45000, &[want], &cfg),
            want
        );
        // The detected process carries the port and the vitepress flags.
        let p = detect_in(&a, Path::new("php"), Path::new("cache"), Some(&cfg));
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].port, Some(ra[0].port));
        assert_eq!(
            p[0].command,
            "npm run docs:dev -- --port $PORT --strictPort --host 127.0.0.1"
        );
        for s in [a, b] {
            fs::remove_dir_all(s.path).unwrap();
        }
    }

    #[test]
    fn row_address_uses_its_host_and_the_apps_scheme() {
        let mut s = vite_site(
            "addr",
            "vuefinder",
            r#"{"dev":"vite","docs:dev":"vitepress dev docs"}"#,
        );
        // Ports nothing forwards, so they are written out.
        let cfg = crate::Config {
            http_port: 18080,
            https_port: 18443,
            ..Default::default()
        };
        assert_eq!(
            address(&cfg, &s, "docs-dev").as_deref(),
            Some("http://docs.vuefinder.test:18080")
        );
        s.secure = true;
        assert_eq!(
            address(&cfg, &s, "docs-dev").as_deref(),
            Some("https://docs.vuefinder.test:18443")
        );
        // The app's own `dev` and unknown ids have no row address.
        assert_eq!(address(&cfg, &s, "dev"), None);
        assert_eq!(address(&cfg, &s, "nope"), None);
        fs::remove_dir_all(&s.path).unwrap();
    }

    #[test]
    fn scripts_that_wrap_the_server_are_not_pinned() {
        let s = vite_site("w", "wrap", r#"{"docs:dev":"run-s build-only docs:serve"}"#);
        let p = detect_in(
            &s,
            Path::new("php"),
            Path::new("cache"),
            Some(&Default::default()),
        );
        fs::remove_dir_all(&s.path).unwrap();
        assert_eq!(p[0].command, "npm run docs:dev");
        assert!(p[0].port.is_some());
    }

    #[test]
    fn vuefinder_docs_runs_its_chain_with_the_port() {
        let pkg = r#"{"dev":"vite","build-only":"vite build","docs:dev":"run-s build-only docs:serve","docs:serve":"vitepress dev docs","a:dev":"astro dev",":x:dev":"vite"}"#;
        let s = vite_site("v", "vuefinder", pkg);
        let p = detect_in(
            &s,
            Path::new("php"),
            Path::new("cache"),
            Some(&Default::default()),
        );
        let cmd = |n: &str| p.iter().find(|p| p.name == n).unwrap().command.clone();
        assert_eq!(
            cmd("docs:dev"),
            "npm run build-only && npm run docs:serve -- --port $PORT --strictPort --host 127.0.0.1"
        );
        assert_eq!(
            cmd("a:dev"),
            "npm run a:dev -- --port $PORT --host 127.0.0.1"
        );
        // An empty cut prefix falls back to the full id.
        let rows = server_rows(&Default::default(), &s);
        assert_eq!(
            rows.iter().find(|r| r.id == "-x-dev").unwrap().host,
            "-x-dev.vuefinder.test"
        );
        // .anvil.toml wins entirely.
        fs::write(s.path.join(".anvil.toml"), "[processes]\nw = \"x\"\n").unwrap();
        assert!(server_rows(&Default::default(), &s).is_empty());
        let p = detect_in(
            &s,
            Path::new("php"),
            Path::new("cache"),
            Some(&Default::default()),
        );
        assert_eq!(p.len(), 1);
        fs::remove_dir_all(&s.path).unwrap();
    }

    fn settings(cfg: &mut crate::Config, site: &str, id: &str, change: ProcessChange) {
        apply_process_change(cfg.sites.entry(site.into()).or_default(), id, change).unwrap();
    }

    #[test]
    fn hidden_server_rows_lose_their_route() {
        let scripts =
            r#"{"dev":"vite","docs:dev":"vitepress dev docs","docs:site":"vitepress dev docs"}"#;
        let s = vite_site("hide", "vuefinder", scripts);
        let mut cfg = crate::Config::default();
        let before = server_rows(&cfg, &s);
        let hide = ProcessChange {
            hidden: Some(true),
            ..Default::default()
        };
        settings(&mut cfg, "vuefinder", &process_id("docs:dev"), hide);
        let after = server_rows(&cfg, &s);
        assert_eq!(after.len(), 1);
        // The other row keeps its port and host.
        let kept = before.iter().find(|r| r.id == after[0].id).unwrap();
        assert_eq!(after[0].id, process_id("docs:site"));
        assert_eq!((after[0].port, &after[0].host), (kept.port, &kept.host));
    }

    #[test]
    fn a_row_address_can_be_edited_and_reset() {
        let scripts =
            r#"{"dev":"vite","docs:dev":"vitepress dev docs","docs:site":"vitepress dev docs"}"#;
        let s = vite_site("host", "vuefinder", scripts);
        let mut cfg = crate::Config {
            http_port: 18080,
            ..Default::default()
        };
        let before = server_rows(&cfg, &s);
        let host = |label: Option<&str>| ProcessChange {
            host: Some(label.map(String::from)),
            ..Default::default()
        };
        change_process(&mut cfg, &s, "docs-dev", host(Some("API-Docs"))).unwrap();
        assert_eq!(
            cfg.sites["vuefinder"].processes["docs-dev"].host.as_deref(),
            Some("api-docs")
        );
        let after = server_rows(&cfg, &s);
        assert_eq!(after[0].host, "api-docs.vuefinder.test");
        assert_eq!(
            address(&cfg, &s, "docs-dev").as_deref(),
            Some("http://api-docs.vuefinder.test:18080")
        );
        // The other row keeps its host, and every row its port.
        assert_eq!(after[1].host, before[1].host);
        let ports = |r: &[ServerRow]| r.iter().map(|r| r.port).collect::<Vec<_>>();
        assert_eq!(ports(&after), ports(&before));
        // Invalid labels and one another row uses are rejected, and change nothing.
        for bad in [
            "",
            "-a",
            "a-",
            "a.b",
            "a_b",
            "ü",
            &"a".repeat(64),
            "docs-site",
        ] {
            assert!(
                change_process(&mut cfg, &s, "docs-dev", host(Some(bad))).is_err(),
                "{bad}"
            );
        }
        assert!(change_process(&mut cfg, &s, "docs-site", host(Some("api-docs"))).is_err());
        assert_eq!(server_rows(&cfg, &s)[0].host, "api-docs.vuefinder.test");
        // A row without an address has no address to edit.
        assert!(change_process(&mut cfg, &s, "dev", host(Some("x"))).is_err());
        // Back to automatic: the entry is gone.
        change_process(&mut cfg, &s, "docs-dev", host(None)).unwrap();
        assert_eq!(server_rows(&cfg, &s)[0].host, before[0].host);
        assert!(cfg.sites["vuefinder"].processes.is_empty());
        fs::remove_dir_all(&s.path).unwrap();
    }

    #[test]
    fn rejected_settings_leave_no_empty_app_entry() {
        let mut cfg = crate::Config::default();
        let shop = site(Path::new("/nonexistent"), "laravel");
        assert!(set_process(&mut cfg, &shop, "", ProcessChange::default()).is_err());
        let empty = ProcessChange {
            command: Some(Some("  ".into())),
            ..Default::default()
        };
        assert!(set_process(&mut cfg, &shop, "queue", empty).is_err());
        assert!(cfg.sites.is_empty());
    }

    #[test]
    fn process_settings_shape_the_rows() {
        // A Laravel app: hide `logs`, edit `queue`, autostart `vite`.
        let (dir, php) = fake_php("set", &format!("echo '{DEV_JSON}'"));
        let cache = dir.parent().unwrap().join("cache");
        let l = site(&dir, "laravel");
        let mut cfg = crate::Config::default();
        let name = |p: &Process| p.name.clone();
        settings(
            &mut cfg,
            "shop",
            "logs",
            ProcessChange {
                hidden: Some(true),
                ..Default::default()
            },
        );
        settings(
            &mut cfg,
            "shop",
            "queue",
            ProcessChange {
                command: Some(Some("php artisan queue:work".into())),
                ..Default::default()
            },
        );
        settings(
            &mut cfg,
            "shop",
            "vite",
            ProcessChange {
                autostart: Some(true),
                ..Default::default()
            },
        );
        let p = detect_in(&l, &php, &cache, Some(&cfg));
        assert_eq!(p.iter().map(name).collect::<Vec<_>>(), ["queue", "vite"]);
        assert_eq!(p[0].command, "php artisan queue:work");
        assert!(p[0].edited && !p[0].autostart);
        assert!(p[1].autostart && !p[1].edited);
        assert_eq!(hidden(&cfg.sites["shop"]), ["logs"]);
        // Resetting the command and unhiding brings the detected row back.
        let reset = ProcessChange {
            command: Some(None),
            hidden: Some(false),
            ..Default::default()
        };
        settings(&mut cfg, "shop", "queue", reset);
        settings(
            &mut cfg,
            "shop",
            "logs",
            ProcessChange {
                hidden: Some(false),
                ..Default::default()
            },
        );
        let p = detect_in(&l, &php, &cache, Some(&cfg));
        assert_eq!(p.len(), 3);
        assert!(!p[0].edited);
        // An entry back at its defaults is removed.
        assert!(!cfg.sites["shop"].processes.contains_key("logs"));
        assert!(!cfg.sites["shop"].processes.contains_key("queue"));
        fs::remove_dir_all(dir.parent().unwrap()).unwrap();

        // A pinned command script becomes a row after the detected ones.
        let pkg = r#"{"dev":"vite","build-only":"vite build","docs:dev":"run-s build-only docs:serve","docs:serve":"vitepress dev docs","docs:build":"npm run build && vitepress build docs","lint":"oxlint"}"#;
        let s = vite_site("pin", "vuefinder", pkg);
        let mut cfg = crate::Config::default();
        let pin = ProcessChange {
            pinned: Some(true),
            ..Default::default()
        };
        settings(&mut cfg, "vuefinder", "docs-serve", pin);
        let pin = ProcessChange {
            pinned: Some(true),
            ..Default::default()
        };
        settings(&mut cfg, "vuefinder", "lint", pin);
        let pin = ProcessChange {
            pinned: Some(true),
            ..Default::default()
        };
        settings(&mut cfg, "vuefinder", "gone", pin);
        let p = detect_in(&s, Path::new("php"), Path::new("cache"), Some(&cfg));
        let ids: Vec<_> = p.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["docs-dev", "docs-serve", "lint"]);
        let serve = &p[1];
        assert!(serve.pinned && !p[0].pinned);
        assert_eq!((serve.source, serve.server), ("package.json", false));
        assert_eq!(
            serve.command,
            "npm run docs:serve -- --port $PORT --strictPort --host 127.0.0.1"
        );
        assert!(serve.port.is_some());
        assert!(address(&cfg, &s, "docs-serve").is_some());
        // A command that doesn't serve has no port or address.
        assert_eq!((p[2].command.as_str(), p[2].port), ("npm run lint", None));
        assert!(address(&cfg, &s, "lint").is_none());
        fs::remove_dir_all(&s.path).unwrap();

        // Validation: no empty command, no id for the app's own row.
        let mut sc = crate::SiteConfig::default();
        for command in ["", "  "] {
            let c = ProcessChange {
                command: Some(Some(command.into())),
                ..Default::default()
            };
            assert!(apply_process_change(&mut sc, "queue", c).is_err());
        }
        let c = ProcessChange {
            autostart: Some(true),
            ..Default::default()
        };
        assert!(apply_process_change(&mut sc, "", c).is_err());
        assert!(sc.processes.is_empty());
    }

    #[test]
    fn keep_on_top_is_stored_and_cleared() {
        let mut sc = crate::SiteConfig::default();
        let top = |on| ProcessChange {
            top: Some(on),
            ..Default::default()
        };
        apply_process_change(&mut sc, "queue", top(true)).unwrap();
        assert!(sc.processes["queue"].top);
        let saved = toml::to_string(&sc).unwrap();
        assert!(saved.contains("top = true"), "{saved}");
        apply_process_change(&mut sc, "queue", top(false)).unwrap();
        assert!(sc.processes.is_empty());
    }

    #[test]
    fn artisan_commands_get_ansi() {
        let ansi = |c: &str| with_ansi(c);
        assert_eq!(
            ansi("php artisan queue:listen --tries=1 --timeout=0"),
            "php artisan queue:listen --tries=1 --timeout=0 --ansi"
        );
        assert_eq!(
            ansi("php artisan pail --timeout=0"),
            "php artisan pail --timeout=0 --ansi"
        );
        assert_eq!(
            ansi("php artisan serve --port=$PORT"),
            "php artisan serve --port=$PORT --ansi"
        );
        assert_eq!(
            ansi("/x/php/8.4/php artisan serve --host=localhost"),
            "/x/php/8.4/php artisan serve --host=localhost --ansi"
        );
        for same in [
            "php artisan serve --ansi",
            "php artisan pail --no-ansi",
            "php artisan",
            "php artisan migrate && php artisan serve",
            "php artisan tinker -- x",
            "npm run dev",
            "php -S localhost:8000",
        ] {
            assert_eq!(ansi(same), same);
        }
    }

    #[test]
    fn server_rows_get_port_in_env() {
        let mut p = Process {
            id: "docs".into(),
            name: "docs".into(),
            command: "x".into(),
            source: "package.json",
            server: true,
            port: Some(45001),
            ..Default::default()
        };
        let site = site(Path::new("/s"), "vite");
        let env = apps::process_env(&site, &p, "test");
        assert!(env.contains(&("PORT".into(), "45001".into())));
        // Vite answers 403 to a host it doesn't know, e.g. docs.vuefinder.test.
        assert!(env.contains(&(
            "__VITE_ADDITIONAL_SERVER_ALLOWED_HOSTS".into(),
            ".test".into()
        )));
        p.port = None;
        assert!(
            !apps::process_env(&site, &p, "test")
                .iter()
                .any(|(k, _)| k == "PORT")
        );
    }

    #[test]
    fn configure_adds_leaves_out_removes_and_resets() {
        let (dir, php) = fake_php("conf", &format!("echo '{DEV_JSON}'"));
        let cache = dir.parent().unwrap().join("cache");
        let l = site(&dir, "laravel");
        let mut cfg = crate::Config::default();
        let ids = |cfg: &crate::Config| -> Vec<String> {
            detect_in(&l, &php, &cache, Some(cfg))
                .into_iter()
                .map(|p| p.id)
                .collect()
        };
        let taken = ids(&cfg);
        let s = cfg.sites.entry("shop".into()).or_default();
        // A new name, not a detected one nor a bad file name, with a command.
        add_entry(s, &taken, "reverb", "php artisan reverb:start").unwrap();
        assert!(add_entry(s, &taken, "queue", "x").is_err());
        assert!(add_entry(s, &taken, "reverb", "x").is_err());
        assert!(add_entry(s, &taken, "../x", "x").is_err());
        assert!(add_entry(s, &taken, "ok", "  ").is_err());
        let p = detect_in(&l, &php, &cache, Some(&cfg));
        let added = p.iter().find(|p| p.id == "reverb").unwrap();
        assert_eq!(
            (
                added.command.as_str(),
                added.added,
                added.edited,
                added.source
            ),
            ("php artisan reverb:start", true, false, "Anvil")
        );
        assert_eq!(ids(&cfg), ["queue", "logs", "vite", "reverb"]);

        // Dev start off for logs: still listed, marked manual.
        let manual = ProcessChange {
            manual: Some(true),
            ..Default::default()
        };
        settings(&mut cfg, "shop", "logs", manual);
        let p = detect_in(&l, &php, &cache, Some(&cfg));
        assert!(p.iter().find(|p| p.id == "logs").unwrap().manual);
        assert!(!p.iter().find(|p| p.id == "queue").unwrap().manual);

        // Removing: an added process is forgotten, a detected one hidden.
        let hide = || ProcessChange {
            hidden: Some(true),
            ..Default::default()
        };
        settings(&mut cfg, "shop", "reverb", hide());
        settings(&mut cfg, "shop", "vite", hide());
        assert!(!cfg.sites["shop"].processes.contains_key("reverb"));
        assert_eq!(ids(&cfg), ["queue", "logs"]);

        // Reset: back to the detected list; an address label and Keep on Top stay.
        settings(
            &mut cfg,
            "shop",
            "queue",
            ProcessChange {
                command: Some(Some("php artisan queue:work".into())),
                autostart: Some(true),
                top: Some(true),
                host: Some(Some("jobs".into())),
                ..Default::default()
            },
        );
        let s = cfg.sites.get_mut("shop").unwrap();
        add_entry(s, &taken, "extra", "sleep 1").unwrap();
        reset_entries(s);
        assert_eq!(ids(&cfg), ["queue", "logs", "vite"]);
        let left = &cfg.sites["shop"].processes;
        assert_eq!(left.keys().collect::<Vec<_>>(), ["queue"]);
        assert_eq!(
            left["queue"],
            crate::ProcessConfig {
                top: true,
                host: Some("jobs".into()),
                ..Default::default()
            }
        );
        let p = detect_in(&l, &php, &cache, Some(&cfg));
        assert!(p.iter().all(|p| !p.manual && !p.autostart && !p.edited));
        fs::remove_dir_all(dir.parent().unwrap()).unwrap();
    }

    #[test]
    fn one_off_scripts_are_package_commands_then_composer_scripts() {
        let dir = std::env::temp_dir().join(format!("anvil-oneoff-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("package.json"),
            r#"{"scripts":{"dev":"vite","build":"vite build","prebuild":"x","test:unit":"vitest"}}"#,
        )
        .unwrap();
        fs::write(
            dir.join("composer.json"),
            r#"{"scripts":{"dev":["npx concurrently a b"],"post-autoload-dump":["@php artisan package:discover"],"pre-install-cmd":"x","test":["@php artisan config:clear","@php artisan test"],"it's":"echo hi"}}"#,
        )
        .unwrap();
        let got = one_off_scripts(&site(&dir, "laravel"));
        let short: Vec<(&str, &str, &str, &str)> = got
            .iter()
            .map(|s| (s.name.as_str(), s.id.as_str(), s.text.as_str(), s.source))
            .collect();
        assert_eq!(
            short,
            [
                ("build", "build", "vite build", "package.json"),
                ("test:unit", "test-unit", "vitest", "package.json"),
                (
                    "test",
                    "composer-test",
                    "@php artisan config:clear && @php artisan test",
                    "composer.json"
                ),
                ("it's", "composer-it-s", "echo hi", "composer.json"),
            ]
        );
        assert_eq!(got[1].run, "npm run 'test:unit'");
        assert!(
            got[3].run.ends_with(r"run-script 'it'\''s'"),
            "{}",
            got[3].run
        );
        // Composer's scripts are a PHP app's only.
        assert_eq!(one_off_scripts(&site(&dir, "vite")).len(), 2);
        fs::remove_dir_all(dir).unwrap();
    }
}
