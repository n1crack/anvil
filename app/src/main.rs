// Hide the extra console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod control;

use anvil::validate::{process_id, site_name};
use anvil::{Config, Server, Site};
use serde::Serialize;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(target_os = "macos")]
use tauri::TitleBarStyle;
use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::TrayIconBuilder;
use tauri::{
    AppHandle, Emitter, Manager, RunEvent, State, WebviewUrl, WebviewWindowBuilder, WindowEvent,
    Wry,
};

/// Lock order: never hold two of these at once. Copy what one guards, drop it, then
/// take the next; `let x = m.lock()…;` drops the guard at the `;`, but an `if let` /
/// `match` scrutinee keeps it through the whole block. If two ever must nest, take
/// them in field order (restarting, server, error, services, apps, app_errors,
/// service_errors, container_logs, agent_log).
#[derive(Default)]
struct App {
    server: Mutex<Option<Server>>,
    /// Held through a whole `start`/`stop` of the web server, so two can't interleave;
    /// taken before `server`, which is only locked to swap.
    restarting: Mutex<()>,
    error: Mutex<Option<String>>,
    services: Mutex<anvil::services::Services>,
    apps: Mutex<anvil::apps::Apps>,
    /// Last start failure per app.
    app_errors: Mutex<std::collections::BTreeMap<String, String>>,
    /// Last start failure per service id.
    service_errors: Mutex<std::collections::BTreeMap<String, String>>,
    /// Worktrees whose own database is being made, with the line saying so.
    preparing: Mutex<std::collections::BTreeMap<String, String>>,
    /// The Containers tab's log streams.
    container_logs: Mutex<anvil::compose::Logs>,
    /// Set when Caddy or PHP-FPM ended on its own, until the watcher has notified.
    server_crashed: AtomicBool,
    /// The last actions taken through the control socket, oldest first.
    agent_log: Mutex<std::collections::VecDeque<control::AgentAction>>,
}

#[derive(Serialize)]
struct Snapshot {
    sites: Vec<Site>,
    /// Parked folders hidden from Anvil, for the list's "Show Hidden Applications".
    hidden: Vec<anvil::HiddenSite>,
    tld: String,
    https_port: u16,
    /// Port for addresses of sites without HTTPS.
    http_port: u16,
    php_default: Option<String>,
    php_installed: Vec<String>,
    editor: Option<String>,
    /// Terminal app for "Open in Terminal"; none means macOS Terminal.
    terminal: Option<String>,
    /// Git app used by "Open in git client", if any is installed.
    git_client: Option<String>,
    groups: std::collections::BTreeMap<String, Vec<String>>,
    /// php.ini defaults for every site.
    ini: std::collections::BTreeMap<String, String>,
    running: bool,
    error: Option<String>,
    /// Apps with their own server: "starting" until the port answers, then "running".
    app_states: std::collections::BTreeMap<String, &'static str>,
    app_errors: std::collections::BTreeMap<String, String>,
    kinds: Vec<(&'static str, &'static str)>,
    /// Parked folders, where new applications can be created.
    paths: Vec<std::path::PathBuf>,
    /// Installed language versions by runtime id, newest first.
    runtimes: std::collections::BTreeMap<&'static str, Vec<String>>,
    /// What each app's `.anvil.toml` asks for (PHP, Node.js, services), apps with one only.
    projects: std::collections::BTreeMap<String, Vec<anvil::project::Need>>,
    /// The Anvil Mailpit each app's .env sends mail to, apps that send there only.
    mail: std::collections::BTreeMap<String, String>,
    /// Each worktree's database and what its Overview warns about.
    worktrees: std::collections::BTreeMap<String, WorktreeView>,
    /// Database copies of worktrees that are gone (never dropped on their own).
    leftovers: Vec<anvil::worktrees::Leftover>,
    /// The extensions Anvil can add (`phpext::CATALOG`), for an app's Settings tab.
    php_ext_catalog: Vec<&'static str>,
    /// Extensions on for every app on a version (Settings › PHP), by version.
    php_ext: std::collections::BTreeMap<String, Vec<String>>,
    /// The catalog extensions built for each version that has some turned on: one on
    /// without a build loads nothing and shows as not built.
    php_ext_built: std::collections::BTreeMap<String, Vec<&'static str>>,
}

/// What a worktree's Overview says about it.
#[derive(Serialize)]
struct WorktreeView {
    db: anvil::worktrees::DbChoice,
    /// What it may choose now (`worktrees::choices`).
    choices: Vec<anvil::worktrees::DbChoice>,
    /// Its own database is made: the choice is then that one.
    prepared: bool,
    /// Its own database (a name, or a SQLite file in it); `None` when there's none to give it.
    database: Option<String>,
    /// Main's, as "copy of …" names it.
    main_database: Option<String>,
    /// Its own database is still to be made (Start, a terminal shim, Prepare database).
    pending: bool,
    /// "Anvil: preparing …" while a copy is made.
    preparing: Option<String>,
    compose: bool,
    /// `bootstrap/cache/config.php` exists: Laravel then ignores the env Anvil passes.
    cached_config: bool,
    /// Why nothing of it starts (`checked_env`).
    error: Option<String>,
    /// Databases Anvil made for it on a server, for Remove's "Also delete".
    copies: Vec<String>,
    /// Its database was there already and the user chose to use it.
    adopted: bool,
    /// What it lacks to run: the files its hooks copy, its packages (`worktrees::missing`).
    missing: anvil::worktrees::Missing,
    /// The hooks' `check` keys its env files still give main's value (`worktrees::same_as_main`).
    same: Vec<String>,
}

fn worktree_view(
    cfg: &Config,
    s: &Site,
    preparing: &std::collections::BTreeMap<String, String>,
) -> WorktreeView {
    use anvil::worktrees::{self as w, MainDb};
    let main = s.worktree_of.as_deref().unwrap_or_default();
    let rec = cfg.worktrees.get(&s.name).cloned().unwrap_or_default();
    let found = w::main_db(cfg, s);
    let (database, main_database) = match &found {
        MainDb::None => (None, None),
        MainDb::Sqlite { to, .. } => (
            Some(to.strip_prefix(&s.path).unwrap_or(to).display().to_string()),
            Some(format!("{main}'s")),
        ),
        MainDb::Anvil { database, .. } => (Some(w::own_db(cfg, s)), Some(database.clone())),
        MainDb::External => (Some(w::own_db(cfg, s)), None),
    };
    WorktreeView {
        db: w::choice(cfg, s),
        choices: w::choices(cfg, s),
        prepared: w::prepared(cfg, s),
        database,
        main_database,
        pending: w::pending(cfg, s).is_some(),
        preparing: preparing.get(&s.name).cloned(),
        compose: anvil::compose::file(&s.path).is_some(),
        cached_config: s.path.join("bootstrap/cache/config.php").exists(),
        error: w::checked_env(cfg, s, false).err().map(err),
        copies: rec.copies.into_iter().map(|(_, d)| d).collect(),
        adopted: rec.adopted,
        missing: w::missing(cfg, s),
        same: w::same_as_main(cfg, s),
    }
}

type Res<T> = Result<T, String>;

/// The sidebar's dot per app: "starting" while its own server's port doesn't answer
/// yet, "running" when anything of it runs (its server, a dev process `<site>@…`, a
/// container), "crashed" when one ended on its own and nothing runs. One-off scripts
/// (`<site>#…`) don't count.
fn app_states(
    sites: &[Site],
    live: &[String],
    crashed: &[String],
    containers: &[std::path::PathBuf],
    answers: impl Fn(u16) -> bool,
) -> std::collections::BTreeMap<String, &'static str> {
    let canon: Vec<_> = containers
        .iter()
        .filter_map(|d| anvil::platform::canonicalize(d).ok())
        .collect();
    let mine = |s: &Site, keys: &[String]| {
        keys.iter().any(|k| {
            k == &s.name
                || k.strip_prefix(s.name.as_str())
                    .is_some_and(|r| r.starts_with('@'))
        })
    };
    sites
        .iter()
        .filter_map(|s| {
            let server = live.contains(&s.name);
            let state = if server && !s.port.is_some_and(&answers) {
                "starting"
            } else if mine(s, live)
                || (!containers.is_empty()
                    && (containers.contains(&s.path)
                        || anvil::platform::canonicalize(&s.path)
                            .is_ok_and(|p| canon.contains(&p))))
            {
                "running"
            } else if mine(s, crashed) {
                "crashed"
            } else {
                return None;
            };
            Some((s.name.clone(), state))
        })
        .collect()
}

/// Folders with a running container, from one `docker ps`, and only when an app has a
/// compose file. Kept a few seconds: the snapshot is asked for often.
/// ponytail: a container started outside Anvil shows within 5 s, or at the next change.
static CONTAINERS: Mutex<Option<(std::time::Instant, Vec<std::path::PathBuf>)>> = Mutex::new(None);
fn running_containers(sites: &[Site]) -> Vec<std::path::PathBuf> {
    if !sites
        .iter()
        .any(|s| anvil::compose::file(&s.path).is_some())
    {
        return vec![];
    }
    let mut cache = anvil::lock(&CONTAINERS);
    if let Some((at, dirs)) = cache.as_ref()
        && at.elapsed() < std::time::Duration::from_secs(5)
    {
        return dirs.clone();
    }
    let dirs: Vec<_> = anvil::compose::running()
        .unwrap_or_default()
        .into_iter()
        .filter(|(_, c)| c.state == "running")
        .map(|(d, _)| d)
        .collect();
    *cache = Some((std::time::Instant::now(), dirs.clone()));
    dirs
}

fn err(e: anyhow::Error) -> String {
    format!("{e:#}")
}

/// `anvil::update` (load, change and save under the config lock), with the UI's errors.
fn edit<T>(f: impl FnOnce(&mut Config) -> anyhow::Result<T>) -> Res<(Config, T)> {
    anvil::update(f).map_err(err)
}

/// Whether Caddy and PHP-FPM serve. One of them ending on its own takes the server
/// down: the rest stops in the background, and why shows as a failed start's error does.
fn running(app: &App) -> bool {
    let mut server = anvil::lock(&app.server);
    let Some(exited) = server.as_mut().map(Server::exited) else {
        return false;
    };
    let Some(why) = exited else {
        return true;
    };
    let dead = server.take();
    drop(server);
    *anvil::lock(&app.error) = Some(why);
    app.server_crashed.store(true, Ordering::SeqCst);
    std::thread::spawn(move || drop(dead));
    false
}

/// A macOS notification, only while no Anvil window has focus and Settings › General ›
/// Notifications is on. macOS asks for permission on the first one; denied, it shows nothing.
fn notify(handle: &AppHandle, title: &str, body: &str) {
    use tauri_plugin_notification::{NotificationExt, PermissionState};
    let focused = handle
        .webview_windows()
        .values()
        .any(|w| w.is_focused().unwrap_or(false));
    let n = handle.notification();
    // On macOS the plugin always answers Granted; the system's own prompt comes with the first.
    if focused
        || !anvil::load().is_ok_and(|c| c.notifications)
        || !matches!(n.request_permission(), Ok(PermissionState::Granted))
    {
        return;
    }
    let mut b = n.builder().title(title);
    if !body.is_empty() {
        b = b.body(body);
    }
    let _ = b.show();
}

/// Crashes not there on the last look (`seen`, which becomes this look's): a stopped or
/// restarted process leaves the list, one still crashed stays seen.
fn new_crashes(
    seen: &mut std::collections::BTreeSet<(String, u64)>,
    crashed: Vec<(String, anvil::apps::Exit)>,
) -> Vec<(String, anvil::apps::Exit)> {
    let now = crashed.iter().map(|(k, e)| (k.clone(), e.at)).collect();
    let new = crashed
        .into_iter()
        .filter(|(k, e)| !seen.contains(&(k.clone(), e.at)))
        .collect();
    *seen = now;
    new
}

/// "shop stopped" for an app, "shop: queue stopped" for one of its processes.
fn crash_title(cfg: &Config, key: &str) -> String {
    let Some((site, id)) = key.split_once('@') else {
        return format!("{key} stopped");
    };
    // Names come from detection, as on the Processes rows.
    let name = anvil::sites(cfg)
        .iter()
        .find(|s| s.name == site)
        .and_then(|s| {
            anvil::processes::detect_with(s, Some(cfg))
                .into_iter()
                .find(|p| p.id == id)
                .map(|p| p.name)
        })
        .unwrap_or_else(|| id.to_string());
    format!("{site}: {name} stopped")
}

/// One notification for the processes that crashed since the last look.
fn notify_crashes(handle: &AppHandle, new: &[(String, anvil::apps::Exit)]) {
    if new.is_empty() {
        return;
    }
    let Ok(cfg) = anvil::load() else {
        return;
    };
    match new {
        [(key, exit)] => notify(handle, &crash_title(&cfg, key), &exit_detail(exit)),
        _ => {
            let names: Vec<String> = new
                .iter()
                .map(|(k, _)| {
                    crash_title(&cfg, k)
                        .trim_end_matches(" stopped")
                        .to_string()
                })
                .collect();
            let title = format!("{} processes stopped", new.len());
            notify(handle, &title, &names.join(", "));
        }
    }
}

fn snapshot(app: &App) -> Res<Snapshot> {
    let cfg = anvil::load().map_err(err)?;
    let https_port = anvil::public_https_port(&cfg);
    let http_port = anvil::public_http_port(&cfg);
    let sites = anvil::sites(&cfg);
    let (live, crashed) = {
        let mut apps = anvil::lock(&app.apps);
        let live: Vec<String> = apps.list().into_iter().map(|(k, _, _)| k).collect();
        let crashed: Vec<String> = apps.crashed().into_iter().map(|(k, _)| k).collect();
        (live, crashed)
    };
    let app_states = app_states(
        &sites,
        &live,
        &crashed,
        &running_containers(&sites),
        anvil::port_answers,
    );
    Ok(Snapshot {
        app_states,
        app_errors: anvil::lock(&app.app_errors).clone(),
        kinds: anvil::apps::KINDS
            .iter()
            .map(|k| (*k, anvil::apps::label(k)))
            .collect(),
        paths: cfg.paths.clone(),
        runtimes: anvil::runtimes::RUNTIMES
            .iter()
            .map(|id| (*id, anvil::runtimes::installed(id)))
            .collect(),
        projects: sites
            .iter()
            .map(|s| (s.name.clone(), anvil::project::needs(&cfg, s)))
            .filter(|(_, n)| !n.is_empty())
            .collect(),
        mail: sites
            .iter()
            .filter_map(|s| {
                Some((
                    s.name.clone(),
                    anvil::resources::mailpit_for(&cfg, &s.path)?,
                ))
            })
            .collect(),
        worktrees: {
            let preparing = anvil::lock(&app.preparing).clone();
            sites
                .iter()
                .filter(|s| s.worktree_of.is_some())
                .map(|s| (s.name.clone(), worktree_view(&cfg, s, &preparing)))
                .collect()
        },
        leftovers: anvil::worktrees::leftovers(&cfg, &sites),
        php_ext_built: sites
            .iter()
            .filter(|s| !s.ext.is_empty())
            .filter_map(|s| s.php.clone())
            .chain(cfg.php_ext.keys().cloned())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .map(|v| {
                let built = anvil::phpext::built_names(&v);
                (v, built)
            })
            .collect(),
        sites,
        hidden: anvil::hidden_sites(&cfg),
        php_default: cfg.php.clone(),
        php_installed: anvil::installed_php(),
        php_ext_catalog: anvil::phpext::offered().map(|e| e.name).collect(),
        php_ext: cfg.php_ext.clone(),
        editor: cfg.editor.clone(),
        terminal: cfg.terminal.clone(),
        git_client: git_client(&cfg).map(|c| app_label(&c)),
        groups: cfg.groups.clone(),
        ini: cfg.ini.clone(),
        running: running(app),
        // The web server's own failure first; else a DNS server that can't answer.
        error: anvil::lock(&app.error)
            .clone()
            .or_else(|| anvil::needs_resolver(&cfg).then(anvil::dns_error).flatten()),
        tld: cfg.tld,
        https_port,
        http_port,
    })
}

/// (Re)start Caddy and PHP-FPM. `server` is only locked to swap: stopping the old
/// server takes up to 3 s a child and the first Caddy download much longer, and
/// `running` is asked on every poll. Meanwhile the server counts as stopped.
fn start(app: &App, cfg: &Config) {
    let _one = anvil::lock(&app.restarting);
    let old = anvil::lock(&app.server).take();
    drop(old); // stop the old one first so ports are free
    let started = Server::start(cfg);
    match started {
        Ok(s) => {
            *anvil::lock(&app.server) = Some(s);
            *anvil::lock(&app.error) = None;
        }
        Err(e) => *anvil::lock(&app.error) = Some(err(e)),
    }
}

fn stop(app: &App) {
    let _one = anvil::lock(&app.restarting);
    let old = anvil::lock(&app.server).take();
    drop(old);
}

/// Something changed: rebuild the tray menu and re-render an open window.
fn changed(handle: &AppHandle) {
    if let (Some(tray), Ok(menu)) = (handle.tray_by_id("anvil"), menu(handle)) {
        let _ = tray.set_menu(Some(menu));
    }
    for w in handle.webview_windows().values() {
        let _ = w.eval("window.refresh && window.refresh()");
    }
}

#[tauri::command(async)]
fn state(app: State<App>) -> Res<Snapshot> {
    snapshot(&app)
}

/// Rescan parked folders; re-serve only if the set of sites changed.
#[tauri::command(async)]
fn refresh(handle: AppHandle, app: State<App>) -> Res<Snapshot> {
    if reload(&app)? {
        changed(&handle);
    }
    snapshot(&app)
}

/// Catch up with config.toml: stop what's gone, re-serve only if the set of sites
/// changed. True when it re-served.
fn reload(app: &App) -> Res<bool> {
    let cfg = anvil::load().map_err(err)?;
    stop_vanished(app, &cfg);
    let restart = running(app) && anvil::needs_restart(&cfg);
    if restart {
        start(app, &cfg);
    }
    Ok(restart)
}

#[tauri::command(async)]
fn set_running(handle: AppHandle, app: State<App>, on: bool) -> Res<Snapshot> {
    if on {
        start(&app, &anvil::load().map_err(err)?);
    } else {
        stop(&app);
    }
    changed(&handle);
    snapshot(&app)
}

/// `version: None` puts the site back on the default PHP version.
#[tauri::command(async)]
fn set_php(
    handle: AppHandle,
    app: State<App>,
    site: String,
    version: Option<String>,
) -> Res<Snapshot> {
    let (cfg, ()) = edit(|c| anvil::set_site_php(c, &site, version))?;
    // Its extensions, built for the version it's on now, before PHP-FPM restarts.
    anvil::phpext::build_missing_for(&cfg, &site);
    if running(&app) {
        start(&app, &cfg);
    }
    changed(&handle);
    snapshot(&app)
}

/// Sites changed on disk config: re-serve if running, then refresh everything.
fn sites_changed(handle: &AppHandle, app: &App, cfg: &Config) {
    stop_vanished(app, cfg);
    if running(app) {
        start(app, cfg);
    }
    changed(handle);
}

fn start_app(app: &App, cfg: &Config, name: &str) {
    let result = site_in(cfg, name).and_then(|site| {
        prepare(app, &site).map_err(err)?;
        let started = anvil::lock(&app.apps).start(&site, cfg);
        started.map_err(err)
    });
    let mut errors = anvil::lock(&app.app_errors);
    match result {
        Ok(()) => errors.remove(name),
        Err(e) => errors.insert(name.into(), e),
    };
}

/// One of an app's processes, as the window shows it: the app's own server or
/// site first (`main`, id `""`), then the others.
#[derive(Serialize)]
struct ProcessRow {
    #[serde(flatten)]
    process: anvil::processes::Process,
    /// `server` (the app's own dev server), `site` (served by Anvil, no Start/Stop of
    /// its own) or `process`.
    kind: &'static str,
    main: bool,
    /// Where the Browser button opens it; `None` when it serves no page (a PHP app's Vite).
    web: Option<String>,
    running: bool,
    /// The newest line it wrote.
    last: Option<String>,
    error: Option<String>,
    /// Where an extra dev server answers, e.g. `https://vuefinder--docs.test`.
    address: Option<String>,
    /// Its address's label was set in the app's settings.
    host_edited: bool,
    pid: Option<u32>,
    /// How it ended on its own, until started or stopped again.
    crashed: Option<anvil::apps::Exit>,
}

/// A package.json command that runs once, with how its last run went.
#[derive(Serialize)]
struct CommandChip {
    name: String,
    /// Its log source is `once:<id>`.
    id: String,
    /// The script as its file has it.
    command: String,
    /// `package.json` or `composer.json`.
    source: &'static str,
    running: bool,
    /// How the last run ended, once it has.
    exit: Option<anvil::apps::Exit>,
}

/// What the Overview shows below an app's details.
#[derive(Serialize)]
struct Overview {
    rows: Vec<ProcessRow>,
    /// package.json scripts that run once (`build`, `lint`).
    commands: Vec<CommandChip>,
    /// The compose file's services; `None` without a compose file.
    containers: Option<Vec<anvil::compose::Container>>,
    /// Why the compose file couldn't be read (docker's warnings left out, see `compose`);
    /// `containers` then holds the project's running containers, if any.
    docker_error: Option<String>,
    /// The compose file's name, as `docker-compose.yml`.
    compose_file: Option<String>,
    /// Rows hidden in the app's settings, to bring back.
    hidden: Vec<HiddenRow>,
}

#[derive(Serialize)]
struct HiddenRow {
    id: String,
    name: String,
}

/// The app's own row: the dev server it runs, or the site Anvil serves (a PHP or
/// static app, running while the web server is). A Compose project has none.
fn main_row(cfg: &Config, site: &Site, serving: bool, running: bool) -> Option<ProcessRow> {
    let server = anvil::apps::runs_process(&site.kind);
    if !server && !anvil::apps::serves(&site.kind) {
        return None;
    }
    let command = site.command.clone().unwrap_or_default();
    let name = match command.split_once("npm run ") {
        _ if !server => site.host.clone(),
        Some((_, rest)) => rest.split_whitespace().next().unwrap_or("server").into(),
        None => "server".into(),
    };
    Some(ProcessRow {
        process: anvil::processes::Process {
            name,
            command: if server { command } else { String::new() },
            source: "anvil",
            server: true,
            port: site.port.filter(|_| server),
            autostart: server && site.autostart,
            edited: server && site.command_custom,
            ..Default::default()
        },
        kind: if server { "server" } else { "site" },
        main: true,
        web: Some(anvil::site_url(cfg, site)),
        running: if server { running } else { serving },
        last: None,
        error: None,
        address: None,
        host_edited: false,
        pid: None,
        crashed: None,
    })
}

/// Where a process row opens: only the extra dev servers of non-PHP apps serve
/// pages (`address` has none for PHP apps); a PHP app's Vite only serves its assets.
fn row_web(cfg: &Config, site: &Site, p: &anvil::processes::Process) -> Option<String> {
    p.port
        .and_then(|_| anvil::processes::address(cfg, site, &p.id))
}

fn process_rows(app: &App, cfg: &Config, site: &Site) -> Vec<ProcessRow> {
    // Detecting may run PHP: do it before taking the locks.
    let detected = anvil::processes::detect_with(site, Some(cfg));
    let serving = running(app);
    // Read each row's last line before the locks too: it touches the disk.
    let mut last: std::collections::BTreeMap<String, Option<String>> =
        std::iter::once(site.name.clone())
            .chain(
                detected
                    .iter()
                    .map(|p| anvil::processes::key(&site.name, &p.id)),
            )
            .map(|k| {
                let line = anvil::logs::last_line(&anvil::apps::log_file(&k));
                (k, line)
            })
            .collect();
    // One lock at a time (see `App`): copy the errors, then take `apps`.
    let errors = anvil::lock(&app.app_errors).clone();
    let mut apps = anvil::lock(&app.apps);
    let crashed = apps.crashed();
    let main = main_row(cfg, site, serving, apps.running(&site.name));
    let rows = detected.into_iter().map(|p| {
        let web = row_web(cfg, site, &p);
        ProcessRow {
            running: apps.running(&anvil::processes::key(&site.name, &p.id)),
            kind: "process",
            main: false,
            address: web.clone(),
            host_edited: cfg
                .sites
                .get(&site.name)
                .and_then(|c| c.processes.get(&p.id))
                .is_some_and(|c| c.host.is_some()),
            web,
            last: None,
            error: None,
            pid: None,
            crashed: None,
            process: p,
        }
    });
    let mut rows: Vec<ProcessRow> = main.into_iter().chain(rows).collect();
    // The app's own row first, then those kept on top, each group in detection order.
    rows.sort_by_key(|r| !r.main);
    rows.into_iter()
        .map(|mut r| {
            // The site row is the web server's; it has no process, log or errors of its own.
            if r.kind == "site" {
                return r;
            }
            let key = if r.main {
                site.name.clone()
            } else {
                anvil::processes::key(&site.name, &r.process.id)
            };
            r.last = last.remove(&key).flatten();
            r.pid = apps.pid(&key);
            r.crashed = crashed
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, e)| e.clone());
            r.error = errors.get(&key).cloned();
            r
        })
        .collect()
}

fn overview(app: &App, cfg: &Config, site: &Site) -> Overview {
    // Docker is only asked about apps that have a compose file.
    let compose_file = anvil::compose::file(&site.path);
    let (containers, docker_error) = match &compose_file {
        None => (None, None),
        Some(_) => match anvil::compose::containers(&site.path) {
            Ok(c) => (Some(c), None),
            // A file that can't be read (unset variables) may still have containers up.
            Err(e) => {
                let real = anvil::platform::canonicalize(&site.path).ok();
                let up: Vec<_> = anvil::compose::running()
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|(dir, _)| {
                        *dir == site.path
                            || (real.is_some() && anvil::platform::canonicalize(dir).ok() == real)
                    })
                    .map(|(_, c)| c)
                    .collect();
                ((!up.is_empty()).then_some(up), Some(err(e)))
            }
        },
    };
    let compose_file =
        compose_file.and_then(|f| Some(f.file_name()?.to_string_lossy().into_owned()));
    let (servers, scripts) = anvil::processes::package_scripts(&site.path);
    // ponytail: a hidden row's name is found among package.json scripts, else its id
    // (artisan dev's names are their ids); the full detection drops hidden rows.
    let hidden = cfg
        .sites
        .get(&site.name)
        .map(anvil::processes::hidden)
        .unwrap_or_default()
        .into_iter()
        .map(|id| {
            let name = servers
                .iter()
                .map(|p| p.name.clone())
                .chain(scripts.iter().cloned())
                .find(|n| anvil::processes::process_id(n) == id)
                .unwrap_or_else(|| id.clone());
            HiddenRow { id, name }
        })
        .collect();
    let one_off = anvil::processes::one_off_scripts(site);
    let commands = {
        let mut apps = anvil::lock(&app.apps);
        one_off
            .into_iter()
            .map(|s| {
                let key = anvil::processes::once_key(&site.name, &s.id);
                let running = apps.running(&key);
                CommandChip {
                    id: s.id,
                    command: s.text,
                    source: s.source,
                    exit: apps.finished(&key).filter(|_| !running),
                    running,
                    name: s.name,
                }
            })
            .collect()
    };
    Overview {
        rows: process_rows(app, cfg, site),
        commands,
        containers,
        docker_error,
        compose_file,
        hidden,
    }
}

fn find_site(name: &str) -> Res<Site> {
    find_site_cfg(name).map(|(_, s)| s)
}

/// The app and the config it was read from, loaded once.
fn find_site_cfg(name: &str) -> Res<(Config, Site)> {
    let cfg = anvil::load().map_err(err)?;
    let site = site_in(&cfg, name)?;
    Ok((cfg, site))
}

/// The app named `name` in `cfg`, else "No app named".
fn site_in(cfg: &Config, name: &str) -> Res<Site> {
    anvil::sites(cfg)
        .into_iter()
        .find(|s| s.name == name)
        .ok_or_else(|| format!("No app named {name}"))
}

/// One line of the Processes window.
#[derive(Serialize)]
struct Row {
    /// What `activity_log` takes: `web:caddy`, `<site>`, `<site>@<id>`, `service:<id>`...
    key: String,
    name: String,
    pid: Option<u32>,
    port: Option<u16>,
    /// `None` for containers (Docker does the accounting) and for crashed rows.
    cpu: Option<f32>,
    memory: Option<u64>,
    /// `running`, `starting` or `crashed`.
    state: &'static str,
    detail: Option<String>,
    /// Whether Stop/Restart apply to it.
    actions: bool,
    /// The page the 🌐 button opens (as in the app's Overview).
    web: Option<String>,
}

#[derive(Serialize)]
struct AppGroup {
    site: String,
    rows: Vec<Row>,
}

#[derive(Serialize)]
struct Activity {
    web: Vec<Row>,
    apps: Vec<AppGroup>,
    services: Vec<Row>,
}

fn row(key: String, name: String, state: &'static str, actions: bool) -> Row {
    Row {
        key,
        name,
        pid: None,
        port: None,
        cpu: None,
        memory: None,
        state,
        detail: None,
        actions,
        web: None,
    }
}

/// Fill in pid, CPU and memory from the `ps` table.
fn measured(mut r: Row, table: &[anvil::resources::Ps], pid: u32, group: bool) -> Row {
    let (cpu, memory) = anvil::resources::usage(table, pid, group);
    r.pid = Some(pid);
    r.cpu = Some(cpu);
    r.memory = Some(memory);
    r
}

fn exit_detail(e: &anvil::apps::Exit) -> String {
    match e.code {
        Some(c) => format!("Exited with code {c}"),
        None => "Ended by a signal".into(),
    }
}

/// What agents and the terminal did through the control socket, newest first: one app's
/// for its Overview, all of them for Processes › Agents.
#[tauri::command]
fn agent_activity(app: State<App>, site: Option<String>) -> Vec<control::AgentAction> {
    control::newest(&app.agent_log, site.as_deref())
}

/// Everything Anvil runs: the web server, apps with their processes and containers, services.
#[tauri::command(async)]
fn activity(app: State<App>) -> Res<Activity> {
    let cfg = anvil::load().map_err(err)?;
    let sites = anvil::sites(&cfg);
    // Quick snapshots, each lock released before anything slow.
    let web_pids = anvil::lock(&app.server)
        .as_ref()
        .map(|s| s.pids())
        .unwrap_or_default();
    let (listed, crashed, scripts) = {
        let mut apps = anvil::lock(&app.apps);
        let listed = apps.list();
        // Commands run once show their script (`build:all`), not the id in their key.
        let scripts: std::collections::HashMap<String, String> = listed
            .iter()
            .filter_map(|(k, _, _)| Some((k.clone(), apps.once_name(k)?.to_string())))
            .collect();
        (listed, apps.crashed(), scripts)
    };
    let (infos, service_pids) = {
        let mut services = anvil::lock(&app.services);
        (services.info(&cfg), services.pids())
    };

    // One `docker ps` for every project, and only when an app has a compose file.
    let containers = if sites
        .iter()
        .any(|s| anvil::compose::file(&s.path).is_some())
    {
        anvil::compose::running().unwrap_or_default()
    } else {
        vec![]
    };

    let table = anvil::resources::ps();
    let web = web_pids
        .into_iter()
        .map(|(label, pid)| {
            // `PHP 8.4 FPM`, `PHP 8.4 FPM (shop)`, on Windows `PHP 8.4 (pool, port 20001)`:
            // one row each, so the key carries the pid; their log is the version's.
            let key = match label.strip_prefix("PHP ").and_then(|l| l.split(' ').next()) {
                Some(v) => format!("web:php-{v}:{pid}"),
                None => "web:caddy".into(),
            };
            measured(row(key, label, "running", false), &table, pid, false)
        })
        .collect();

    let services = infos
        .iter()
        .filter_map(|i| {
            let pid = *service_pids.get(&i.id)?;
            let mut r = row(format!("service:{}", i.id), i.name.into(), "running", true);
            r.port = Some(i.port);
            r.detail = (i.id != i.kind).then(|| i.id.clone());
            Some(measured(r, &table, pid, false))
        })
        .collect();

    let mut groups = vec![];
    for site in &sites {
        let mut rows = vec![];
        let mine = |key: &str| {
            key.strip_prefix(site.name.as_str())
                .filter(|rest| rest.is_empty() || rest.starts_with(['@', '#']))
                .map(str::to_string)
        };
        let live: Vec<_> = listed
            .iter()
            .filter_map(|(k, pid, _)| Some((k, *pid, mine(k)?)))
            .collect();
        let dead: Vec<_> = crashed
            .iter()
            .filter_map(|(k, e)| Some((k, e, mine(k)?)))
            .collect();
        if !live.is_empty() || !dead.is_empty() {
            // Names come from detection, which may run PHP.
            let detected = anvil::processes::detect_with(site, Some(&cfg));
            let found = |id: &str| detected.iter().find(|p| p.id == id);
            let name = |id: &str| found(id).map_or_else(|| id.to_string(), |p| p.name.clone());
            for (key, pid, rest) in live {
                let mut r = if rest.is_empty() {
                    let mut r = row(key.clone(), site.name.clone(), "running", true);
                    r.port = site.port;
                    r.web = Some(anvil::site_url(&cfg, site));
                    r
                } else {
                    let id = &rest[1..];
                    let shown = scripts.get(key).cloned().unwrap_or_else(|| name(id));
                    let mut r = row(key.clone(), shown, "running", true);
                    r.port = found(id).and_then(|p| p.port);
                    r.web = found(id).and_then(|p| row_web(&cfg, site, p));
                    r
                };
                // A server is starting until its port answers.
                if r.port.is_some_and(|p| !anvil::port_answers(p)) {
                    r.state = "starting";
                }
                rows.push(measured(r, &table, pid, true));
            }
            for (key, exit, rest) in dead {
                let shown = if rest.is_empty() {
                    site.name.clone()
                } else {
                    name(&rest[1..])
                };
                let mut r = row(key.clone(), shown, "crashed", true);
                r.detail = Some(exit_detail(exit));
                rows.push(r);
            }
        }
        // The project folder may be named through a symlink on either side.
        let real = anvil::platform::canonicalize(&site.path).ok();
        let here = |dir: &std::path::Path| {
            dir == site.path || (real.is_some() && anvil::platform::canonicalize(dir).ok() == real)
        };
        if anvil::compose::file(&site.path).is_some() {
            for (_, c) in containers
                .iter()
                .filter(|(dir, c)| c.state == "running" && here(dir))
            {
                let state = if c.health.as_deref() == Some("starting") {
                    "starting"
                } else {
                    "running"
                };
                let key = format!("container:{}/{}", site.name, c.service);
                let mut r = row(key, c.service.clone(), state, true);
                r.port = c.ports.first().copied();
                r.detail = c.health.clone();
                rows.push(r);
            }
        }
        // The app's own row first.
        rows.sort_by_key(|r| r.key != site.name);
        if !rows.is_empty() {
            groups.push(AppGroup {
                site: site.name.clone(),
                rows,
            });
        }
    }
    Ok(Activity {
        web,
        apps: groups,
        services,
    })
}

/// What a log key from the UI names. Only the listed forms; never a path.
#[derive(Debug, PartialEq)]
enum LogKey<'a> {
    Caddy,
    Php(&'a str),
    App(&'a str),
    Process(&'a str, &'a str),
    Once(&'a str, &'a str),
    Service(&'a str),
    Container(&'a str, &'a str),
}

/// `services` are the configured service ids; any other `service:` key is refused.
fn parse_log_key<'a>(key: &'a str, services: &[String]) -> Res<LogKey<'a>> {
    let parsed = if key == "web:caddy" {
        Some(LogKey::Caddy)
    } else if let Some(v) = key.strip_prefix("web:php-") {
        let v = v.split_once(':').map_or(v, |(v, pid)| {
            if pid.chars().all(|c| c.is_ascii_digit()) {
                v
            } else {
                ""
            }
        });
        (!v.is_empty() && v.chars().all(|c| c.is_ascii_digit() || c == '.') && !v.contains(".."))
            .then_some(LogKey::Php(v))
    } else if let Some(id) = key.strip_prefix("service:") {
        (process_id(id) && services.iter().any(|s| s == id)).then_some(LogKey::Service(id))
    } else if let Some(rest) = key.strip_prefix("container:") {
        rest.split_once('/')
            .filter(|(s, c)| site_name(s) && site_name(c))
            .map(|(s, c)| LogKey::Container(s, c))
    } else if let Some((s, id)) = key.split_once('@') {
        (site_name(s) && process_id(id)).then_some(LogKey::Process(s, id))
    } else if let Some((s, id)) = key.split_once('#') {
        (site_name(s) && process_id(id)).then_some(LogKey::Once(s, id))
    } else {
        site_name(key).then_some(LogKey::App(key))
    };
    parsed.ok_or_else(|| format!("Not a log: {key}"))
}

/// New log content for a row of the Processes window, by key (see `parse_log_key`):
/// raw, so a process's colors survive.
#[tauri::command(async)]
fn activity_log(key: String, from: u64) -> Res<anvil::logs::Text> {
    if let Some(file) = activity_log_file(&key)? {
        if !file.exists() {
            return Ok(anvil::logs::Text::default());
        }
        return anvil::logs::tail_text(&file, from).map_err(err);
    }
    let LogKey::Container(site, service) = parse_log_key(&key, &[])? else {
        return Ok(anvil::logs::Text::default());
    };
    let text = anvil::compose::logs(&find_site(site)?.path, service).map_err(err)?;
    // ponytail: no incremental tail for containers; the whole text again unless its size is unchanged.
    let end = text.len() as u64;
    Ok(anvil::logs::Text {
        text: if from == end { String::new() } else { text },
        end_offset: end,
    })
}

/// Empty a Processes-window row's log file. Containers' logs are Docker's and stay.
#[tauri::command(async)]
fn activity_log_clear(key: String) -> Res<()> {
    match activity_log_file(&key)? {
        Some(file) if file.exists() => std::fs::OpenOptions::new()
            .write(true)
            .open(&file)
            .and_then(|f| f.set_len(0))
            .map_err(|e| e.to_string()),
        Some(_) => Ok(()),
        None => Err("A container's log is kept by Docker and can't be cleared here".into()),
    }
}

/// The log file behind a Processes-window row; None for a container (Docker keeps it).
fn activity_log_file(key: &str) -> Res<Option<std::path::PathBuf>> {
    // The service list is read only for a service key; Caddy and PHP logs need no config.
    let services = if key.starts_with("service:") {
        anvil::services::instances(&anvil::load().map_err(err)?)
    } else {
        vec![]
    };
    let file = match parse_log_key(key, &services)? {
        LogKey::Caddy => anvil::caddy_log(),
        LogKey::Php(v) => anvil::php_fpm_log(v),
        LogKey::Service(id) => anvil::services::log_file(id),
        LogKey::App(site) => {
            find_site(site)?;
            anvil::apps::log_file(site)
        }
        LogKey::Process(site, id) => {
            find_site(site)?;
            anvil::processes::log_file(site, id)
        }
        LogKey::Once(site, id) => {
            find_site(site)?;
            anvil::apps::log_file(&anvil::processes::once_key(site, id))
        }
        LogKey::Container(..) => return Ok(None),
    };
    Ok(Some(file))
}

#[tauri::command(async)]
fn processes(app: State<App>, site: String) -> Res<Overview> {
    let (cfg, s) = find_site_cfg(&site)?;
    if let Some(e) = anvil::processes::project_file_error(&s) {
        return Err(e);
    }
    Ok(overview(&app, &cfg, &s))
}

/// Start, stop or restart one process, or when `name` is None the app's own server and
/// its processes: Dev start/stop. Dev start leaves out the processes marked manual.
#[tauri::command(async)]
fn process_run(
    handle: AppHandle,
    app: State<App>,
    site: String,
    name: Option<String>,
    action: String,
    columns: Option<u16>,
    lines: Option<u16>,
) -> Res<Overview> {
    if name.as_deref().is_some_and(|n| !process_id(n)) {
        return Err(format!("Not a process: {name:?}"));
    }
    let (cfg, s) = find_site_cfg(&site)?;
    set_size(&app, &s.name, columns, lines);
    if name.is_none() && anvil::apps::runs_process(&s.kind) {
        run_app(&app, &cfg, &s.name, &action);
        changed(&handle);
    }
    if action != "start" {
        // By key, not detection: a row whose script was renamed or removed still stops.
        let keys = match &name {
            Some(n) => vec![anvil::processes::key(&s.name, n)],
            // Scripts run once (`<site>#…`) aren't Dev's: they finish on their own.
            None => anvil::lock(&app.apps)
                .process_keys(&s.name)
                .into_iter()
                .filter(|k| !k.contains('#'))
                .collect(),
        };
        for key in keys {
            stop_app(&app, &key);
            anvil::lock(&app.app_errors).remove(&key);
        }
    }
    if action != "stop" {
        for p in anvil::processes::detect_with(&s, Some(&cfg))
            .into_iter()
            .filter(|p| match name.as_deref() {
                Some(n) => n == p.id,
                None => !p.manual,
            })
        {
            start_process(&app, &cfg, &s, &p);
        }
    }
    Ok(overview(&app, &cfg, &s))
}

/// Run one of the app's one-off scripts by id (see `one_off_scripts`); its output is
/// the log source `once:<id>`.
#[tauri::command(async)]
fn command_run(
    app: State<App>,
    site: String,
    id: String,
    columns: Option<u16>,
    lines: Option<u16>,
) -> Res<Overview> {
    let (cfg, s) = find_site_cfg(&site)?;
    set_size(&app, &s.name, columns, lines);
    let script = anvil::processes::one_off_scripts(&s)
        .into_iter()
        .find(|c| c.id == id)
        .ok_or_else(|| format!("{site} has no script {id}"))?;
    prepare(&app, &s).map_err(err)?;
    anvil::lock(&app.apps)
        .run_once(&s, &script, &cfg)
        .map_err(err)?;
    Ok(overview(&app, &cfg, &s))
}

/// Stop a package.json command run once (`command_run`), key `<site>#<id>`.
#[tauri::command(async)]
fn once_stop(handle: AppHandle, app: State<App>, site: String, id: String) -> Res<()> {
    find_site(&site)?;
    stop_app(&app, &anvil::processes::once_key(&site, &id));
    changed(&handle);
    Ok(())
}

/// `docker compose up -d|stop|restart` one service. It can take minutes (a slow
/// shutdown), so no lock is held while Docker works.
#[tauri::command(async)]
fn container_run(app: State<App>, site: String, service: String, action: String) -> Res<Overview> {
    let (cfg, s) = find_site_cfg(&site)?;
    if anvil::compose::file(&s.path).is_none() {
        return Err(format!("{site} has no compose file"));
    }
    anvil::compose::action(&s.path, &service, &action).map_err(err)?;
    *anvil::lock(&CONTAINERS) = None;
    Ok(overview(&app, &cfg, &s))
}

/// Start All / Stop All for the containers: one `docker compose up -d|stop` for the
/// whole project, so compose keeps the dependency order. No lock is held meanwhile.
#[tauri::command(async)]
fn containers_run(app: State<App>, site: String, action: String) -> Res<Overview> {
    let (cfg, s) = find_site_cfg(&site)?;
    if anvil::compose::file(&s.path).is_none() {
        return Err(format!("{site} has no compose file"));
    }
    anvil::compose::project_action(&s.path, &action).map_err(err)?;
    *anvil::lock(&CONTAINERS) = None;
    Ok(overview(&app, &cfg, &s))
}

/// New lines of one service's log for the Containers tab, from the stream kept while
/// the tab shows the app (`compose::Logs`): `text` after line `after`, `next` to ask
/// after next time.
#[derive(Serialize)]
struct LogLines {
    text: String,
    next: u64,
}

#[tauri::command(async)]
fn container_log_read(app: State<App>, site: String, service: String, after: u64) -> Res<LogLines> {
    let (text, next) = anvil::lock(&app.container_logs)
        .read(
            &site,
            || find_site(&site).map(|s| s.path).map_err(anyhow::Error::msg),
            &service,
            after,
        )
        .map_err(err)?;
    Ok(LogLines { text, next })
}

/// The Containers tab is hidden, or shows another app: its log streams end.
#[tauri::command(async)]
fn container_logs_stop(app: State<App>) {
    anvil::lock(&app.container_logs).stop();
}

/// Start the processes marked Start with Anvil. Only apps that still exist and rows
/// still detected (not removed, renamed or hidden) start.
fn autostart_processes(app: &App, cfg: &Config) {
    let wanted = |s: &Site| {
        cfg.sites
            .get(&s.name)
            .is_some_and(|c| c.processes.values().any(|p| p.autostart))
    };
    for site in anvil::sites(cfg).into_iter().filter(wanted) {
        for p in anvil::processes::detect_with(&site, Some(cfg))
            .into_iter()
            .filter(|p| p.autostart)
        {
            start_process(app, cfg, &site, &p);
        }
    }
}

/// A worktree's own database before anything of it starts (`worktrees::prepare_site`),
/// outside `app.apps`: a copy can take minutes. Its line is in `app.preparing` meanwhile.
fn prepare(app: &App, site: &Site) -> anyhow::Result<()> {
    let made = anvil::worktrees::prepare_site(site, &mut |line| {
        anvil::lock(&app.preparing).insert(site.name.clone(), line.into());
        // The window shows the line on the worktree's Overview while the copy runs.
        if let Some(h) = HANDLE.get() {
            changed(h);
        }
    });
    anvil::lock(&app.preparing).remove(&site.name);
    // Its web requests wait for its own database (`worktrees::web_env`): served once it's
    // there, made now or from a terminal before.
    if made.is_ok()
        && site.worktree_of.is_some()
        && running(app)
        && let Ok(cfg) = anvil::load()
        && anvil::needs_restart(&cfg)
    {
        start(app, &cfg);
    }
    made
}

fn start_process(app: &App, cfg: &Config, site: &Site, p: &anvil::processes::Process) {
    let started = prepare(app, site)
        .and_then(|()| anvil::octane::prepare(site, p))
        .and_then(|()| anvil::lock(&app.apps).start_process(site, p, cfg));
    if let Err(e) = started {
        let key = anvil::processes::key(&site.name, &p.id);
        anvil::lock(&app.app_errors).insert(key, err(e));
        return;
    }
    // laravel/octane added while Anvil runs: the web server learns of the row only now,
    // and sends the app's address to it.
    if p.id == anvil::octane::ID
        && running(app)
        && let Ok(cfg) = anvil::load()
        && anvil::needs_restart(&cfg)
    {
        start(app, &cfg);
    }
}

/// A release newer than this one, as `update_check` found it.
#[derive(Clone, Serialize)]
struct Release {
    version: String,
    notes: Option<String>,
}

/// What the last check found; windows opened later read it with `update_state`.
static FOUND: Mutex<Option<Release>> = Mutex::new(None);
/// When the app's binary on disk was written, as of this process's start.
static STARTED_EXE: std::sync::OnceLock<Option<std::time::SystemTime>> = std::sync::OnceLock::new();

fn exe_mtime() -> Option<std::time::SystemTime> {
    std::env::current_exe()
        .ok()?
        .metadata()
        .ok()?
        .modified()
        .ok()
}

/// A newer build replaced the binary on disk since this process started (a local build).
fn rebuilt() -> bool {
    let started = *STARTED_EXE.get_or_init(exe_mtime);
    started.is_some() && exe_mtime() > started
}

#[derive(Serialize)]
struct UpdateState {
    update: Option<Release>,
    /// Offer Relaunch: the binary on disk is newer than this process.
    relaunch: bool,
}

#[tauri::command(async)]
fn update_state() -> UpdateState {
    UpdateState {
        update: anvil::lock(&FOUND).clone(),
        relaunch: rebuilt(),
    }
}

/// The newest release, when it is newer than this Anvil; windows hear of it as `update`.
#[tauri::command]
async fn update_check(handle: AppHandle) -> Res<Option<Release>> {
    use tauri_plugin_updater::UpdaterExt;
    let update = handle.updater().map_err(|e| e.to_string())?;
    let found = update.check().await.map_err(|e| e.to_string())?;
    let found = found.map(|u| Release {
        version: u.version,
        notes: u.body,
    });
    *anvil::lock(&FOUND) = found.clone();
    let _ = handle.emit("update", ());
    Ok(found)
}

/// Download and install the newest release (progress as `update-progress`, a percent or
/// null), then restart into it with the same dev processes running.
#[tauri::command]
async fn update_install(handle: AppHandle) -> Res<()> {
    use tauri_plugin_updater::UpdaterExt;
    let updater = handle.updater().map_err(|e| e.to_string())?;
    let update = updater
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Anvil is up to date")?;
    let (mut got, mut shown) = (0u64, None);
    update
        .download_and_install(
            |chunk, total| {
                got += chunk as u64;
                let pct = total.map(|t| (got * 100 / t.max(1)).min(100));
                if shown != Some(pct) {
                    shown = Some(pct);
                    let _ = handle.emit("update-progress", pct);
                }
            },
            || {},
        )
        .await
        .map_err(|e| e.to_string())?;
    relaunch(handle)
}

/// Restart Anvil; the dev processes running now start again in the new one.
#[tauri::command(async)]
fn relaunch(handle: AppHandle) -> Res<()> {
    let keys: Vec<String> = anvil::lock(&handle.state::<App>().apps)
        .list()
        .into_iter()
        .map(|(k, ..)| k)
        // Scripts run once (`<site>#…`) finish on their own.
        .filter(|k| !k.contains('#'))
        .collect();
    save_keys(&restore_file(), &keys).map_err(|e| e.to_string())?;
    // Stopped here, off the main thread, before the new Anvil starts: a restart's
    // `ExitRequested` can't be held back to wait for a stop.
    stop_everything(&handle.state::<App>());
    STOPPED.store(true, Ordering::SeqCst);
    handle.restart()
}

/// The dev processes to start again after a restart, one key per line.
fn restore_file() -> std::path::PathBuf {
    anvil::home().join("run").join("restart-processes")
}

fn save_keys(path: &std::path::Path, keys: &[String]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, keys.join("\n"))
}

/// The keys `save_keys` wrote, once: the file is gone afterwards.
fn take_keys(path: &std::path::Path) -> Vec<String> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let _ = std::fs::remove_file(path);
    text.lines()
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect()
}

/// Start what ran before a restart: apps (`<site>`) and their processes (`<site>@<id>`)
/// that still exist; ones running already (Start with Anvil) are left alone.
fn restore_processes(app: &App, cfg: &Config, keys: &[String]) {
    for key in keys {
        let (name, id) = key
            .split_once('@')
            .map_or((key.as_str(), None), |(n, i)| (n, Some(i)));
        let Ok(site) = site_in(cfg, name) else {
            continue;
        };
        match id {
            None => start_app(app, cfg, name),
            Some(id) => {
                let found = anvil::processes::detect_with(&site, Some(cfg))
                    .into_iter()
                    .find(|p| p.id == id);
                if let Some(p) = found {
                    start_process(app, cfg, &site, &p);
                }
            }
        }
    }
}

/// Stop an app or process without keeping `apps` locked while it shuts down (up to 5 s).
fn stop_app(app: &App, key: &str) {
    let stopping = anvil::lock(&app.apps).take(key);
    if let Some(s) = stopping {
        s.wait(anvil::apps::GRACE);
    }
}

/// Stop what runs for a site that is gone (a worktree removed outside Anvil), all at once
/// and without keeping `apps` locked while it shuts down.
fn stop_vanished(app: &App, cfg: &Config) {
    let stopping = anvil::lock(&app.apps).take_vanished(&anvil::sites(cfg));
    if !stopping.is_empty() {
        anvil::stop_all(stopping, anvil::apps::GRACE);
    }
}

/// Stop a service without keeping `services` locked while it shuts down (up to 10 s).
fn stop_service(app: &App, id: &str) {
    let stopping = anvil::lock(&app.services).take(id);
    if let Some(s) = stopping {
        s.wait(anvil::services::GRACE);
    }
}

/// Set once `stop_everything` ran for Quit or Relaunch: the exit may go ahead.
static STOPPED: AtomicBool = AtomicBool::new(false);
/// Set when Quit starts stopping, so a second Quit meanwhile doesn't stop again.
static QUITTING: AtomicBool = AtomicBool::new(false);
/// Set while this Anvil owns the control socket's file.
static SERVING: AtomicBool = AtomicBool::new(false);

/// Stop everything Anvil runs (dev processes, the web server, services, container log
/// streams) at once: SIGTERM to all, one shared deadline (a service's, the longest
/// grace), then SIGKILL what's left. Takes up to 10 s; call it off the main thread.
fn stop_everything(app: &App) {
    // First, so the CLI reads "not running" during the stop. Once: a relaunched Anvil
    // may own the path by the final Exit.
    if SERVING.swap(false, Ordering::SeqCst) {
        let _ = std::fs::remove_file(anvil::control::socket_path());
    }
    anvil::lock(&app.container_logs).stop();
    let mut all = anvil::lock(&app.apps).take_all();
    // Held through the stop, like `stop`, so a web server start can't interleave.
    let _one = anvil::lock(&app.restarting);
    let server = anvil::lock(&app.server).take();
    all.extend(server.into_iter().flat_map(|mut s| s.take_all()));
    all.extend(anvil::lock(&app.services).take_all());
    anvil::stop_all(all, anvil::services::GRACE);
}

fn run_app(app: &App, cfg: &Config, site: &str, action: &str) {
    if action != "start" {
        stop_app(app, site);
        anvil::lock(&app.app_errors).remove(site);
    }
    if action != "stop" {
        start_app(app, cfg, site);
    }
}

/// The Dev tab's output pane in characters, given with a start: COLUMNS and LINES of
/// what the app starts from then on (see `Apps::set_size`).
fn set_size(app: &App, site: &str, columns: Option<u16>, lines: Option<u16>) {
    if let Some(c) = columns {
        let l = lines.unwrap_or(anvil::apps::TERM_SIZE.1);
        anvil::lock(&app.apps).set_size(site, c, l);
    }
}

#[tauri::command(async)]
fn app_run(
    handle: AppHandle,
    app: State<App>,
    site: String,
    action: String,
    columns: Option<u16>,
    lines: Option<u16>,
) -> Res<Snapshot> {
    set_size(&app, &site, columns, lines);
    run_app(&app, &anvil::load().map_err(err)?, &site, &action);
    changed(&handle);
    snapshot(&app)
}

/// Change an app's kind, start command, port or autostart. Fields left out stay as they are;
/// `reset` lists fields to put back to their defaults.
#[tauri::command(async)]
#[allow(clippy::too_many_arguments)]
fn set_app(
    handle: AppHandle,
    app: State<App>,
    site: String,
    kind: Option<String>,
    command: Option<String>,
    port: Option<u16>,
    autostart: Option<bool>,
    reset: Option<Vec<String>>,
) -> Res<Snapshot> {
    let reset = reset.unwrap_or_default();
    let pick = |name: &str, v: Option<String>| -> Option<Option<String>> {
        if reset.iter().any(|r| r == name) {
            Some(None)
        } else {
            v.map(Some)
        }
    };
    let port = if reset.iter().any(|r| r == "port") {
        Some(None)
    } else {
        port.map(Some)
    };
    let reroute =
        kind.is_some() || port.is_some() || reset.iter().any(|r| r == "kind" || r == "port");
    let (cfg, ()) = edit(|c| {
        anvil::set_site_app(
            c,
            &site,
            pick("kind", kind),
            pick("command", command),
            port,
            autostart,
        )
    })?;
    // A running app picks up a new command or port by restarting.
    let was_running = anvil::lock(&app.apps).running(&site);
    if was_running {
        stop_app(&app, &site);
    }
    if reroute {
        sites_changed(&handle, &app, &cfg);
    }
    if was_running {
        start_app(&app, &cfg, &site);
    }
    changed(&handle);
    snapshot(&app)
}

/// Change one row's settings: its command (`reset` goes back to the detected one),
/// Start with Anvil, hidden, pinned, kept on top or its address's label (`reset_host`: automatic). The app's own row (`id` "") maps onto `set_app`
/// and can't be hidden. A running row whose command changed restarts.
#[tauri::command(async)]
#[allow(clippy::too_many_arguments)]
fn set_process(
    handle: AppHandle,
    app: State<App>,
    site: String,
    id: String,
    command: Option<String>,
    reset: Option<bool>,
    autostart: Option<bool>,
    hidden: Option<bool>,
    host: Option<String>,
    reset_host: Option<bool>,
    manual: Option<bool>,
) -> Res<Overview> {
    if !id.is_empty() && !process_id(&id) {
        return Err(format!("Not a process: {id}"));
    }
    let s = find_site(&site)?;
    let command = if reset == Some(true) {
        Some(None)
    } else {
        command.map(Some)
    };
    let restart = command.is_some();
    if id.is_empty() {
        if hidden == Some(true) {
            return Err("The app's own row can't be hidden".into());
        }
        if manual == Some(true) {
            return Err("The app's own server always starts with Dev".into());
        }
        if !anvil::apps::runs_process(&s.kind) {
            return Err(format!("Anvil serves {site}; it has no command of its own"));
        }
        let (cfg, ()) = edit(|c| anvil::set_site_app(c, &s.name, None, command, None, autostart))?;
        if restart && anvil::lock(&app.apps).running(&s.name) {
            stop_app(&app, &s.name);
            start_app(&app, &cfg, &s.name);
        }
    } else {
        let change = anvil::processes::ProcessChange {
            command,
            autostart,
            hidden,
            manual,
            host: if reset_host == Some(true) {
                Some(None)
            } else {
                host.map(Some)
            },
        };
        let (cfg, reroute) = edit(|c| {
            let before = routes(c, &s);
            anvil::processes::set_process(c, &s, &id, change)?;
            Ok(routes(c, &s) != before)
        })?;
        let key = anvil::processes::key(&s.name, &id);
        let was_running = anvil::lock(&app.apps).running(&key);
        // A hidden row is out of sight, so it doesn't keep running unseen.
        if was_running && (restart || hidden == Some(true)) {
            stop_app(&app, &key);
            anvil::lock(&app.app_errors).remove(&key);
        }
        if was_running && restart && hidden != Some(true) {
            let p = anvil::processes::detect_with(&s, Some(&cfg))
                .into_iter()
                .find(|p| p.id == id);
            if let Some(p) = p {
                start_process(&app, &cfg, &s, &p);
            }
        }
        // Only a server row's route needs the web server reloaded, and that restart
        // (Caddy, PHP-FPM) runs in the background so the click answers at once.
        if reroute {
            reroute_later(&handle);
        }
    }
    changed(&handle);
    let (cfg, s) = find_site_cfg(&site)?;
    Ok(overview(&app, &cfg, &s))
}

/// What the web server routes for an app's server rows and Octane: a change needs a reload.
fn routes(c: &Config, s: &Site) -> Vec<(String, String, u16)> {
    anvil::processes::server_rows(c, s)
        .into_iter()
        .map(|r| (r.id, r.host, r.port))
        .chain(
            anvil::processes::octane_row(c, s)
                .map(|p| (p.id, String::new(), p.port.unwrap_or_default())),
        )
        .collect()
}

/// Reload the web server for changed routes in the background, so the click answers
/// at once (Caddy and PHP-FPM restart).
fn reroute_later(handle: &AppHandle) {
    let handle = handle.clone();
    std::thread::spawn(move || {
        if let Ok(cfg) = anvil::load() {
            sites_changed(&handle, &handle.state::<App>(), &cfg);
        }
    });
}

/// Add a process of the user's own (Configure's + Add process); `name` is also its id.
#[tauri::command(async)]
fn process_add(
    handle: AppHandle,
    app: State<App>,
    site: String,
    name: String,
    command: String,
) -> Res<Overview> {
    let s = find_site(&site)?;
    let (cfg, ()) = edit(|c| anvil::processes::add_process(c, &s, &name, &command))?;
    changed(&handle);
    Ok(overview(&app, &cfg, &s))
}

/// Configure's Reset to detected (`processes::reset_processes`). A process that is no
/// longer listed stops; a changed route reloads the web server in the background.
#[tauri::command(async)]
fn processes_reset(handle: AppHandle, app: State<App>, site: String) -> Res<Overview> {
    let s = find_site(&site)?;
    let (cfg, before) = edit(|c| {
        let before = routes(c, &s);
        anvil::processes::reset_processes(c, &s)?;
        Ok(before)
    })?;
    let listed: Vec<String> = anvil::processes::detect_with(&s, Some(&cfg))
        .into_iter()
        .map(|p| anvil::processes::key(&s.name, &p.id))
        .collect();
    let gone: Vec<String> = anvil::lock(&app.apps)
        .process_keys(&s.name)
        .into_iter()
        .filter(|k| k.contains('@') && !listed.contains(k))
        .collect();
    for k in gone {
        stop_app(&app, &k);
        anvil::lock(&app.app_errors).remove(&k);
    }
    if routes(&cfg, &s) != before {
        reroute_later(&handle);
    }
    changed(&handle);
    Ok(overview(&app, &cfg, &s))
}

/// Disk, processes and services for one app. Sizing big folders takes a moment,
/// so it runs off the main thread and only holds locks for the quick parts.
#[tauri::command]
async fn resources(handle: AppHandle, site: String) -> Res<anvil::resources::Resources> {
    tauri::async_runtime::spawn_blocking(move || {
        let (cfg, s) = find_site_cfg(&site)?;
        let (total, items) = anvil::resources::disk(&s);
        let app = handle.state::<App>();
        let pid = anvil::lock(&app.apps).pid(&s.name);
        let processes = anvil::resources::processes(&s, pid);
        let live = anvil::lock(&app.services).pids();
        let services = anvil::resources::services_used(&cfg, &s, &live);
        Ok(anvil::resources::Resources {
            total,
            items,
            processes,
            services,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Branch, changes and last commit; `None` when the app is not a git repository.
#[tauri::command]
async fn git_info(site: String) -> Res<Option<anvil::git::GitInfo>> {
    tauri::async_runtime::spawn_blocking(move || {
        let s = find_site(&site)?;
        Ok(anvil::git::info(&s.path))
    })
    .await
    .map_err(|e| e.to_string())?
}

fn site_dir(site: &str) -> Res<std::path::PathBuf> {
    find_site(site).map(|s| s.path)
}

/// Dependencies from the app's manifests and lock files, cached until they change.
#[tauri::command]
async fn packages(site: String) -> Res<anvil::packages::Packages> {
    tauri::async_runtime::spawn_blocking(move || Ok(anvil::packages::cached(&site_dir(&site)?)))
        .await
        .map_err(|e| e.to_string())?
}

/// Newer versions of the app's direct dependencies, asked from each registry.
#[tauri::command]
async fn package_updates(site: String) -> Res<Vec<anvil::packages::Update>> {
    tauri::async_runtime::spawn_blocking(move || {
        Ok(anvil::packages::updates(&anvil::packages::cached(
            &site_dir(&site)?,
        )))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Known vulnerabilities in one ecosystem of the app, from its package manager's audit.
#[tauri::command]
async fn package_audit(site: String, ecosystem: String) -> Res<Vec<anvil::packages::Advisory>> {
    tauri::async_runtime::spawn_blocking(move || {
        let (cfg, s) = find_site_cfg(&site)?;
        anvil::packages::audit(&s.path, &ecosystem, &cfg).map_err(err)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Delete one cleanable item of an app (see `resources::clean` for the guards).
#[tauri::command]
async fn clean(handle: AppHandle, site: String, path: String) -> Res<u64> {
    tauri::async_runtime::spawn_blocking(move || {
        let s = find_site(&site)?;
        let running = anvil::lock(&handle.state::<App>().apps).running(&s.name);
        anvil::resources::clean(&s, &path, running).map_err(err)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Run `f` off the main thread with the app named `site`.
async fn with_site<T: Send + 'static>(
    site: String,
    f: impl FnOnce(Site) -> anyhow::Result<T> + Send + 'static,
) -> Res<T> {
    tauri::async_runtime::spawn_blocking(move || {
        let s = find_site(&site)?;
        f(s).map_err(err)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// A log path, only if it is one of the app's own sources.
fn log_path(s: &Site, source: &str) -> anyhow::Result<std::path::PathBuf> {
    Ok(std::path::PathBuf::from(anvil::logs::find(s, source)?.path))
}

#[tauri::command]
async fn log_sources(site: String) -> Res<Vec<anvil::logs::LogSource>> {
    with_site(site, |s| Ok(anvil::logs::sources(&s))).await
}

#[tauri::command]
async fn log_read(site: String, source: String, before: Option<u64>) -> Res<anvil::logs::Page> {
    with_site(site, move |s| {
        anvil::logs::read(&log_path(&s, &source)?, before, anvil::logs::PAGE)
    })
    .await
}

#[tauri::command]
async fn log_tail(site: String, source: String, from: u64) -> Res<anvil::logs::Page> {
    with_site(site, move |s| {
        anvil::logs::tail(&log_path(&s, &source)?, from)
    })
    .await
}

/// The console's view of a process's output: raw, so colors survive.
#[tauri::command]
async fn log_text(site: String, source: String, from: u64) -> Res<anvil::logs::Text> {
    with_site(site, move |s| {
        anvil::logs::tail_text(&log_path(&s, &source)?, from)
    })
    .await
}

#[tauri::command]
async fn log_search(
    site: String,
    source: String,
    query: String,
    level: Option<String>,
    before: Option<u64>,
) -> Res<anvil::logs::Page> {
    with_site(site, move |s| {
        anvil::logs::search(
            &log_path(&s, &source)?,
            &query,
            level.as_deref(),
            before,
            500,
        )
    })
    .await
}

/// The Requests tab: a page of the app's requests that match `filter`, newest first.
#[tauri::command]
async fn requests_read(
    site: String,
    filter: anvil::requests::Filter,
    before: Option<u64>,
) -> Res<anvil::requests::Page> {
    with_site(site, move |s| {
        anvil::requests::read(&anvil::requests::file(&s.name), &filter, before, 200)
    })
    .await
}

/// The Requests tab while it's shown: the app's requests since `from`.
#[tauri::command]
async fn requests_tail(
    site: String,
    filter: anvil::requests::Filter,
    from: u64,
) -> Res<anvil::requests::Page> {
    with_site(site, move |s| {
        anvil::requests::tail(&anvil::requests::file(&s.name), from, &filter)
    })
    .await
}

#[tauri::command]
async fn log_clear(site: String, source: String) -> Res<()> {
    with_site(site, move |s| anvil::logs::clear(&s, &source)).await
}

#[tauri::command]
async fn log_open(site: String, source: String) -> Res<()> {
    with_site(site, move |s| {
        let log = log_path(&s, &source)?;
        anvil::run(&mut if cfg!(windows) {
            anvil::platform::open(log)
        } else {
            anvil::platform::open_with("Console", log)
        })
    })
    .await
}

/// A stack frame's file at its line, in the editor from Settings. Only files inside
/// the app's folder; an editor whose CLI isn't installed opens the file without the line.
#[tauri::command]
async fn open_in_editor(site: String, path: String, line: u32) -> Res<()> {
    with_site(site, move |s| {
        let editor = anvil::load()?
            .editor
            .ok_or_else(|| anyhow::anyhow!("No editor set in Settings"))?;
        let file = anvil::validate::file_in(std::path::Path::new(&s.path), &path)?;
        let (bin, args) = anvil::logs::editor_command(&editor, &file, line);
        // Not waited on here: a launcher script may stay until the editor quits.
        if let Ok(mut child) = anvil::platform::command(&bin).args(&args).spawn() {
            std::thread::spawn(move || child.wait());
            return Ok(());
        }
        anvil::run(&mut anvil::platform::open_with(&editor, &file))
    })
    .await
}

#[derive(Serialize)]
struct AnvilStorage {
    total: u64,
    items: Vec<anvil::resources::DiskItem>,
}

#[tauri::command]
async fn storage_anvil() -> Res<AnvilStorage> {
    tauri::async_runtime::spawn_blocking(|| {
        let (total, items) = anvil::resources::anvil_disk();
        AnvilStorage { total, items }
    })
    .await
    .map_err(|e| e.to_string())
}

/// Every app's size. Each one is sent as a `storage-app` event as soon as it is
/// measured, so the list fills in while the rest are counted.
#[tauri::command]
async fn storage_apps(handle: AppHandle) -> Res<Vec<anvil::resources::AppSize>> {
    tauri::async_runtime::spawn_blocking(move || {
        let cfg = anvil::load().map_err(err)?;
        let all = anvil::resources::all_apps(&anvil::sites(&cfg), |a| {
            let _ = handle.emit("storage-app", a);
        });
        anvil::resources::save_app_sizes(&all);
        Ok(all)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The last measurement, shown at once while a new one runs.
#[tauri::command(async)]
fn storage_last() -> Option<(u64, Vec<anvil::resources::AppSize>)> {
    anvil::resources::last_app_sizes()
}

#[tauri::command]
async fn clean_anvil(path: String) -> Res<u64> {
    tauri::async_runtime::spawn_blocking(move || anvil::resources::clean_anvil(&path).map_err(err))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn clean_all_safe() -> Res<(u64, Vec<String>)> {
    tauri::async_runtime::spawn_blocking(|| {
        let cfg = anvil::load().map_err(err)?;
        Ok(anvil::resources::clean_all_safe(&anvil::sites(&cfg)))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn runtimes() -> Res<Vec<anvil::runtimes::RuntimeInfo>> {
    tauri::async_runtime::spawn_blocking(|| {
        let cfg = anvil::load().map_err(err)?;
        Ok(anvil::runtimes::RUNTIMES
            .iter()
            .map(|id| {
                let mut info = anvil::runtimes::info(id, true);
                info.pinned_by = info
                    .installed
                    .iter()
                    .map(|v| (v.clone(), anvil::pinned_to(&cfg, id, v)))
                    .filter(|(_, apps)| !apps.is_empty())
                    .collect();
                info
            })
            .collect())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// npm, pnpm, yarn and bun as the Node apps Anvil runs would find them.
#[tauri::command]
async fn package_managers() -> Res<Vec<anvil::runtimes::PackageManager>> {
    tauri::async_runtime::spawn_blocking(anvil::runtimes::package_managers)
        .await
        .map_err(|e| e.to_string())
}

/// Install pnpm, yarn or bun into Anvil's folder for the apps it runs.
#[tauri::command]
async fn package_manager_install(name: String) -> Res<()> {
    tauri::async_runtime::spawn_blocking(move || anvil::runtimes::install_pm(&name))
        .await
        .map_err(|e| e.to_string())?
        .map_err(err)
}

#[tauri::command(async)]
fn package_manager_uninstall(name: String) -> Res<()> {
    anvil::runtimes::uninstall_pm(&name).map_err(err)
}

#[tauri::command]
async fn runtime_install(handle: AppHandle, id: String, version: String) -> Res<()> {
    tauri::async_runtime::spawn_blocking(move || anvil::runtimes::install(&id, &version))
        .await
        .map_err(|e| e.to_string())?
        .map_err(err)?;
    changed(&handle);
    Ok(())
}

#[tauri::command(async)]
fn runtime_uninstall(handle: AppHandle, id: String, version: String) -> Res<()> {
    let cfg = anvil::load().map_err(err)?;
    anvil::runtime_uninstall(&cfg, &id, &version).map_err(err)?;
    changed(&handle);
    Ok(())
}

/// Create a new application from a template, serve it, and start it if it runs
/// its own server. Returns its name.
#[tauri::command]
async fn create_app(
    handle: AppHandle,
    kind: String,
    name: String,
    parent: std::path::PathBuf,
    options: Option<anvil::create::Options>,
    columns: Option<u16>,
    lines: Option<u16>,
) -> Res<String> {
    let done = create(&handle, kind, &name, parent, options, columns, lines).await;
    match &done {
        Ok(site) => notify(&handle, &format!("{site} is ready"), ""),
        Err(e) => notify(
            &handle,
            &format!("{name}: setup failed"),
            e.lines().next().unwrap_or(""),
        ),
    }
    done
}

async fn create(
    handle: &AppHandle,
    kind: String,
    name: &str,
    parent: std::path::PathBuf,
    options: Option<anvil::create::Options>,
    columns: Option<u16>,
    lines: Option<u16>,
) -> Res<String> {
    let cfg = anvil::load().map_err(err)?;
    let opts = anvil::create::resolve(&kind, &cfg, &options.unwrap_or_default()).map_err(err)?;
    let job = (kind.clone(), name.to_string(), parent.clone(), opts.clone());
    let (c, l) = anvil::create::LOG_SIZE;
    let size = (columns.unwrap_or(c), lines.unwrap_or(l));
    let dir = tauri::async_runtime::spawn_blocking(move || {
        anvil::create::create(&job.0, &job.1, &job.2, &job.3, size)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(err)?;

    let app = handle.state::<App>();
    let (cfg, site) = edit(|cfg| {
        let parked = cfg.paths.iter().any(|p| {
            anvil::platform::canonicalize(p).ok() == anvil::platform::canonicalize(&parent).ok()
        });
        let site = if parked {
            anvil::site_name(name)
        } else {
            anvil::link(cfg, name, dir)?
        };
        // The versions chosen in the sheet, when they aren't the defaults anyway.
        let defaults = anvil::create::resolve(&kind, cfg, &Default::default())?;
        if let Some(v) = opts.get("php").filter(|v| defaults.get("php") != Some(v)) {
            anvil::set_site_php(cfg, &site, Some(v.clone()))?;
        }
        if let Some(v) = opts.get("node").filter(|v| defaults.get("node") != Some(v)) {
            anvil::set_site_runtime(cfg, &site, Some(v.clone()))?;
        }
        if let Some(id) = opts.get("db").and_then(|d| d.strip_prefix("service:")) {
            let mut list = cfg.services[id].apps.clone();
            list.push(site.clone());
            anvil::services::set_apps(cfg, id, &list)?;
        }
        Ok(site)
    })?;
    sites_changed(handle, &app, &cfg);
    if anvil::apps::runs_process(&kind) {
        start_app(&app, &cfg, &site);
    }
    changed(handle);
    Ok(site)
}

/// What the New application sheet offers for a template (`create_app`'s `options`).
#[tauri::command(async)]
fn create_options(kind: String) -> Res<Vec<anvil::create::Opt>> {
    Ok(anvil::create::options(&kind, &anvil::load().map_err(err)?))
}

/// The creation log of `name` from `from` on, read while `create_app` runs.
#[tauri::command(async)]
fn create_log(name: String, from: u64) -> Res<anvil::logs::Text> {
    anvil::validate::new_app_name(&name).map_err(err)?;
    let file = anvil::create::log_file(&name);
    if !file.exists() {
        return Ok(Default::default());
    }
    anvil::logs::tail_text(&file, from).map_err(err)
}

/// A failed creation's log, shown in Finder (its half-made folder is gone).
#[tauri::command(async)]
fn create_reveal(name: String) -> Res<()> {
    anvil::validate::new_app_name(&name).map_err(err)?;
    anvil::run(&mut anvil::platform::reveal(&anvil::create::log_file(
        &name,
    )))
    .map_err(err)
}

/// Serve one project folder; its name comes from the folder.
#[tauri::command(async)]
fn link_folder(handle: AppHandle, app: State<App>, path: std::path::PathBuf) -> Res<String> {
    let path = anvil::platform::canonicalize(&path).map_err(|e| e.to_string())?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or("Pick a folder")?;
    let (cfg, name) = edit(|c| anvil::link(c, &name, path))?;
    sites_changed(&handle, &app, &cfg);
    Ok(name)
}

/// Serve every project inside a folder.
#[tauri::command(async)]
fn park_folder(handle: AppHandle, app: State<App>, path: std::path::PathBuf) -> Res<()> {
    let path = anvil::platform::canonicalize(&path).map_err(|e| e.to_string())?;
    let (cfg, ()) = edit(|c| {
        if !c.paths.contains(&path) {
            c.paths.push(path);
        }
        Ok(())
    })?;
    sites_changed(&handle, &app, &cfg);
    Ok(())
}

#[tauri::command(async)]
fn add_alias(handle: AppHandle, app: State<App>, site: String, alias: String) -> Res<Snapshot> {
    let path = find_site(&site)?.path;
    let (cfg, _) = edit(|c| anvil::link(c, &alias, path))?;
    sites_changed(&handle, &app, &cfg);
    snapshot(&app)
}

/// Set once at setup: what `prepare` tells the window through while a copy runs.
static HANDLE: std::sync::OnceLock<AppHandle> = std::sync::OnceLock::new();

/// What New Worktree offers for a worktree of the app `site`: "Copy the database too", and
/// the commands to offer once it's made.
#[tauri::command(async)]
fn worktree_new_db(site: String) -> Res<anvil::worktrees::NewDb> {
    let cfg = anvil::load().map_err(err)?;
    Ok(anvil::worktrees::new_db(&cfg, &site_in(&cfg, &site)?))
}

/// A new worktree of the app `site`, next to it (`worktrees::add`); `copy_db` unset follows the hooks.
#[tauri::command(async)]
fn worktree_add(
    handle: AppHandle,
    app: State<App>,
    site: String,
    branch: String,
    new: bool,
    copy_db: Option<bool>,
) -> Res<Snapshot> {
    let mut cfg = anvil::load().map_err(err)?;
    let main = site_in(&cfg, &site)?;
    let (folder, choice) =
        anvil::worktrees::add(&cfg, &main, &branch, new, copy_db).map_err(err)?;
    // Before anything of it runs: its first use makes the database chosen.
    cfg = edit(|c| anvil::worktrees::set_db_at(c, &folder, choice))?.0;
    sites_changed(&handle, &app, &cfg);
    snapshot(&app)
}

/// The worktree's `after_create` commands as they would run in it, to offer.
#[tauri::command(async)]
fn worktree_after_create_list(site: String) -> Res<Vec<String>> {
    let cfg = anvil::load().map_err(err)?;
    Ok(anvil::worktrees::after_create(&cfg, &site_in(&cfg, &site)?))
}

/// Run the `commands` the user was offered (`worktree_after_create_list`), once they said so:
/// refused when the worktree's would now be others. Its database first, then each in turn;
/// the first that fails stops the rest.
#[tauri::command(async)]
fn worktree_after_create(
    handle: AppHandle,
    app: State<App>,
    site: String,
    commands: Vec<String>,
) -> Res<Snapshot> {
    let cfg = anvil::load().map_err(err)?;
    let s = site_in(&cfg, &site)?;
    if anvil::worktrees::after_create(&cfg, &s) != commands {
        return Err(format!(
            "{site}'s after-create commands changed; nothing ran"
        ));
    }
    let ran = prepare(&app, &s).and_then(|()| {
        commands
            .iter()
            .try_for_each(|c| anvil::worktrees::run_after(&cfg, &s, c, true))
    });
    changed(&handle);
    ran.map_err(err)?;
    snapshot(&app)
}

/// "Rewrite with the hooks": the hooks' keys into the worktree's env files.
#[tauri::command(async)]
fn worktree_rewrite(handle: AppHandle, app: State<App>, site: String) -> Res<Snapshot> {
    let cfg = anvil::load().map_err(err)?;
    anvil::worktrees::rewrite(&cfg, &site_in(&cfg, &site)?).map_err(err)?;
    changed(&handle);
    snapshot(&app)
}

/// The app's Settings tab, Worktrees: its hooks as they apply, and what it sets itself.
#[derive(Serialize)]
struct AppHooks {
    hooks: anvil::worktrees::Resolved,
    own: anvil::worktrees::Hooks,
}

/// `AppHooks` of the app `site`; refused for a folder that isn't a git repository.
#[tauri::command(async)]
fn worktree_hooks(site: String) -> Res<AppHooks> {
    let cfg = anvil::load().map_err(err)?;
    let s = site_in(&cfg, &site)?;
    if anvil::git::git_dir(&s.path).is_none() {
        return Err(format!("{site} isn't a git repository"));
    }
    Ok(AppHooks {
        hooks: anvil::worktrees::hooks(&cfg, &s),
        own: cfg
            .sites
            .get(&site)
            .map(|c| c.worktree.clone())
            .unwrap_or_default(),
    })
}

/// Set one of the app's own hooks (`value` unset: the next layer's); no `field`: all of
/// them back ("Use preset"). The worktrees' env follows.
#[tauri::command(async)]
fn set_app_hooks(
    handle: AppHandle,
    app: State<App>,
    site: String,
    field: Option<String>,
    value: Option<serde_json::Value>,
) -> Res<AppHooks> {
    let (cfg, ()) = edit(|c| {
        let own = &mut c.sites.entry(site.clone()).or_default().worktree;
        match &field {
            Some(f) => anvil::worktrees::set_field(own, f, value),
            None => {
                *own = Default::default();
                Ok(())
            }
        }
    })?;
    sites_changed(&handle, &app, &cfg);
    worktree_hooks(site)
}

/// What removing the worktree deletes that git doesn't keep (`worktrees::removal_losses`).
#[tauri::command(async)]
fn worktree_losses(site: String) -> Res<Vec<String>> {
    let cfg = anvil::load().map_err(err)?;
    Ok(anvil::worktrees::removal_losses(
        &cfg,
        &site_in(&cfg, &site)?,
    ))
}

/// `remove`'s safety check first (unless `force`; refused, nothing is stopped), then stop
/// everything of the worktree (one shared deadline), then `worktrees::remove`. The config is
/// saved after a failure too: a failed drop has forgotten the databases that went.
#[tauri::command(async)]
fn worktree_remove(
    handle: AppHandle,
    app: State<App>,
    site: String,
    drop_db: bool,
    force: bool,
) -> Res<Snapshot> {
    let cfg = anvil::load().map_err(err)?;
    let wt = site_in(&cfg, &site)?;
    if wt.worktree_of.is_none() {
        return Err(format!("{site} isn't a worktree"));
    }
    let others: Vec<Site> = anvil::sites(&cfg)
        .into_iter()
        .filter(|s| s.name != site)
        .collect();
    anvil::worktrees::stop_if_clean(&wt, force, || {
        let stopping = anvil::lock(&app.apps).take_vanished(&others);
        anvil::stop_all(stopping, anvil::apps::GRACE);
    })
    .map_err(err)?;
    anvil::lock(&app.app_errors).retain(|k, _| k.split(['@', '#']).next() != Some(&site));
    let (cfg, removed) = edit(|c| Ok(anvil::worktrees::remove(c, &wt, drop_db, force)))?;
    sites_changed(&handle, &app, &cfg);
    removed.map_err(err)?;
    snapshot(&app)
}

/// Make the worktree's own database now (Overview's Prepare database), as its first Start would.
#[tauri::command(async)]
fn worktree_prepare(handle: AppHandle, app: State<App>, site: String) -> Res<Snapshot> {
    let cfg = anvil::load().map_err(err)?;
    let s = site_in(&cfg, &site)?;
    let made = prepare(&app, &s);
    changed(&handle);
    made.map_err(err)?;
    snapshot(&app)
}

/// "Use existing <db>": the worktree takes the database that is there already (`worktrees::adopt`).
#[tauri::command(async)]
fn worktree_adopt(handle: AppHandle, app: State<App>, site: String) -> Res<Snapshot> {
    let (cfg, ()) = edit(|c| {
        let s = anvil::sites(c)
            .into_iter()
            .find(|s| s.name == site && s.worktree_of.is_some())
            .ok_or_else(|| anyhow::anyhow!("No worktree named {site}"))?;
        anvil::worktrees::adopt(c, &s)
    })?;
    sites_changed(&handle, &app, &cfg);
    snapshot(&app)
}

/// Give a worktree the files its hooks copy (Overview's "Copy .env from <app>").
#[tauri::command(async)]
fn worktree_copy_files(handle: AppHandle, app: State<App>, site: String) -> Res<Snapshot> {
    let cfg = anvil::load().map_err(err)?;
    anvil::worktrees::copy_files(&cfg, &site_in(&cfg, &site)?).map_err(err)?;
    sites_changed(&handle, &app, &cfg);
    snapshot(&app)
}

/// Install a worktree's packages (`worktrees::install`); `what` is `composer` or `js`.
#[tauri::command(async)]
fn worktree_install(
    handle: AppHandle,
    app: State<App>,
    site: String,
    what: String,
) -> Res<Snapshot> {
    let cfg = anvil::load().map_err(err)?;
    let s = site_in(&cfg, &site)?;
    let done = anvil::worktrees::install(&cfg, &s, &what);
    changed(&handle);
    done.map_err(err)?;
    snapshot(&app)
}

/// Delete a database left from a removed worktree (`worktrees::drop_leftover`).
#[tauri::command(async)]
fn leftover_drop(
    handle: AppHandle,
    app: State<App>,
    service: String,
    database: String,
) -> Res<Snapshot> {
    let (_, ()) = edit(|c| {
        let sites = anvil::sites(c);
        anvil::worktrees::drop_leftover(c, &sites, &service, &database)
    })?;
    changed(&handle);
    snapshot(&app)
}

/// Keep a leftover's databases; Anvil forgets they were the gone worktree's.
#[tauri::command(async)]
fn leftover_keep(
    handle: AppHandle,
    state: State<App>,
    app: String,
    worktree: String,
) -> Res<Snapshot> {
    let (_, ()) = edit(|c| {
        let sites = anvil::sites(c);
        anvil::worktrees::keep_leftover(c, &sites, &app, &worktree);
        Ok(())
    })?;
    changed(&handle);
    snapshot(&state)
}

/// Unlinks linked names and hides parked folders. Never deletes files.
#[tauri::command(async)]
fn remove_site(handle: AppHandle, app: State<App>, site: String) -> Res<Snapshot> {
    // Its processes and commands stop with it: afterwards nothing could reach them.
    let keys = anvil::lock(&app.apps).process_keys(&site);
    for key in keys {
        stop_app(&app, &key);
    }
    let (cfg, ()) = edit(|c| anvil::remove(c, &site))?;
    sites_changed(&handle, &app, &cfg);
    snapshot(&app)
}

/// What still needs doing before `.test` addresses work: resolver, certificate, PHP.
#[tauri::command(async)]
fn setup_status() -> Res<anvil::Setup> {
    Ok(anvil::setup_status(&anvil::load().map_err(err)?))
}

/// Every system step still missing, behind one password prompt: resolver file,
/// ports daemon, the certificate authority trusted system-wide (Caddy is started
/// first when it hasn't made one yet), then the terminal shims.
fn setup_all(app: &App, cfg: &Config) -> Res<()> {
    let root = anvil::root_cert();
    if !root.exists() {
        if anvil::lock(&app.server).is_none() {
            start(app, cfg);
        }
        for _ in 0..50 {
            if root.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    }
    // After Caddy: `trusted` counts a missing authority as trusted.
    let status = anvil::setup_status(cfg);
    let ports = if status.ports {
        None
    } else {
        Some(anvil::stage_ports_daemon(cfg).map_err(err)?)
    };
    let cert = (root.exists() && !status.trusted).then_some(root.as_path());
    let script = anvil::setup_script(cfg, !status.resolver, ports.as_deref(), cert);
    let res = script.map_err(err).and_then(|script| {
        if script.is_empty() {
            return Ok(());
        }
        let prompt = format!(
            "Anvil sets up .{} addresses and HTTPS on this Mac.",
            cfg.tld
        );
        anvil::run_as_admin(&script, &prompt).map_err(err)
    });
    if let Some(p) = ports {
        let _ = std::fs::remove_file(p);
    }
    res?;
    anvil::shims::setup().map_err(err)?;
    Ok(())
}

/// Do one setup step. The resolver file needs an administrator: macOS asks for
/// the password in its own dialog. "all" does every step still missing.
#[tauri::command(async)]
fn setup_fix(handle: AppHandle, app: State<App>, step: String) -> Res<anvil::Setup> {
    let mut cfg = anvil::load().map_err(err)?;
    // `php_install` saves through `anvil::update` itself, after the download.
    match step.as_str() {
        "all" => setup_all(&app, &cfg)?,
        "resolver" => {
            let ok =
                |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
            if !ok(&cfg.tld) {
                return Err(format!("{} isn't a valid domain suffix", cfg.tld));
            }
            let script = anvil::setup_script(&cfg, true, None, None).map_err(err)?;
            let prompt = format!("Anvil sets up .{} addresses on this Mac.", cfg.tld);
            anvil::run_as_admin(&script, &prompt).map_err(err)?;
        }
        // Windows: Caddy listens on 80 and 443 itself.
        "ports" if cfg!(windows) => {}
        "ports" => {
            let (file, _) = anvil::ports_daemon(&cfg);
            let tmp = anvil::stage_ports_daemon(&cfg).map_err(err)?;
            let q = |p: &std::path::Path| anvil::shims::sh_quote(&p.to_string_lossy());
            let (tmp, file, label) = (q(&tmp), q(&file), anvil::PORTS_LABEL);
            // bootout first: a changed port reloads the daemon.
            let script = format!(
                "launchctl bootout system/{label} 2>/dev/null; cp {tmp} {file} && chown root:wheel {file} && chmod 644 {file} && launchctl bootstrap system {file}"
            );
            let res =
                anvil::run_as_admin(&script, "Anvil lets addresses work without a port number.");
            let _ = std::fs::remove_file(anvil::home().join("run").join("ports.plist"));
            res.map_err(err)?;
        }
        "trust" => anvil::trust().map_err(err)?,
        "php" => anvil::php_install(&mut cfg, "8.4").map_err(err)?,
        other => return Err(format!("Unknown setup step {other}")),
    }
    changed(&handle);
    setup_status()
}

#[tauri::command(async)]
fn unhide_site(handle: AppHandle, app: State<App>, site: String) -> Res<Snapshot> {
    let (cfg, ()) = edit(|c| anvil::unhide(c, &site))?;
    sites_changed(&handle, &app, &cfg);
    snapshot(&app)
}

/// Pin an app to a language version (`None`: from the project, or the newest).
#[tauri::command(async)]
fn set_runtime(
    handle: AppHandle,
    app: State<App>,
    site: String,
    version: Option<String>,
) -> Res<Snapshot> {
    let (cfg, ()) = edit(|c| anvil::set_site_runtime(c, &site, version))?;
    let was_running = anvil::lock(&app.apps).running(&site);
    if was_running {
        stop_app(&app, &site);
        start_app(&app, &cfg, &site);
    }
    changed(&handle);
    snapshot(&app)
}

#[tauri::command(async)]
fn set_site_ini(
    handle: AppHandle,
    app: State<App>,
    site: String,
    key: String,
    value: Option<String>,
) -> Res<Snapshot> {
    let (cfg, ()) = edit(|c| anvil::set_site_ini(c, &site, &key, value.as_deref()))?;
    sites_changed(&handle, &app, &cfg);
    snapshot(&app)
}

/// Let a Next.js app answer at `.<tld>` by adding it to `allowedDevOrigins`;
/// Next restarts itself when its config changes.
#[tauri::command(async)]
fn allow_next_origin(handle: AppHandle, app: State<App>, site: String) -> Res<Snapshot> {
    let (cfg, s) = find_site_cfg(&site)?;
    anvil::apps::allow_next_origin(&s.path, &cfg.tld).map_err(err)?;
    changed(&handle);
    snapshot(&app)
}

/// HTTPS on or off for one site.
#[tauri::command(async)]
fn set_secure(handle: AppHandle, app: State<App>, site: String, on: bool) -> Res<Snapshot> {
    let (cfg, ()) = edit(|c| anvil::set_secure(c, &site, on))?;
    sites_changed(&handle, &app, &cfg);
    snapshot(&app)
}

/// The app's web root, relative to its project folder; `None` goes back to the detected one.
#[tauri::command(async)]
fn set_root(
    handle: AppHandle,
    app: State<App>,
    site: String,
    dir: Option<String>,
) -> Res<Snapshot> {
    let (cfg, ()) = edit(|c| anvil::set_site_root(c, &site, dir.as_deref()))?;
    sites_changed(&handle, &app, &cfg);
    snapshot(&app)
}

/// Whether sites without their own choice get HTTPS.
#[tauri::command(async)]
fn set_https_default(handle: AppHandle, on: bool) -> Res<Settings> {
    apply(
        &handle,
        |cfg| {
            anvil::set_https_default(cfg, on);
            Ok(())
        },
        true,
    )
}

#[tauri::command(async)]
fn set_group(app: State<App>, site: String, group: Option<String>) -> Res<Snapshot> {
    edit(|c| anvil::set_group(c, &site, group.as_deref()))?;
    snapshot(&app)
}

/* ---------- Services ---------- */

#[derive(Serialize)]
struct ServiceRow {
    #[serde(flatten)]
    info: anvil::services::ServiceInfo,
    error: Option<String>,
    /// Installed database apps that can open it.
    clients: Vec<&'static str>,
}

#[tauri::command(async)]
fn services(app: State<App>) -> Res<Vec<ServiceRow>> {
    let cfg = anvil::load().map_err(err)?;
    let errors = anvil::lock(&app.service_errors).clone();
    Ok(anvil::lock(&app.services)
        .info(&cfg)
        .into_iter()
        .map(|info| ServiceRow {
            error: errors.get(&info.id).cloned(),
            clients: anvil::databases::clients(info.kind),
            info,
        })
        .collect())
}

/// Start a service with `services` locked only around the bookkeeping: `launch` takes
/// at least 300 ms, a first MySQL init much longer.
fn start_service(app: &App, cfg: &Config, id: &str) {
    let begun = anvil::lock(&app.services).begin(cfg, id);
    let result = match begun {
        Ok(true) => {
            let launched = anvil::services::launch(cfg, id);
            anvil::lock(&app.services).finish(id, launched)
        }
        Ok(false) => Ok(()),
        Err(e) => Err(e),
    };
    let mut errors = anvil::lock(&app.service_errors);
    match result {
        Ok(()) => errors.remove(id),
        Err(e) => errors.insert(id.into(), err(e)),
    };
}

#[tauri::command(async)]
fn set_service_port(
    handle: AppHandle,
    app: State<App>,
    id: String,
    port: Option<u16>,
    web: Option<bool>,
) -> Res<Vec<ServiceRow>> {
    let (cfg, ()) = edit(|c| {
        if web == Some(true) {
            anvil::services::set_web_port(c, &id, port)
        } else {
            anvil::services::set_port(c, &id, port)
        }
    })?;
    let was_running = anvil::lock(&app.services).running(&id);
    stop_service(&app, &id);
    if was_running || cfg.services.get(&id).is_some_and(|s| s.enabled) {
        start_service(&app, &cfg, &id);
    }
    changed(&handle);
    services(app)
}

/// Start, stop or restart a service. Starting also makes it start with Anvil.
#[tauri::command(async)]
fn service_run(
    handle: AppHandle,
    app: State<App>,
    id: String,
    action: String,
) -> Res<Vec<ServiceRow>> {
    let (cfg, ()) = if action == "restart" {
        (anvil::load().map_err(err)?, ())
    } else {
        edit(|c| anvil::services::set_enabled(c, &id, action == "start"))?
    };
    if action != "start" {
        stop_service(&app, &id);
        anvil::lock(&app.service_errors).remove(&id);
    }
    if action != "stop" {
        start_service(&app, &cfg, &id);
    }
    changed(&handle);
    services(app)
}

#[tauri::command(async)]
fn set_service_tags(
    handle: AppHandle,
    app: State<App>,
    id: String,
    tags: Vec<String>,
) -> Res<Vec<ServiceRow>> {
    edit(|c| anvil::services::set_tags(c, &id, &tags))?;
    changed(&handle);
    services(app)
}

#[tauri::command(async)]
fn set_service_apps(
    handle: AppHandle,
    app: State<App>,
    id: String,
    apps: Vec<String>,
) -> Res<Vec<ServiceRow>> {
    edit(|c| anvil::services::set_apps(c, &id, &apps))?;
    changed(&handle);
    services(app)
}

/// `what` is "data": the service's data folder in Finder.
#[tauri::command(async)]
fn open_service(id: String, what: String) -> Res<()> {
    let cfg = anvil::load().map_err(err)?;
    if !cfg.services.contains_key(&id) {
        return Err(format!("There's no service called {id}"));
    }
    // The log is in the Processes window (show_processes with the service's key).
    if what != "data" {
        return Err(format!("Can't open {what}"));
    }
    let path = anvil::services::data(&id);
    if !path.exists() {
        return Err(format!("Nothing there yet — start {id} first"));
    }
    anvil::run(&mut anvil::platform::open(path)).map_err(err)
}

/// Databases, keys or messages in a running service.
#[tauri::command]
async fn service_contents(id: String) -> Res<anvil::databases::Contents> {
    tauri::async_runtime::spawn_blocking(move || anvil::databases::contents(&anvil::load()?, &id))
        .await
        .map_err(|e| e.to_string())?
        .map_err(err)
}

/// `action`: "create" or "drop" a database, or "empty" Redis/Mailpit.
#[tauri::command]
async fn service_data(
    id: String,
    action: String,
    name: Option<String>,
) -> Res<anvil::databases::Contents> {
    tauri::async_runtime::spawn_blocking(move || {
        let cfg = anvil::load()?;
        let name = name.unwrap_or_default();
        match action.as_str() {
            "create" => anvil::databases::create(&cfg, &id, &name)?,
            "drop" => anvil::databases::drop_database(&cfg, &id, &name)?,
            _ => anvil::databases::empty(&cfg, &id)?,
        }
        anvil::databases::contents(&cfg, &id)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(err)
}

/// Open a service (or one of its databases) in an installed database app.
#[tauri::command(async)]
fn open_in_client(id: String, client: String, database: Option<String>) -> Res<()> {
    let cfg = anvil::load().map_err(err)?;
    let kind = anvil::services::kind_of(&cfg, &id).map_err(err)?;
    if !anvil::databases::clients(kind).contains(&client.as_str()) {
        return Err(format!("{client} isn't installed"));
    }
    let url = anvil::databases::url(&cfg, &id, database.as_deref()).map_err(err)?;
    anvil::run(&mut anvil::platform::open_with(&client, url)).map_err(err)
}

#[tauri::command(async)]
fn service_kinds() -> Res<Vec<anvil::services::Kind>> {
    Ok(anvil::services::kinds(&anvil::load().map_err(err)?))
}

/// Like creating a container: downloads the program the first time, then starts it.
#[tauri::command]
#[allow(clippy::too_many_arguments)] // one per field of the Add service sheet
async fn add_service(
    handle: AppHandle,
    kind: String,
    name: Option<String>,
    port: Option<u16>,
    version: Option<String>,
    tags: Vec<String>,
    apps: Vec<String>,
    accept_license: Option<bool>,
) -> Res<Vec<ServiceRow>> {
    let (job_cfg, id) = edit(|c| {
        // Nothing is installed before its license is accepted, here or earlier.
        let v = version
            .clone()
            .unwrap_or_else(|| anvil::services::default_version(&kind).to_string());
        anvil::services::require_license(c, &kind, &v, accept_license.unwrap_or(false))?;
        let id = anvil::services::add(c, &kind, name.as_deref(), port, version.as_deref())?;
        let meta = anvil::services::set_tags(c, &id, &tags)
            .and_then(|_| anvil::services::set_apps(c, &id, &apps));
        if let Err(e) = meta {
            anvil::services::remove(c, &id, false)
                .map_err(|undo| undo.context(format!("{e:#}; and it couldn't be removed again")))?;
            return Err(e);
        }
        Ok(id)
    })?;
    let job_id = id.clone();
    let installed = tauri::async_runtime::spawn_blocking(move || {
        anvil::services::ensure_installed(&job_cfg, &job_id)
    })
    .await
    .map_err(|e| e.to_string())?;
    // The install can take minutes; the change goes on top of the config as it is now.
    if let Err(e) = installed {
        if let Err(undo) = edit(|c| anvil::services::remove(c, &id, false)) {
            return Err(format!(
                "{}; and the service couldn't be removed again: {undo}",
                err(e)
            ));
        }
        return Err(err(e));
    }
    let (cfg, ()) = edit(|c| anvil::services::set_enabled(c, &id, true))?;
    let app = handle.state::<App>();
    start_service(&app, &cfg, &id);
    changed(&handle);
    services(app)
}

#[tauri::command(async)]
fn remove_service(
    handle: AppHandle,
    app: State<App>,
    id: String,
    delete_data: bool,
) -> Res<Vec<ServiceRow>> {
    // Stop first so the data folder isn't in use when it's deleted.
    stop_service(&app, &id);
    anvil::lock(&app.service_errors).remove(&id);
    edit(|c| anvil::services::remove(c, &id, delete_data))?;
    changed(&handle);
    services(app)
}

#[tauri::command(async)]
fn open_url(url: String) -> Res<()> {
    let tld = anvil::load().map(|c| c.tld).unwrap_or_default();
    anvil::validate::link(&url, &tld).map_err(err)?;
    anvil::run(&mut anvil::platform::open(url)).map_err(err)
}

/* ---------- Settings ---------- */

#[derive(Serialize)]
struct PhpVersion {
    version: &'static str,
    installed: bool,
    default: bool,
    /// Apps pinned to it: it can't be uninstalled until they are unpinned.
    pinned_by: Vec<String>,
    opcache: anvil::opcache::State,
    /// An OPcache build for it is running, so a window opened meanwhile says so too.
    opcache_building: bool,
    /// The catalog (`phpext::CATALOG`), each on or off for every app on this version.
    extensions: Vec<PhpExt>,
}

#[derive(Serialize)]
struct PhpExt {
    name: &'static str,
    on: bool,
    /// Built for the installed patch release; on without it loads nothing.
    built: bool,
    /// Being built by this app, so a window opened meanwhile says so too.
    building: bool,
}

#[derive(Serialize)]
struct Settings {
    paths: Vec<std::path::PathBuf>,
    hidden: Vec<String>,
    tld: String,
    http_port: u16,
    https_port: u16,
    dns_port: u16,
    editor: Option<String>,
    editors: Vec<String>,
    php: Vec<PhpVersion>,
    ini: std::collections::BTreeMap<String, String>,
    terminal: Option<String>,
    terminals: Vec<String>,
    git_client: Option<String>,
    git_clients: Vec<String>,
    theme: Option<String>,
    dock_icon: bool,
    trim_app_logs: bool,
    notifications: bool,
    https_default: bool,
    worktree_db: anvil::worktrees::DbChoice,
    /// Settings › Plugins › Agents (MCP).
    mcp: bool,
    /// Settings › Worktrees: each preset as it applies.
    worktree_presets: Vec<PresetView>,
    login: bool,
    config_dir: std::path::PathBuf,
}

/// A framework preset in Settings › Worktrees.
#[derive(Serialize)]
struct PresetView {
    id: &'static str,
    name: &'static str,
    /// The kinds of app that use it, by name.
    kinds: Vec<&'static str>,
    hooks: anvil::worktrees::Hooks,
    /// The user changed it: "Reset to default" is offered.
    edited: bool,
}

const TERMINALS: [&str; 8] = [
    "Terminal",
    "iTerm",
    "Warp",
    "Ghostty",
    "WezTerm",
    "kitty",
    "Alacritty",
    "Hyper",
];

const EDITORS: [&str; 10] = [
    "PhpStorm",
    "Visual Studio Code",
    "Cursor",
    "Zed",
    "Sublime Text",
    "Nova",
    "WebStorm",
    "IntelliJ IDEA",
    "Windsurf",
    "Fleet",
];

const GIT_CLIENTS: [&str; 9] = [
    "GitHub Desktop",
    "Fork",
    "Tower",
    "Sourcetree",
    "GitKraken",
    "Sublime Merge",
    "SmartGit",
    "GitUp",
    "Gitfox",
];

/// An app's display name: "Fork" stays, "/Applications/Some App.app" becomes "Some App".
fn app_label(app: &str) -> String {
    std::path::Path::new(app)
        .file_stem()
        .map_or(app.to_string(), |s| s.to_string_lossy().into_owned())
}

/// The chosen git app, or the first one installed.
fn git_client(cfg: &Config) -> Option<String> {
    cfg.git_client
        .clone()
        .or_else(|| installed_apps(&GIT_CLIENTS).into_iter().next())
}

/// Which of `apps` are installed in the usual macOS app folders.
fn installed_apps(apps: &[&str]) -> Vec<String> {
    let user = anvil::user_home().join("Applications");
    let dirs = [
        std::path::Path::new("/Applications"),
        std::path::Path::new("/Applications/Utilities"),
        std::path::Path::new("/System/Applications/Utilities"),
        user.as_path(),
    ];
    apps.iter()
        .filter(|a| dirs.iter().any(|d| d.join(format!("{a}.app")).exists()))
        .map(|a| a.to_string())
        .collect()
}

fn apply_theme(handle: &AppHandle, theme: Option<&str>) {
    let theme = match theme {
        Some("light") => Some(tauri::Theme::Light),
        Some("dark") => Some(tauri::Theme::Dark),
        _ => None,
    };
    for w in handle.webview_windows().values() {
        let _ = w.set_theme(theme);
    }
}

/// With the Dock icon off, Anvil only shows in the Dock while a window is open.
fn update_dock(handle: &AppHandle) {
    #[cfg(not(target_os = "macos"))]
    let _ = handle;
    #[cfg(target_os = "macos")]
    {
        let keep = anvil::load().map(|c| c.dock_icon).unwrap_or(false);
        let visible = handle
            .webview_windows()
            .values()
            .any(|w| w.is_visible().unwrap_or(false));
        let _ = handle.set_activation_policy(if keep || visible {
            tauri::ActivationPolicy::Regular
        } else {
            tauri::ActivationPolicy::Accessory
        });
    }
}

#[tauri::command(async)]
fn settings(handle: AppHandle) -> Res<Settings> {
    use tauri_plugin_autostart::ManagerExt;
    let cfg = anvil::load().map_err(err)?;
    let installed = anvil::installed_php();
    let fwd = anvil::ports_forwarded(&cfg);
    let worktree_presets = anvil::worktrees::PRESETS
        .iter()
        .map(|(id, name, kinds)| PresetView {
            id,
            name,
            kinds: kinds.iter().map(|k| anvil::apps::label(k)).collect(),
            hooks: anvil::worktrees::preset(&cfg, id),
            edited: cfg.worktree_presets.contains_key(*id),
        })
        .collect();
    Ok(Settings {
        php: anvil::PHP_VERSIONS
            .iter()
            .map(|v| PhpVersion {
                version: v,
                installed: installed.iter().any(|i| i == v),
                default: cfg.php.as_deref() == Some(*v),
                pinned_by: anvil::pinned_to(&cfg, "php", v),
                opcache: anvil::opcache::state(v),
                opcache_building: anvil::lock(&OPCACHE_BUILDS).iter().any(|b| b == v),
                extensions: anvil::phpext::offered()
                    .map(|e| PhpExt {
                        name: e.name,
                        on: anvil::phpext::on(&cfg, v, e.name),
                        built: anvil::phpext::built(v, e.name),
                        building: anvil::lock(&EXT_BUILDS).contains(&(v.to_string(), e.name)),
                    })
                    .collect(),
            })
            .collect(),
        editors: installed_apps(&EDITORS),
        terminals: installed_apps(&TERMINALS),
        git_clients: installed_apps(&GIT_CLIENTS),
        git_client: cfg.git_client.clone(),
        login: handle.autolaunch().is_enabled().unwrap_or(false),
        config_dir: anvil::home(),
        terminal: cfg.terminal,
        theme: cfg.theme,
        dock_icon: cfg.dock_icon,
        trim_app_logs: cfg.trim_app_logs,
        notifications: cfg.notifications,
        https_default: cfg.https_default == Some(true),
        worktree_db: cfg.worktree_db,
        mcp: cfg.mcp,
        worktree_presets,
        hidden: cfg.hidden.clone(),
        paths: cfg.paths,
        tld: cfg.tld,
        // The ports addresses use: 80 and 443 once forwarded.
        http_port: if fwd { 80 } else { cfg.http_port },
        https_port: if fwd { 443 } else { cfg.https_port },
        dns_port: cfg.dns_port,
        editor: cfg.editor,
        ini: cfg.ini,
    })
}

/// Save a config change; re-serve when it affects what is served.
fn apply(
    handle: &AppHandle,
    f: impl FnOnce(&mut Config) -> anyhow::Result<()>,
    reserve: bool,
) -> Res<Settings> {
    let (cfg, ()) = edit(f)?;
    let app = handle.state::<App>();
    if reserve && running(&app) {
        start(&app, &cfg);
    }
    changed(handle);
    settings(handle.clone())
}

#[tauri::command(async)]
fn add_path(handle: AppHandle, path: std::path::PathBuf) -> Res<Settings> {
    apply(
        &handle,
        |cfg| {
            let path = anvil::platform::canonicalize(&path)?;
            if !cfg.paths.contains(&path) {
                cfg.paths.push(path);
            }
            Ok(())
        },
        true,
    )
}

#[tauri::command(async)]
fn remove_path(handle: AppHandle, path: std::path::PathBuf) -> Res<Settings> {
    apply(
        &handle,
        |cfg| {
            cfg.paths.retain(|p| p != &path);
            Ok(())
        },
        true,
    )
}

#[tauri::command(async)]
fn set_editor(handle: AppHandle, editor: Option<String>) -> Res<Settings> {
    apply(
        &handle,
        |cfg| {
            cfg.editor = editor;
            Ok(())
        },
        false,
    )
}

#[tauri::command(async)]
fn set_terminal(handle: AppHandle, terminal: Option<String>) -> Res<Settings> {
    apply(
        &handle,
        |cfg| {
            cfg.terminal = terminal;
            Ok(())
        },
        false,
    )
}

#[tauri::command(async)]
fn set_git_client(handle: AppHandle, git_client: Option<String>) -> Res<Settings> {
    // Any app can be picked by path ("Other app…"); it has to be a real .app bundle.
    if let Some(c) = git_client.as_deref().filter(|c| c.contains('/')) {
        let p = std::path::Path::new(c);
        if p.extension().is_none_or(|e| e != "app") || !p.is_dir() {
            return Err(format!("{c} isn't an app"));
        }
    }
    apply(
        &handle,
        |cfg| {
            cfg.git_client = git_client;
            Ok(())
        },
        false,
    )
}

#[tauri::command(async)]
fn set_theme(handle: AppHandle, theme: Option<String>) -> Res<Settings> {
    apply_theme(&handle, theme.as_deref());
    apply(
        &handle,
        |cfg| {
            cfg.theme = theme;
            Ok(())
        },
        false,
    )
}

/// Change the domain suffix and serve the new addresses.
#[tauri::command(async)]
fn set_tld(handle: AppHandle, tld: String) -> Res<Settings> {
    let settings = apply(&handle, |cfg| anvil::set_tld(cfg, &tld), true)?;
    // Running dev servers were told which hosts to accept; start them again with the new one.
    let app = handle.state::<App>();
    let cfg = anvil::load().map_err(err)?;
    let names = anvil::lock(&app.apps).names();
    for name in names {
        stop_app(&app, &name);
        start_app(&app, &cfg, &name);
    }
    changed(&handle);
    Ok(settings)
}

#[tauri::command(async)]
fn set_dock_icon(handle: AppHandle, on: bool) -> Res<Settings> {
    let s = apply(
        &handle,
        |cfg| {
            cfg.dock_icon = on;
            Ok(())
        },
        false,
    );
    update_dock(&handle);
    s
}

/// Settings › Worktrees › Copy the database: the hooks' last `copy_database` layer, also
/// what one opened outside Anvil uses until it has its own, so the PHP-FPM pools follow.
#[tauri::command(async)]
fn set_worktree_db(handle: AppHandle, choice: anvil::worktrees::DbChoice) -> Res<Settings> {
    apply(
        &handle,
        |cfg| {
            cfg.worktree_db = choice;
            Ok(())
        },
        true,
    )
}

/// Settings › Worktrees: one field of a preset (`value` unset: Anvil's); no `field`:
/// the whole preset back to Anvil's ("Reset to default"). The worktrees' env follows.
#[tauri::command(async)]
fn set_worktree_preset(
    handle: AppHandle,
    id: String,
    field: Option<String>,
    value: Option<serde_json::Value>,
) -> Res<Settings> {
    if !anvil::worktrees::PRESETS.iter().any(|p| p.0 == id) {
        return Err(format!("No preset named {id}"));
    }
    apply(
        &handle,
        |cfg| {
            if let Some(f) = &field {
                let mut h = cfg.worktree_presets.get(&id).cloned().unwrap_or_default();
                anvil::worktrees::set_field(&mut h, f, value)?;
                cfg.worktree_presets.insert(id.clone(), h);
            }
            if field.is_none() || cfg.worktree_presets.get(&id).is_some_and(|h| h.is_empty()) {
                cfg.worktree_presets.remove(&id);
            }
            Ok(())
        },
        true,
    )
}

/// Settings › Plugins › Agents (MCP): whether `anvil mcp` serves, and the socket answers it.
#[tauri::command(async)]
fn set_mcp(handle: AppHandle, on: bool) -> Res<Settings> {
    apply(
        &handle,
        |cfg| {
            cfg.mcp = on;
            Ok(())
        },
        false,
    )
}

/// Settings › General › Notifications (`notify`).
#[tauri::command(async)]
fn set_notifications(handle: AppHandle, on: bool) -> Res<Settings> {
    apply(
        &handle,
        |cfg| {
            cfg.notifications = on;
            Ok(())
        },
        false,
    )
}

/// Settings › Storage › Trim app logs weekly (`anvil::logs::retention`).
#[tauri::command(async)]
fn set_trim_app_logs(handle: AppHandle, on: bool) -> Res<Settings> {
    apply(
        &handle,
        |cfg| {
            cfg.trim_app_logs = on;
            Ok(())
        },
        false,
    )
}

/// Anvil's logs capped, and apps' trimmed weekly when that is on.
fn trim_logs() {
    let Ok(cfg) = anvil::load() else {
        return;
    };
    let sites: Vec<std::path::PathBuf> = anvil::sites(&cfg).into_iter().map(|s| s.path).collect();
    anvil::logs::retention(
        &anvil::home(),
        cfg.trim_app_logs,
        &sites,
        std::time::SystemTime::now(),
    );
}

#[tauri::command(async)]
fn set_login(handle: AppHandle, on: bool) -> Res<Settings> {
    use tauri_plugin_autostart::ManagerExt;
    let auto = handle.autolaunch();
    if on { auto.enable() } else { auto.disable() }.map_err(|e| e.to_string())?;
    settings(handle)
}

/// A folder on Settings › Performance (Windows) and whether it is on a Dev Drive.
#[derive(Serialize)]
struct PerfFolder {
    name: String,
    path: String,
    dev_drive: Option<bool>,
}

#[derive(Serialize)]
struct Performance {
    /// Defender's real-time protection: "on", "off", "passive", or unknown.
    defender: Option<&'static str>,
    dev_drive_supported: bool,
    folders: Vec<PerfFolder>,
}

/// What makes file-heavy work slow on Windows, read only: Defender's state, and which of
/// Anvil's and the apps' folders sit on a Dev Drive. Anvil changes neither.
#[tauri::command(async)]
fn performance() -> Res<Performance> {
    let cfg = anvil::load().map_err(err)?;
    let folder = |name: String, path: &std::path::Path| PerfFolder {
        name,
        path: path.display().to_string(),
        dev_drive: anvil::platform::on_dev_drive(path),
    };
    let home = anvil::home();
    let mut folders = vec![folder("Anvil".into(), &home)];
    folders.extend(
        anvil::sites(&cfg)
            .iter()
            .filter(|s| s.worktree_of.is_none())
            .map(|s| folder(s.name.clone(), &s.path)),
    );
    Ok(Performance {
        defender: anvil::platform::defender(),
        dev_drive_supported: anvil::platform::dev_drive_supported(),
        folders,
    })
}

#[tauri::command(async)]
fn open_config_dir() -> Res<()> {
    anvil::run(&mut anvil::platform::open(anvil::home())).map_err(err)
}

#[tauri::command(async)]
fn set_default_php(handle: AppHandle, version: String) -> Res<Settings> {
    let (cfg, ()) = edit(|cfg| anvil::set_default_php(cfg, &version))?;
    anvil::phpext::build_missing(&cfg, &version);
    let app = handle.state::<App>();
    if running(&app) {
        start(&app, &cfg);
    }
    changed(&handle);
    settings(handle)
}

/// `site: None` changes the default for every site.
#[tauri::command(async)]
fn set_ini(
    handle: AppHandle,
    site: Option<String>,
    key: String,
    value: Option<String>,
) -> Res<Settings> {
    apply(
        &handle,
        |cfg| match &site {
            Some(site) => anvil::set_site_ini(cfg, site, &key, value.as_deref()),
            None => anvil::set_global_ini(cfg, &key, value.as_deref()),
        },
        true,
    )
}

/// Downloads take a while; run them off the main thread.
#[tauri::command]
async fn php_install(handle: AppHandle, version: String) -> Res<Settings> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut cfg = anvil::load()?;
        anvil::php_install(&mut cfg, &version)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(err)?;
    changed(&handle);
    settings(handle)
}

/// Versions whose OPcache is being built: a second click (or window) doesn't start another.
static OPCACHE_BUILDS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Builds OPcache for an installed 8.1–8.4 (minutes), then restarts PHP-FPM to load it.
#[tauri::command]
async fn php_opcache(handle: AppHandle, version: String) -> Res<Settings> {
    let h = handle.clone();
    tauri::async_runtime::spawn_blocking(move || -> Res<()> {
        if !anvil::installed_php().contains(&version) {
            return Err(format!("PHP {version} is not installed"));
        }
        {
            let mut building = anvil::lock(&OPCACHE_BUILDS);
            if building.contains(&version) {
                return Err(format!("OPcache for PHP {version} is already being added"));
            }
            building.push(version.clone());
        }
        let built = anvil::opcache::build(&version);
        anvil::lock(&OPCACHE_BUILDS).retain(|v| *v != version);
        built.map_err(err)?;
        let cfg = anvil::load().map_err(err)?;
        let app = h.state::<App>();
        if running(&app) {
            start(&app, &cfg);
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())??;
    changed(&handle);
    settings(handle)
}

/// (version, extension) pairs being built here; another process's build is refused by
/// `phpext::build`'s lock file.
static EXT_BUILDS: Mutex<Vec<(String, &'static str)>> = Mutex::new(Vec::new());

/// `turn_on` (`phpext::add` or `add_site`) for `name` on `version`, marked in
/// `EXT_BUILDS` while it builds: a second click or window is refused meanwhile.
fn ext_on(
    version: &str,
    name: &str,
    turn_on: impl FnOnce(&'static str) -> anyhow::Result<Config>,
) -> Res<Config> {
    let ext = anvil::phpext::offered()
        .find(|e| e.name == name)
        .ok_or_else(|| format!("{name} is not in Anvil's catalog"))?;
    let key = (version.to_string(), ext.name);
    let building = !anvil::phpext::built(version, ext.name);
    if building {
        let mut list = anvil::lock(&EXT_BUILDS);
        if list.contains(&key) {
            return Err(format!("{name} for PHP {version} is already being built"));
        }
        list.push(key.clone());
    }
    let added = turn_on(ext.name);
    if building {
        anvil::lock(&EXT_BUILDS).retain(|k| *k != key);
    }
    added.map_err(err)
}

/// Turns a catalog extension on (building it first unless built for this patch release)
/// or off (its build is kept) for every app on `version`, then restarts PHP-FPM.
#[tauri::command]
async fn php_ext(handle: AppHandle, version: String, name: String, on: bool) -> Res<Settings> {
    let h = handle.clone();
    tauri::async_runtime::spawn_blocking(move || -> Res<()> {
        let cfg = if on {
            ext_on(&version, &name, |n| anvil::phpext::add(&version, n))?
        } else {
            anvil::phpext::remove(&version, &name).map_err(err)?
        };
        let app = h.state::<App>();
        if running(&app) {
            start(&app, &cfg);
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())??;
    changed(&handle);
    settings(handle)
}

/// The same for one app only (its Settings tab): it then has a PHP-FPM of its own.
#[tauri::command]
async fn app_ext(handle: AppHandle, site: String, name: String, on: bool) -> Res<Snapshot> {
    let h = handle.clone();
    tauri::async_runtime::spawn_blocking(move || -> Res<()> {
        let cfg = if on {
            let s = site_in(&anvil::load().map_err(err)?, &site)?;
            let version = s
                .php
                .ok_or_else(|| format!("{site} runs on no PHP of Anvil's"))?;
            ext_on(&version, &name, |n| anvil::phpext::add_site(&site, n))?
        } else {
            anvil::phpext::remove_site(&site, &name).map_err(err)?
        };
        let app = h.state::<App>();
        if running(&app) {
            start(&app, &cfg);
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())??;
    changed(&handle);
    snapshot(&handle.state::<App>())
}

/// Opens Apple's installer for the Xcode Command Line Tools, which OPcache builds with.
#[tauri::command(async)]
fn xcode_tools_install() -> Res<()> {
    anvil::run(anvil::platform::command("xcode-select").arg("--install")).map_err(err)
}

#[tauri::command(async)]
fn php_uninstall(handle: AppHandle, version: String) -> Res<Settings> {
    apply(&handle, |cfg| anvil::php_uninstall(cfg, &version), false)
}

/// The Mac's title bar drawn over the page, as the main window has it; elsewhere the
/// system's own.
fn titlebar<'a, R: tauri::Runtime, M: Manager<R>>(
    b: WebviewWindowBuilder<'a, R, M>,
) -> WebviewWindowBuilder<'a, R, M> {
    #[cfg(target_os = "macos")]
    let b = b.title_bar_style(TitleBarStyle::Overlay).hidden_title(true);
    b
}

fn open_settings(handle: &AppHandle) {
    if let Some(w) = handle.get_webview_window("settings") {
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let built = titlebar(
        WebviewWindowBuilder::new(handle, "settings", WebviewUrl::App("settings.html".into()))
            .title("Anvil Settings")
            .inner_size(780.0, 640.0)
            .min_inner_size(640.0, 480.0),
    )
    .build();
    if built.is_ok() {
        apply_theme(handle, anvil::load().ok().and_then(|c| c.theme).as_deref());
        update_dock(handle);
    }
}

#[tauri::command(async)]
fn show_settings(handle: AppHandle) {
    open_settings(&handle);
}

/// The task-manager window: everything Anvil runs, with usage and logs. With `select`
/// (a row key such as "service:cache"), that row is shown on its tab with its log.
fn open_processes(handle: &AppHandle, select: Option<&str>) {
    if let Some(w) = handle.get_webview_window("processes") {
        if let Some(key) = select {
            let _ = handle.emit_to("processes", "activity-select", key);
        }
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    // A row key is plain (see `parse_log_key`), so it goes into the script as it is.
    let pick = select
        .filter(|k| {
            k.chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.:/@#".contains(c))
        })
        .map_or("null".into(), |k| format!("\"{k}\""));
    let built = titlebar(
        WebviewWindowBuilder::new(
            handle,
            "processes",
            WebviewUrl::App("processes.html".into()),
        )
        .initialization_script(format!("window.activitySelect = {pick};"))
        .title("Processes")
        .inner_size(820.0, 560.0)
        .min_inner_size(640.0, 420.0),
    )
    .build();
    if built.is_ok() {
        apply_theme(handle, anvil::load().ok().and_then(|c| c.theme).as_deref());
        update_dock(handle);
    }
}

#[tauri::command(async)]
fn show_processes(handle: AppHandle, select: Option<String>) {
    open_processes(&handle, select.as_deref());
}

#[tauri::command(async)]
fn open(site: String, target: String) -> Res<()> {
    let (cfg, s) = find_site_cfg(&site)?;
    let mut cmd = match target.as_str() {
        "browser" => anvil::platform::open(anvil::site_url(&cfg, &s)),
        "finder" => anvil::platform::open(&s.path),
        "root" => anvil::platform::open(&s.root),
        "terminal" => anvil::platform::terminal(cfg.terminal.as_deref(), &s.path),
        "editor" => {
            let editor = cfg.editor.ok_or("No editor set in ~/.anvil/config.toml")?;
            anvil::platform::open_with(&editor, &s.path)
        }
        "git" => {
            let client = git_client(&cfg).ok_or("No git app installed")?;
            anvil::platform::open_with(&client, &s.path)
        }
        other => return Err(format!("Unknown target {other}")),
    };
    anvil::run(&mut cmd).map_err(err)
}

/// The site's favicon as a data URL, so the webview never needs file access.
#[tauri::command(async)]
fn icon(site: String) -> Option<String> {
    use base64::Engine;
    let cfg = anvil::load().ok()?;
    let s = anvil::sites(&cfg).into_iter().find(|s| s.name == site)?;
    let path = anvil::icon(&s)?;
    let mime = match path.extension()?.to_str()? {
        "svg" => "image/svg+xml",
        "png" => "image/png",
        _ => "image/x-icon",
    };
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() > 512 * 1024 {
        return None;
    }
    Some(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

/// When the app's code last changed (unix seconds), for "edited 2 h ago".
#[tauri::command(async)]
fn touched(site: String) -> Res<Option<u64>> {
    let cfg = anvil::load().map_err(err)?;
    let s = anvil::sites(&cfg)
        .into_iter()
        .find(|s| s.name == site)
        .ok_or_else(|| format!("No application named {site}"))?;
    Ok(anvil::resources::touched(&s.path))
}

/// Every window closes; Anvil stays in the menu bar.
fn hide_windows(handle: &AppHandle) {
    for w in handle.webview_windows().values() {
        if w.label() == "main" {
            let _ = w.hide();
        } else {
            let _ = w.close();
        }
    }
    update_dock(handle);
}

fn show(handle: &AppHandle) {
    if let Some(w) = handle.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
    update_dock(handle);
}

/// The app's menu bar: every shortcut is listed where macOS users look for it.
/// Picks are sent to the main window as a `menu` event; the page acts on them
/// the same way as on its own key handling.
fn app_menu(handle: &AppHandle) -> tauri::Result<Menu<Wry>> {
    use tauri::menu::{MenuItemBuilder, SubmenuBuilder};
    let item = |id: &str, text: &str, key: Option<&str>| {
        let b = MenuItemBuilder::with_id(format!("m:{id}"), text);
        match key {
            Some(k) => b.accelerator(k).build(handle),
            None => b.build(handle),
        }
    };
    let anvil = SubmenuBuilder::new(handle, "Anvil")
        .about(None)
        .separator()
        .item(&item("settings", "Settings…", Some("CmdOrCtrl+,"))?)
        .separator()
        .services()
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        // ⌘Q closes the windows only: apps, processes and services keep running in
        // the menu bar. Quitting for real (which stops them) is a separate item.
        .item(&item(
            "close_all",
            "Close Anvil Windows",
            Some("CmdOrCtrl+Q"),
        )?)
        .item(&item(
            "quit_all",
            "Quit Anvil and Stop Everything",
            Some("CmdOrCtrl+Alt+Q"),
        )?)
        .build()?;
    let file = SubmenuBuilder::new(handle, "File")
        .item(&item("new", "New…", Some("CmdOrCtrl+N"))?)
        .item(&item("park", "Add Folder of Applications…", None)?)
        .separator()
        .close_window()
        .build()?;
    // Without these, copy and paste stop working in text fields.
    let edit = SubmenuBuilder::new(handle, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;
    let view = SubmenuBuilder::new(handle, "View")
        .item(&item("mode:apps", "Applications", None)?)
        .item(&item("mode:services", "Services", None)?)
        .separator()
        .item(&item("tab:overview", "Overview", Some("CmdOrCtrl+1"))?)
        .item(&item("tab:git", "Git", Some("CmdOrCtrl+2"))?)
        .item(&item("tab:dev", "Dev", Some("CmdOrCtrl+3"))?)
        .item(&item("tab:logs", "Logs", Some("CmdOrCtrl+4"))?)
        .item(&item("tab:packages", "Packages", Some("CmdOrCtrl+5"))?)
        .item(&item("tab:resources", "Resources", Some("CmdOrCtrl+6"))?)
        .item(&item("tab:settings", "Settings", Some("CmdOrCtrl+7"))?)
        .separator()
        .item(&item("find", "Find", Some("CmdOrCtrl+F"))?)
        .item(&item("reload", "Reload", Some("CmdOrCtrl+R"))?)
        .build()?;
    let mut application = SubmenuBuilder::new(handle, "Application")
        .item(&item(
            "open:browser",
            "Open in Browser",
            Some("CmdOrCtrl+O"),
        )?)
        .item(&item("open:editor", "Open in Editor", Some("CmdOrCtrl+E"))?)
        .item(&item(
            "open:terminal",
            "Open Terminal",
            Some("CmdOrCtrl+T"),
        )?);
    // Without a git app the item could only do nothing, as the Git tab hides its button.
    // ponytail: checked once at launch; a git app installed later shows after a restart.
    if anvil::load().is_ok_and(|cfg| git_client(&cfg).is_some()) {
        application = application.item(&item("open:git", "Open in Git App", Some("CmdOrCtrl+G"))?);
    }
    let application = application
        .item(&item("logs", "Show Logs", Some("CmdOrCtrl+L"))?)
        .build()?;
    let window = SubmenuBuilder::new(handle, "Window")
        .minimize()
        .maximize()
        .separator()
        .bring_all_to_front()
        .build()?;
    Menu::with_items(
        handle,
        &[&anvil, &file, &edit, &view, &application, &window],
    )
}

fn on_app_menu(handle: &AppHandle, id: &str) {
    let Some(action) = id.strip_prefix("m:") else {
        return;
    };
    match action {
        "settings" => return open_settings(handle),
        "close_all" => return hide_windows(handle),
        "quit_all" => return handle.exit(0),
        _ => {}
    }
    show(handle);
    if let Some(w) = handle.get_webview_window("main") {
        let _ = w.emit("menu", action);
    }
}

fn menu(handle: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let app = handle.state::<App>();
    let on = running(&app);
    let cfg = anvil::load().unwrap_or_default();
    // One scan of the folders serves the whole menu.
    let all = anvil::sites(&cfg);

    let status = if on {
        // Aliases share their app's folder; count each app once.
        let parked: std::collections::HashSet<_> =
            all.iter().filter(|s| !s.linked).map(|s| &s.path).collect();
        let n = all
            .iter()
            .filter(|s| !s.linked || !parked.contains(&s.path))
            .count();
        format!("Serving {n} applications")
    } else if anvil::lock(&app.error).is_some() {
        "Stopped, open Anvil for details".into()
    } else {
        "Stopped".into()
    };

    let item = |id: &str, text: &str, enabled: bool, key: Option<&str>| {
        MenuItem::with_id(handle, id, text, enabled, key)
    };
    let sep = || PredefinedMenuItem::separator(handle);

    let mut items: Vec<Box<dyn IsMenuItem<Wry>>> = vec![
        Box::new(item("sites", "Open Anvil", true, None)?),
        Box::new(sep()?),
        Box::new(item("status", &status, false, None)?),
        Box::new(item(
            if on { "stop" } else { "start" },
            if on { "Stop" } else { "Start" },
            true,
            None,
        )?),
        Box::new(item("restart", "Restart", on, None)?),
        Box::new(sep()?),
    ];
    // PHP versions in one submenu, so the menu doesn't grow with every install.
    let php: Vec<CheckMenuItem<Wry>> = anvil::installed_php()
        .iter()
        .rev()
        .map(|v| {
            CheckMenuItem::with_id(
                handle,
                format!("php:{v}"),
                format!("PHP {v}"),
                true,
                cfg.php.as_deref() == Some(v.as_str()),
                None::<&str>,
            )
        })
        .collect::<tauri::Result<_>>()?;
    if !php.is_empty() {
        let refs: Vec<&dyn IsMenuItem<Wry>> =
            php.iter().map(|c| c as &dyn IsMenuItem<Wry>).collect();
        items.push(Box::new(Submenu::with_items(
            handle,
            "Default PHP Version",
            true,
            &refs,
        )?));
    }
    items.extend([
        Box::new(sep()?) as Box<dyn IsMenuItem<Wry>>,
        Box::new(item("logs", "Show Anvil Logs in Finder", true, None)?),
        Box::new(item("settings", "Settings", true, Some("CmdOrCtrl+,"))?),
        Box::new(sep()?),
        Box::new(item("quit", "Quit Anvil", true, Some("CmdOrCtrl+Q"))?),
    ]);

    let refs: Vec<&dyn IsMenuItem<Wry>> = items.iter().map(|b| b.as_ref()).collect();
    Menu::with_items(handle, &refs)
}

fn on_menu(handle: &AppHandle, id: &str) {
    let app = handle.state::<App>();
    let home = anvil::home();
    match id {
        "sites" => show(handle),
        // Stopping and starting Caddy and PHP-FPM takes seconds (a download the first
        // time): off the main thread, so the menu bar doesn't beachball.
        "start" | "restart" | "stop" => {
            let (handle, stopping) = (handle.clone(), id == "stop");
            std::thread::spawn(move || {
                let app = handle.state::<App>();
                if stopping {
                    stop(&app);
                } else if let Ok(cfg) = anvil::load() {
                    start(&app, &cfg);
                }
                changed(&handle);
            });
            return;
        }
        "logs" => {
            let _ = std::fs::create_dir_all(home.join("log"));
            let _ = anvil::platform::open(home.join("log")).status();
        }
        "settings" => open_settings(handle),
        "quit" => handle.exit(0),
        _ => {
            if let Some(v) = id.strip_prefix("php:") {
                match anvil::update(|c| anvil::set_default_php(c, v)) {
                    Err(e) => *anvil::lock(&app.error) = Some(err(e)),
                    Ok((cfg, ())) if running(&app) => {
                        let (handle, v) = (handle.clone(), v.to_string());
                        std::thread::spawn(move || {
                            anvil::phpext::build_missing(&cfg, &v);
                            start(&handle.state::<App>(), &cfg);
                            changed(&handle);
                        });
                    }
                    Ok(_) => {}
                }
            }
        }
    }
    changed(handle);
}

fn main() {
    // Opened from Finder, Anvil gets launchd's short PATH; what it starts (pnpm,
    // Homebrew tools, nvm's node…) needs the one a terminal has.
    if let Some(login) = anvil::login_path() {
        let path = anvil::merge_path(&anvil::env_path(), &login);
        // SAFETY: no other thread exists yet.
        unsafe { std::env::set_var("PATH", path) };
    }
    // The binary's age now, for Relaunch to compare against.
    STARTED_EXE.get_or_init(exe_mtime);
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(App::default())
        .invoke_handler(tauri::generate_handler![
            state,
            performance,
            activity,
            agent_activity,
            activity_log,
            activity_log_clear,
            refresh,
            set_running,
            set_php,
            add_alias,
            link_folder,
            create_app,
            create_options,
            create_log,
            create_reveal,
            park_folder,
            remove_site,
            unhide_site,
            worktree_add,
            worktree_new_db,
            worktree_remove,
            worktree_losses,
            worktree_after_create,
            worktree_after_create_list,
            worktree_rewrite,
            worktree_hooks,
            set_app_hooks,
            worktree_prepare,
            worktree_adopt,
            worktree_copy_files,
            worktree_install,
            leftover_drop,
            leftover_keep,
            setup_status,
            setup_fix,
            set_group,
            set_site_ini,
            set_secure,
            set_root,
            set_https_default,
            allow_next_origin,
            set_runtime,
            open,
            icon,
            touched,
            settings,
            show_settings,
            show_processes,
            add_path,
            remove_path,
            set_editor,
            set_default_php,
            set_ini,
            php_install,
            php_opcache,
            php_ext,
            app_ext,
            xcode_tools_install,
            php_uninstall,
            set_terminal,
            set_git_client,
            git_info,
            set_theme,
            set_tld,
            processes,
            process_run,
            command_run,
            container_run,
            containers_run,
            once_stop,
            container_log_read,
            container_logs_stop,
            set_dock_icon,
            set_trim_app_logs,
            set_notifications,
            set_worktree_db,
            set_mcp,
            set_worktree_preset,
            set_login,
            open_config_dir,
            services,
            set_service_port,
            service_kinds,
            service_contents,
            service_data,
            open_in_client,
            service_run,
            set_service_tags,
            set_service_apps,
            open_service,
            add_service,
            remove_service,
            open_url,
            app_run,
            set_app,
            set_process,
            process_add,
            processes_reset,
            resources,
            packages,
            package_updates,
            package_audit,
            clean,
            log_sources,
            log_read,
            log_tail,
            log_text,
            log_search,
            log_clear,
            requests_read,
            requests_tail,
            open_in_editor,
            log_open,
            storage_anvil,
            storage_apps,
            storage_last,
            clean_anvil,
            clean_all_safe,
            runtimes,
            package_managers,
            package_manager_install,
            package_manager_uninstall,
            runtime_install,
            runtime_uninstall,
            update_state,
            update_check,
            update_install,
            relaunch
        ])
        .setup(|app| {
            let _ = HANDLE.set(app.handle().clone());
            // Windows keeps a window started by another program (the installer's Finish)
            // behind the one in front; on top for a moment brings it forward.
            #[cfg(windows)]
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_always_on_top(true);
                let _ = w.set_focus();
                let _ = w.set_always_on_top(false);
            }
            // SIGTERM (a logout, `kill`), SIGINT and SIGHUP quit as Quit does, stopping
            // everything: by default they end Anvil at once and its dev processes run on
            // with no Anvil that knows them.
            let h = app.handle().clone();
            let _ = ctrlc::set_handler(move || h.exit(0));
            // Clicking a notification brings Anvil forward.
            use tauri_plugin_notification::NotificationExt;
            let h = app.handle().clone();
            let _ = app.notification().on_action(move |a| {
                if a.action_id() == "tap" {
                    show(&h);
                }
            });
            if let Ok(cfg) = anvil::load() {
                apply_theme(app.handle(), cfg.theme.as_deref());
                // Starting everything (and the first web server download) happens in the
                // background, so the window and menu bar icon appear right away.
                let handle = app.handle().clone();
                tauri::async_runtime::spawn_blocking(move || {
                    let state = handle.state::<App>();
                    // What a crashed Anvil left running stops first; then a fresh Anvil
                    // shows only this run's output, as a new terminal would.
                    anvil::services::migrate_files(&cfg);
                    anvil::apps::kill_all_orphans();
                    anvil::apps::clear_stale_logs();
                    // Shims rewritten, so they follow the app when it's moved or updated.
                    if anvil::shims::ready() {
                        let _ = anvil::shims::install(&anvil::home());
                    }
                    // Log retention now and hourly, off the start's way.
                    std::thread::spawn(|| {
                        loop {
                            trim_logs();
                            std::thread::sleep(std::time::Duration::from_secs(3600));
                        }
                    });
                    start(&state, &cfg);
                    // The CLI and agents reach Anvil here; without it the app still works.
                    match anvil::control::bind() {
                        Ok(l) => {
                            SERVING.store(true, Ordering::SeqCst);
                            let h = handle.clone();
                            std::thread::spawn(move || {
                                anvil::control::serve(
                                    l,
                                    std::sync::Arc::new(move |r| control::dispatch(&h, r)),
                                )
                            });
                        }
                        Err(e) => eprintln!("Anvil: no control socket: {e:#}"),
                    }
                    // A crashed Caddy or PHP-FPM shows in the menu bar and window within 2 s.
                    // So does a crashed dev process, as a notification while Anvil isn't in front.
                    let watch = handle.clone();
                    std::thread::spawn(move || {
                        let mut was = true;
                        let mut seen = Default::default();
                        loop {
                            std::thread::sleep(std::time::Duration::from_secs(2));
                            let state = watch.state::<App>();
                            let now = running(&state);
                            if was && !now {
                                changed(&watch);
                            }
                            was = now;
                            if state.server_crashed.swap(false, Ordering::SeqCst) {
                                let why = anvil::lock(&state.error).clone().unwrap_or_default();
                                let label = why.split(" stopped").next().unwrap_or("Web server");
                                notify(&watch, &format!("{label} stopped"), &why);
                            }
                            let crashed = anvil::lock(&state.apps).crashed();
                            notify_crashes(&watch, &new_crashes(&mut seen, crashed));
                            // A worktree deleted in a terminal: its processes stop even with the window hidden.
                            // One stat per running child; the full rescan only when one is gone.
                            let running = anvil::lock(&state.apps).running_dirs();
                            if !anvil::apps::gone_dirs(&running).is_empty()
                                && let Ok(cfg) = anvil::load()
                            {
                                stop_vanished(&state, &cfg);
                            }
                        }
                    });
                    for id in anvil::services::enabled(&cfg) {
                        start_service(&state, &cfg, &id);
                    }
                    for site in anvil::sites(&cfg).iter().filter(|s| s.autostart) {
                        start_app(&state, &cfg, &site.name);
                    }
                    autostart_processes(&state, &cfg);
                    restore_processes(&state, &cfg, &take_keys(&restore_file()));
                    stop_vanished(&state, &cfg);
                    changed(&handle);
                });
            }
            // Look for a release 10 s in, then daily; installing waits for the user.
            // ponytail: thread::sleep pauses while the Mac sleeps, so "daily" is daily awake time.
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(10));
                loop {
                    let _ = tauri::async_runtime::block_on(update_check(handle.clone()));
                    if rebuilt() {
                        let _ = handle.emit("update", ());
                    }
                    std::thread::sleep(std::time::Duration::from_secs(24 * 3600));
                }
            });
            // Template image: macOS recolours it for light and dark menu bars.
            TrayIconBuilder::with_id("anvil")
                .icon(tauri::image::Image::from_bytes(include_bytes!(
                    "../icons/tray.png"
                ))?)
                .icon_as_template(true)
                .tooltip("Anvil")
                .menu(&menu(app.handle())?)
                .on_menu_event(|handle, e| on_menu(handle, e.id.as_ref()))
                .build(app)?;
            Ok(())
        })
        // macOS's menu bar; on Windows it would be a menu row inside the window.
        .menu(move |h| {
            if cfg!(target_os = "macos") {
                app_menu(h)
            } else {
                Menu::new(h)
            }
        })
        .on_menu_event(|handle, e| on_app_menu(handle, e.id.as_ref()))
        .on_window_event(|w, e| {
            // Closing the main window keeps Anvil running in the menu bar.
            match e {
                WindowEvent::CloseRequested { api, .. } if w.label() == "main" => {
                    api.prevent_close();
                    let _ = w.hide();
                    update_dock(w.app_handle());
                }
                WindowEvent::Destroyed => update_dock(w.app_handle()),
                // A local build replaced the binary: offer Relaunch when the user comes back.
                WindowEvent::Focused(true) if rebuilt() => {
                    let _ = w.app_handle().emit("update", ());
                }
                _ => {}
            }
        })
        .build(tauri::generate_context!())
        .expect("failed to build Anvil")
        .run(|handle, e| match e {
            // macOS asking Anvil to quit (Dock, a logout aside) without Anvil choosing to:
            // keep running in the menu bar so nothing the user started stops.
            RunEvent::ExitRequested {
                code: None, api, ..
            } => {
                api.prevent_exit();
                hide_windows(handle);
            }
            // Quit (tray, menu, Cmd-Q): stop everything in a thread so the UI doesn't
            // freeze, then exit for real. Relaunch stops first and sets STOPPED itself.
            RunEvent::ExitRequested {
                code: Some(code),
                api,
                ..
            } if code != tauri::RESTART_EXIT_CODE && !STOPPED.load(Ordering::SeqCst) => {
                api.prevent_exit();
                hide_windows(handle);
                if !QUITTING.swap(true, Ordering::SeqCst) {
                    let handle = handle.clone();
                    std::thread::spawn(move || {
                        // Exit even if a stop panicked: Quit must never hang.
                        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            stop_everything(&handle.state::<App>())
                        }));
                        STOPPED.store(true, Ordering::SeqCst);
                        handle.exit(code);
                    });
                }
            }
            // Normally nothing is left by now; this catches whatever started since.
            RunEvent::Exit => stop_everything(&handle.state::<App>()),
            #[cfg(target_os = "macos")]
            RunEvent::Reopen { .. } => show(handle),
            _ => {}
        });
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn a_crash_notifies_once() {
        let exit = |at| anvil::apps::Exit { code: Some(1), at };
        let mut seen = Default::default();
        let new = new_crashes(&mut seen, vec![("shop@queue".into(), exit(10))]);
        assert_eq!(new.len(), 1);
        // Still crashed on the next look: nothing new.
        assert!(new_crashes(&mut seen, vec![("shop@queue".into(), exit(10))]).is_empty());
        // Stopped by the user, so gone from the list: nothing.
        assert!(new_crashes(&mut seen, vec![]).is_empty());
        // Started again and crashed again: a new crash.
        assert_eq!(
            new_crashes(&mut seen, vec![("shop@queue".into(), exit(20))]).len(),
            1
        );
    }

    #[test]
    fn restart_keys_are_read_once() {
        let dir = std::env::temp_dir().join(format!("anvil-restart-{}", std::process::id()));
        let file = dir.join("run").join("restart-processes");
        let keys = vec!["shop".to_string(), "shop@queue".to_string()];
        save_keys(&file, &keys).unwrap();
        assert_eq!(take_keys(&file), keys);
        assert!(!file.exists());
        assert!(take_keys(&file).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_unknown_app_is_named_in_the_error() {
        let cfg = Config::default();
        assert_eq!(
            site_in(&cfg, "shop").err().as_deref(),
            Some("No app named shop")
        );
    }

    #[test]
    fn log_keys_are_parsed_and_paths_rejected() {
        let parse = |k| parse_log_key(k, &["redis".to_string()]);
        assert_eq!(parse("web:caddy"), Ok(LogKey::Caddy));
        assert_eq!(parse("web:php-8.4"), Ok(LogKey::Php("8.4")));
        assert_eq!(parse("web:php-8.4:4242"), Ok(LogKey::Php("8.4")));
        assert!(parse("web:php-8.4:../x").is_err());
        assert_eq!(parse("my.app"), Ok(LogKey::App("my.app")));
        assert_eq!(parse("a@dev"), Ok(LogKey::Process("a", "dev")));
        assert_eq!(parse("a#build"), Ok(LogKey::Once("a", "build")));
        assert_eq!(parse("service:redis"), Ok(LogKey::Service("redis")));
        assert_eq!(parse("container:a/db"), Ok(LogKey::Container("a", "db")));
        for bad in [
            "../etc/passwd",
            "/tmp/x",
            "container:a/../b",
            "web:../x",
            "web:php-..",
            "a@../x",
            "a#/x",
            "service:../x",
            "..",
            "",
            "container:a/b/c",
            "service:pgsql",
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }

    fn site(name: &str, kind: &str, dir: &std::path::Path) -> Site {
        Site {
            name: name.into(),
            host: format!("{name}.test"),
            secure: false,
            path: dir.into(),
            root: dir.into(),
            root_custom: None,
            root_detected: ".".into(),
            php: None,
            php_pinned: false,
            ext: vec![],
            own_master: false,
            octane_ports: None,
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
            worktree_of: None,
            branch: None,
            aliases: vec![],
        }
    }

    #[test]
    fn sidebar_dot_says_whether_anything_of_the_app_runs() {
        let dir = std::env::temp_dir().join(format!("anvil-states-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut vite = site("vuefinder", "vite", std::path::Path::new("/x/vuefinder"));
        vite.port = Some(41001);
        let sites = [
            site(
                "ozdemir.be",
                "laravel",
                std::path::Path::new("/x/ozdemir.be"),
            ),
            vite.clone(),
            site("elpida", "compose", &dir),
            site("shop", "laravel", std::path::Path::new("/x/shop")),
            site("notes", "folder", std::path::Path::new("/x/notes")),
            site("blog", "laravel", std::path::Path::new("/x/blog")),
        ];
        let keys = |k: &[&str]| k.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let up = |p: u16| p == 41001;
        let states = app_states(
            &sites,
            // A Laravel app's queue, vuefinder's own server, a one-off script.
            &keys(&["ozdemir.be@queue", "vuefinder", "notes#lint"]),
            &keys(&["shop@logs", "ozdemir.be@vite"]),
            std::slice::from_ref(&dir),
            up,
        );
        assert_eq!(states.get("ozdemir.be"), Some(&"running"));
        assert_eq!(states.get("vuefinder"), Some(&"running"));
        assert_eq!(states.get("elpida"), Some(&"running"));
        assert_eq!(states.get("shop"), Some(&"crashed"));
        assert_eq!(states.get("notes"), None);
        assert_eq!(states.get("blog"), None);
        // Its own server whose port doesn't answer yet: starting.
        let states = app_states(&[vite], &keys(&["vuefinder"]), &[], &[], |_| false);
        assert_eq!(states.get("vuefinder"), Some(&"starting"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn main_row_is_the_dev_server_or_the_served_site() {
        let cfg = Config::default();
        let dir = std::env::temp_dir();
        let mut v = site("vuefinder", "vite", &dir);
        v.command = anvil::apps::default_command("vite", &dir);
        v.port = Some(3000);
        v.autostart = true;
        let r = main_row(&cfg, &v, false, true).unwrap();
        assert_eq!((r.kind, r.process.name.as_str()), ("server", "dev"));
        assert_eq!(r.process.id, "");
        assert_eq!(r.process.command, v.command.clone().unwrap());
        assert_eq!(r.web, Some(anvil::site_url(&cfg, &v)));
        assert!(r.main && r.running && r.process.autostart && !r.process.edited);
        // A custom command that isn't an npm script is just the server.
        v.command = Some("cargo run".into());
        v.command_custom = true;
        let r = main_row(&cfg, &v, false, false).unwrap();
        assert_eq!(r.process.name, "server");
        assert!(r.process.edited && !r.running);

        // A PHP app's row is the site Anvil serves: running while the web server is.
        let mut l = site("shop", "laravel", &dir);
        l.autostart = true;
        let r = main_row(&cfg, &l, true, false).unwrap();
        assert_eq!((r.kind, r.process.name.as_str()), ("site", "shop.test"));
        assert_eq!(r.web, Some(anvil::site_url(&cfg, &l)));
        assert!(r.running && r.process.command.is_empty() && !r.process.autostart);
        assert!(!main_row(&cfg, &l, false, true).unwrap().running);
        assert_eq!(
            main_row(&cfg, &site("s", "static", &dir), true, false)
                .unwrap()
                .kind,
            "site"
        );
        assert!(main_row(&cfg, &site("c", "compose", &dir), true, true).is_none());
    }

    #[test]
    fn only_extra_servers_of_non_php_apps_open_in_the_browser() {
        let cfg = Config::default();
        let dir = std::env::temp_dir().join(format!("anvil-web-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("package.json"),
            r#"{"scripts":{"dev":"vite","docs:dev":"vitepress dev docs"}}"#,
        )
        .unwrap();
        let v = site("vuefinder", "vite", &dir);
        let docs = anvil::processes::detect_with(&v, Some(&cfg))
            .into_iter()
            .find(|p| p.name == "docs:dev")
            .unwrap();
        let web = row_web(&cfg, &v, &docs).unwrap();
        assert!(web.contains("vuefinder--docs.test"), "{web}");
        // The same row in a PHP app is Vite serving assets: no page of its own.
        let l = site("shop", "laravel", &dir);
        assert_eq!(row_web(&cfg, &l, &docs), None);
        let worker = anvil::processes::Process {
            id: "queue".into(),
            ..Default::default()
        };
        assert_eq!(row_web(&cfg, &v, &worker), None);
    }
}
