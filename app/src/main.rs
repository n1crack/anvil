// Hide the extra console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anvil::{Config, Server, Site};
use serde::Serialize;
use std::process::Command;
use std::sync::Mutex;
use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::TrayIconBuilder;
use tauri::{
    AppHandle, Emitter, Manager, RunEvent, State, TitleBarStyle, WebviewUrl, WebviewWindowBuilder,
    WindowEvent, Wry,
};

#[derive(Default)]
struct App {
    server: Mutex<Option<Server>>,
    error: Mutex<Option<String>>,
    services: Mutex<anvil::services::Services>,
    apps: Mutex<anvil::apps::Apps>,
    /// Last start failure per app.
    app_errors: Mutex<std::collections::BTreeMap<String, String>>,
    /// Last start failure per service id.
    service_errors: Mutex<std::collections::BTreeMap<String, String>>,
}

#[derive(Serialize)]
struct Snapshot {
    sites: Vec<Site>,
    tld: String,
    https_port: u16,
    /// Port for addresses of sites without HTTPS.
    http_port: u16,
    php_default: Option<String>,
    php_installed: Vec<String>,
    editor: Option<String>,
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
}

type Res<T> = Result<T, String>;

fn err(e: anyhow::Error) -> String {
    format!("{e:#}")
}

fn running(app: &App) -> bool {
    app.server.lock().unwrap().is_some()
}

fn snapshot(app: &App) -> Res<Snapshot> {
    let cfg = anvil::load().map_err(err)?;
    let https_port = anvil::public_https_port(&cfg);
    let http_port = anvil::public_http_port(&cfg);
    let sites = anvil::sites(&cfg);
    let mut apps = app.apps.lock().unwrap();
    let app_states = sites
        .iter()
        .filter(|s| apps.running(&s.name))
        .map(|s| {
            let up = s
                .port
                .is_some_and(|p| std::net::TcpStream::connect(("127.0.0.1", p)).is_ok());
            (s.name.clone(), if up { "running" } else { "starting" })
        })
        .collect();
    drop(apps);
    Ok(Snapshot {
        app_states,
        app_errors: app.app_errors.lock().unwrap().clone(),
        kinds: anvil::apps::KINDS
            .iter()
            .map(|k| (*k, anvil::apps::label(k)))
            .collect(),
        paths: cfg.paths.clone(),
        runtimes: anvil::runtimes::RUNTIMES
            .iter()
            .map(|id| (*id, anvil::runtimes::installed(id)))
            .collect(),
        sites,
        php_default: cfg.php.clone(),
        php_installed: anvil::installed_php(),
        editor: cfg.editor.clone(),
        git_client: git_client(&cfg).map(|c| app_label(&c)),
        groups: cfg.groups.clone(),
        ini: cfg.ini.clone(),
        running: running(app),
        error: app.error.lock().unwrap().clone(),
        tld: cfg.tld,
        https_port,
        http_port,
    })
}

fn start(app: &App, cfg: &Config) {
    let mut server = app.server.lock().unwrap();
    *server = None; // stop the old one first so ports are free
    match Server::start(cfg) {
        Ok(s) => {
            *server = Some(s);
            *app.error.lock().unwrap() = None;
        }
        Err(e) => *app.error.lock().unwrap() = Some(err(e)),
    }
}

fn stop(app: &App) {
    *app.server.lock().unwrap() = None;
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
    let cfg = anvil::load().map_err(err)?;
    if running(&app) && anvil::needs_restart(&cfg) {
        start(&app, &cfg);
        changed(&handle);
    }
    snapshot(&app)
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
    let mut cfg = anvil::load().map_err(err)?;
    anvil::set_site_php(&mut cfg, &site, version).map_err(err)?;
    if running(&app) {
        start(&app, &cfg);
    }
    changed(&handle);
    snapshot(&app)
}

/// Sites changed on disk config: re-serve if running, then refresh everything.
fn sites_changed(handle: &AppHandle, app: &App, cfg: &Config) {
    if running(app) {
        start(app, cfg);
    }
    changed(handle);
}

fn start_app(app: &App, cfg: &Config, name: &str) {
    let result = anvil::sites(cfg)
        .into_iter()
        .find(|s| s.name == name)
        .ok_or_else(|| anyhow::anyhow!("No app named {name}"))
        .and_then(|site| app.apps.lock().unwrap().start(&site, &cfg.tld));
    let mut errors = app.app_errors.lock().unwrap();
    match result {
        Ok(()) => errors.remove(name),
        Err(e) => errors.insert(name.into(), err(e)),
    };
}

/// One of an app's other processes, as the window shows it.
#[derive(Serialize)]
struct ProcessRow {
    #[serde(flatten)]
    process: anvil::processes::Process,
    running: bool,
    /// The newest line it wrote.
    last: Option<String>,
    error: Option<String>,
    /// Where an extra dev server answers, e.g. `https://docs.vuefinder.test`.
    address: Option<String>,
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
    docker_error: Option<String>,
}

fn process_rows(app: &App, cfg: &Config, site: &Site) -> Vec<ProcessRow> {
    // Detecting may run PHP: do it before taking the locks.
    let detected = anvil::processes::detect_with(site, Some(cfg));
    let errors = app.app_errors.lock().unwrap();
    let mut apps = app.apps.lock().unwrap();
    let crashed = apps.crashed();
    detected
        .into_iter()
        .map(|p| {
            let key = anvil::processes::key(&site.name, &p.id);
            let last = anvil::logs::read(
                &anvil::processes::log_file(&site.name, &p.id),
                None,
                8 * 1024,
            )
            .ok()
            .and_then(|page| page.entries.into_iter().next())
            .map(|e| e.message.lines().next().unwrap_or("").to_string());
            ProcessRow {
                running: apps.running(&key),
                pid: apps.pid(&key),
                crashed: crashed
                    .iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, e)| e.clone()),
                error: errors.get(&key).cloned(),
                last,
                // Only extra servers of non-PHP apps have an address of their own.
                address: p
                    .port
                    .and_then(|_| anvil::processes::address(cfg, site, &p.id)),
                process: p,
            }
        })
        .collect()
}

fn overview(app: &App, cfg: &Config, site: &Site) -> Overview {
    // Docker is only asked about apps that have a compose file.
    let (containers, docker_error) = match anvil::compose::file(&site.path) {
        None => (None, None),
        Some(_) => match anvil::compose::containers(&site.path) {
            Ok(c) => (Some(c), None),
            Err(e) => (None, Some(err(e))),
        },
    };
    let scripts = anvil::processes::package_scripts(&site.path).1;
    let commands = {
        let mut apps = app.apps.lock().unwrap();
        scripts
            .into_iter()
            .map(|name| {
                let key = anvil::processes::once_key(&site.name, &name);
                let running = apps.running(&key);
                CommandChip {
                    id: anvil::processes::process_id(&name),
                    exit: apps.finished(&key).filter(|_| !running),
                    running,
                    name,
                }
            })
            .collect()
    };
    Overview {
        rows: process_rows(app, cfg, site),
        commands,
        containers,
        docker_error,
    }
}

fn find_site(name: &str) -> Res<Site> {
    find_site_cfg(name).map(|(_, s)| s)
}

/// The app and the config it was read from, loaded once.
fn find_site_cfg(name: &str) -> Res<(Config, Site)> {
    let cfg = anvil::load().map_err(err)?;
    let site = anvil::sites(&cfg)
        .into_iter()
        .find(|s| s.name == name)
        .ok_or_else(|| format!("No app named {name}"))?;
    Ok((cfg, site))
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

fn listening(port: u16) -> bool {
    std::net::TcpStream::connect(("127.0.0.1", port)).is_ok()
}

/// Everything Anvil runs: the web server, apps with their processes and containers, services.
#[tauri::command(async)]
fn activity(app: State<App>) -> Res<Activity> {
    let cfg = anvil::load().map_err(err)?;
    let sites = anvil::sites(&cfg);
    // Quick snapshots, each lock released before anything slow.
    let web_pids = app
        .server
        .lock()
        .unwrap()
        .as_ref()
        .map(|s| s.pids())
        .unwrap_or_default();
    let (listed, crashed, scripts) = {
        let mut apps = app.apps.lock().unwrap();
        let listed = apps.list();
        // Commands run once show their script (`build:all`), not the id in their key.
        let scripts: std::collections::HashMap<String, String> = listed
            .iter()
            .filter_map(|(k, _, _)| Some((k.clone(), apps.once_name(k)?.to_string())))
            .collect();
        (listed, apps.crashed(), scripts)
    };
    let (infos, service_pids) = {
        let mut services = app.services.lock().unwrap();
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
            let key = match label
                .strip_prefix("PHP ")
                .and_then(|l| l.strip_suffix(" FPM"))
            {
                Some(v) => format!("web:php-{v}"),
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
                    r
                } else {
                    let id = &rest[1..];
                    let shown = scripts.get(key).cloned().unwrap_or_else(|| name(id));
                    let mut r = row(key.clone(), shown, "running", true);
                    r.port = found(id).and_then(|p| p.port);
                    r
                };
                // A server is starting until its port answers.
                if r.port.is_some_and(|p| !listening(p)) {
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
        let real = site.path.canonicalize().ok();
        let here = |dir: &std::path::Path| {
            dir == site.path || (real.is_some() && dir.canonicalize().ok() == real)
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

fn plain(s: &str, dots: bool) -> bool {
    !s.is_empty()
        && !s.starts_with("..")
        && s != "."
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || (dots && c == '.'))
}

/// `services` are the configured service ids; any other `service:` key is refused.
fn parse_log_key<'a>(key: &'a str, services: &[String]) -> Res<LogKey<'a>> {
    let parsed = if key == "web:caddy" {
        Some(LogKey::Caddy)
    } else if let Some(v) = key.strip_prefix("web:php-") {
        (!v.is_empty() && v.chars().all(|c| c.is_ascii_digit() || c == '.') && !v.contains(".."))
            .then_some(LogKey::Php(v))
    } else if let Some(id) = key.strip_prefix("service:") {
        (plain(id, false) && services.iter().any(|s| s == id)).then_some(LogKey::Service(id))
    } else if let Some(rest) = key.strip_prefix("container:") {
        rest.split_once('/')
            .filter(|(s, c)| plain(s, true) && plain(c, true))
            .map(|(s, c)| LogKey::Container(s, c))
    } else if let Some((s, id)) = key.split_once('@') {
        (plain(s, true) && plain(id, false)).then_some(LogKey::Process(s, id))
    } else if let Some((s, id)) = key.split_once('#') {
        (plain(s, true) && plain(id, false)).then_some(LogKey::Once(s, id))
    } else {
        plain(key, true).then_some(LogKey::App(key))
    };
    parsed.ok_or_else(|| format!("Not a log: {key}"))
}

/// New log content for a row of the Processes window, by key (see `parse_log_key`).
#[tauri::command(async)]
fn activity_log(key: String, from: u64) -> Res<anvil::logs::Page> {
    let log = anvil::home().join("log");
    // The service list is read only for a service key; Caddy and PHP logs need no config.
    let services = if key.starts_with("service:") {
        anvil::services::instances(&anvil::load().map_err(err)?)
    } else {
        vec![]
    };
    let file = match parse_log_key(&key, &services)? {
        LogKey::Caddy => log.join("caddy.log"),
        LogKey::Php(v) => log.join(format!("php-{v}-fpm.log")),
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
        LogKey::Container(site, service) => {
            let text = anvil::compose::logs(&find_site(site)?.path, service).map_err(err)?;
            // ponytail: no incremental tail for containers; the whole text again unless its size is unchanged.
            let end = text.len() as u64;
            let mut entries: Vec<_> = anvil::logs::split(&text)
                .into_iter()
                .map(anvil::logs::entry)
                .collect();
            entries.reverse();
            if from == end {
                entries.clear();
            }
            return Ok(anvil::logs::Page {
                entries,
                start_offset: 0,
                end_offset: end,
                truncated: false,
            });
        }
    };
    if !file.exists() {
        return Ok(anvil::logs::Page::default());
    }
    anvil::logs::tail(&file, from).map_err(err)
}

#[tauri::command(async)]
fn processes(app: State<App>, site: String) -> Res<Overview> {
    let (cfg, s) = find_site_cfg(&site)?;
    if let Some(e) = anvil::processes::project_file_error(&s) {
        return Err(e);
    }
    Ok(overview(&app, &cfg, &s))
}

/// Start, stop or restart one process, or all of them and the app's own server
/// when `name` is None.
#[tauri::command(async)]
fn process_run(
    handle: AppHandle,
    app: State<App>,
    site: String,
    name: Option<String>,
    action: String,
) -> Res<Overview> {
    if name.as_deref().is_some_and(|n| !plain(n, false)) {
        return Err(format!("Not a process: {name:?}"));
    }
    let (cfg, s) = find_site_cfg(&site)?;
    if name.is_none() && anvil::apps::runs_process(&s.kind) {
        run_app(&app, &cfg, &s.name, &action);
        changed(&handle);
    }
    if action != "start" {
        // By key, not detection: a row whose script was renamed or removed still stops.
        let keys = match &name {
            Some(n) => vec![anvil::processes::key(&s.name, n)],
            None => app.apps.lock().unwrap().process_keys(&s.name),
        };
        for key in keys {
            app.apps.lock().unwrap().stop(&key);
            app.app_errors.lock().unwrap().remove(&key);
        }
    }
    if action != "stop" {
        for p in anvil::processes::detect_with(&s, Some(&cfg))
            .into_iter()
            .filter(|p| name.as_deref().is_none_or(|n| n == p.id))
        {
            let key = anvil::processes::key(&s.name, &p.id);
            let started = app.apps.lock().unwrap().start_process(&s, &p, &cfg.tld);
            if let Err(e) = started {
                app.app_errors.lock().unwrap().insert(key, err(e));
            }
        }
    }
    Ok(overview(&app, &cfg, &s))
}

/// Run one of the app's package.json commands once (`npm run <script>`); its
/// output is the log source `once:<id>`.
#[tauri::command(async)]
fn command_run(app: State<App>, site: String, script: String) -> Res<Overview> {
    let (cfg, s) = find_site_cfg(&site)?;
    if !anvil::processes::package_scripts(&s.path)
        .1
        .contains(&script)
    {
        return Err(format!("{site} has no command named {script}"));
    }
    app.apps
        .lock()
        .unwrap()
        .run_once(&s, &script)
        .map_err(err)?;
    Ok(overview(&app, &cfg, &s))
}

/// Stop a package.json command run once (`command_run`), key `<site>#<id>`.
#[tauri::command(async)]
fn once_stop(handle: AppHandle, app: State<App>, site: String, id: String) -> Res<()> {
    find_site(&site)?;
    app.apps
        .lock()
        .unwrap()
        .stop(&anvil::processes::once_key(&site, &id));
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
    Ok(overview(&app, &cfg, &s))
}

#[tauri::command(async)]
fn container_log(site: String, service: String) -> Res<String> {
    let s = find_site(&site)?;
    anvil::compose::logs(&s.path, &service).map_err(err)
}

fn run_app(app: &App, cfg: &Config, site: &str, action: &str) {
    if action != "start" {
        app.apps.lock().unwrap().stop(site);
        app.app_errors.lock().unwrap().remove(site);
    }
    if action != "stop" {
        start_app(app, cfg, site);
    }
}

#[tauri::command(async)]
fn app_run(handle: AppHandle, app: State<App>, site: String, action: String) -> Res<Snapshot> {
    run_app(&app, &anvil::load().map_err(err)?, &site, &action);
    changed(&handle);
    snapshot(&app)
}

/// Change an app's kind, start command, port or autostart. Fields left out stay as they are;
/// `reset` lists fields to put back to their defaults.
#[tauri::command]
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
    let mut cfg = anvil::load().map_err(err)?;
    let reroute =
        kind.is_some() || port.is_some() || reset.iter().any(|r| r == "kind" || r == "port");
    anvil::set_site_app(
        &mut cfg,
        &site,
        pick("kind", kind),
        pick("command", command),
        port,
        autostart,
    )
    .map_err(err)?;
    // A running app picks up a new command or port by restarting.
    let was_running = app.apps.lock().unwrap().running(&site);
    if was_running {
        app.apps.lock().unwrap().stop(&site);
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

/// Disk, processes and services for one app. Sizing big folders takes a moment,
/// so it runs off the main thread and only holds locks for the quick parts.
#[tauri::command]
async fn resources(handle: AppHandle, site: String) -> Res<anvil::resources::Resources> {
    tauri::async_runtime::spawn_blocking(move || {
        let cfg = anvil::load().map_err(err)?;
        let s = anvil::sites(&cfg)
            .into_iter()
            .find(|s| s.name == site)
            .ok_or_else(|| format!("No app named {site}"))?;
        let (total, items) = anvil::resources::disk(&s);
        let app = handle.state::<App>();
        let pid = app.apps.lock().unwrap().pid(&s.name);
        let processes = anvil::resources::processes(&s, pid);
        let live = app.services.lock().unwrap().pids();
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
        let cfg = anvil::load().map_err(err)?;
        let s = anvil::sites(&cfg)
            .into_iter()
            .find(|s| s.name == site)
            .ok_or_else(|| format!("No app named {site}"))?;
        Ok(anvil::git::info(&s.path))
    })
    .await
    .map_err(|e| e.to_string())?
}

fn site_dir(site: &str) -> Res<std::path::PathBuf> {
    let cfg = anvil::load().map_err(err)?;
    anvil::sites(&cfg)
        .into_iter()
        .find(|s| s.name == site)
        .map(|s| s.path)
        .ok_or_else(|| format!("No app named {site}"))
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

/// Delete one cleanable item of an app (see `resources::clean` for the guards).
#[tauri::command]
async fn clean(handle: AppHandle, site: String, path: String) -> Res<u64> {
    tauri::async_runtime::spawn_blocking(move || {
        let cfg = anvil::load().map_err(err)?;
        let s = anvil::sites(&cfg)
            .into_iter()
            .find(|s| s.name == site)
            .ok_or_else(|| format!("No app named {site}"))?;
        let running = handle.state::<App>().apps.lock().unwrap().running(&s.name);
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
        let cfg = anvil::load().map_err(err)?;
        let s = anvil::sites(&cfg)
            .into_iter()
            .find(|s| s.name == site)
            .ok_or_else(|| format!("No app named {site}"))?;
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

#[tauri::command]
async fn log_clear(site: String, source: String) -> Res<()> {
    with_site(site, move |s| anvil::logs::clear(&s, &source)).await
}

#[tauri::command]
async fn log_open(site: String, source: String) -> Res<()> {
    with_site(site, move |s| {
        anvil::run(
            Command::new("open")
                .args(["-a", "Console"])
                .arg(log_path(&s, &source)?),
        )
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
        anvil::runtimes::RUNTIMES
            .iter()
            .map(|id| anvil::runtimes::info(id, true))
            .collect()
    })
    .await
    .map_err(|e| e.to_string())
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
    anvil::runtimes::uninstall(&id, &version).map_err(err)?;
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
) -> Res<String> {
    let job = (kind.clone(), name.clone(), parent.clone());
    let dir = tauri::async_runtime::spawn_blocking(move || {
        let cfg = anvil::load()?;
        anvil::create::create(&job.0, &job.1, &job.2, cfg.php.as_deref())
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(err)?;

    let app = handle.state::<App>();
    let mut cfg = anvil::load().map_err(err)?;
    let parked = cfg
        .paths
        .iter()
        .any(|p| p.canonicalize().ok() == parent.canonicalize().ok());
    let site = if parked {
        anvil::site_name(&name)
    } else {
        anvil::link(&mut cfg, &name, dir).map_err(err)?
    };
    sites_changed(&handle, &app, &cfg);
    if anvil::apps::runs_process(&kind) {
        start_app(&app, &cfg, &site);
    }
    changed(&handle);
    Ok(site)
}

#[tauri::command(async)]
fn open_create_log(name: String) -> Res<()> {
    anvil::run(
        Command::new("open")
            .args(["-a", "Console"])
            .arg(anvil::create::log_file(&name)),
    )
    .map_err(err)
}

/// Serve one project folder; its name comes from the folder.
#[tauri::command(async)]
fn link_folder(handle: AppHandle, app: State<App>, path: std::path::PathBuf) -> Res<String> {
    let path = path.canonicalize().map_err(|e| e.to_string())?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or("Pick a folder")?;
    let mut cfg = anvil::load().map_err(err)?;
    let name = anvil::link(&mut cfg, &name, path).map_err(err)?;
    sites_changed(&handle, &app, &cfg);
    Ok(name)
}

/// Serve every project inside a folder.
#[tauri::command(async)]
fn park_folder(handle: AppHandle, app: State<App>, path: std::path::PathBuf) -> Res<()> {
    let path = path.canonicalize().map_err(|e| e.to_string())?;
    let mut cfg = anvil::load().map_err(err)?;
    if !cfg.paths.contains(&path) {
        cfg.paths.push(path);
    }
    anvil::save(&cfg).map_err(err)?;
    sites_changed(&handle, &app, &cfg);
    Ok(())
}

#[tauri::command(async)]
fn add_alias(handle: AppHandle, app: State<App>, site: String, alias: String) -> Res<Snapshot> {
    let mut cfg = anvil::load().map_err(err)?;
    let path = anvil::sites(&cfg)
        .into_iter()
        .find(|s| s.name == site)
        .ok_or_else(|| format!("No site named {site}"))?
        .path;
    anvil::link(&mut cfg, &alias, path).map_err(err)?;
    sites_changed(&handle, &app, &cfg);
    snapshot(&app)
}

/// Unlinks linked names and hides parked folders. Never deletes files.
#[tauri::command(async)]
fn remove_site(handle: AppHandle, app: State<App>, site: String) -> Res<Snapshot> {
    // Its processes and commands stop with it: afterwards nothing could reach them.
    let keys = app.apps.lock().unwrap().process_keys(&site);
    for key in keys {
        app.apps.lock().unwrap().stop(&key);
    }
    let mut cfg = anvil::load().map_err(err)?;
    anvil::remove(&mut cfg, &site).map_err(err)?;
    sites_changed(&handle, &app, &cfg);
    snapshot(&app)
}

/// What still needs doing before `.test` addresses work: resolver, certificate, PHP.
#[tauri::command(async)]
fn setup_status() -> Res<anvil::Setup> {
    Ok(anvil::setup_status(&anvil::load().map_err(err)?))
}

/// Do one setup step. The resolver file needs an administrator: macOS asks for
/// the password in its own dialog.
#[tauri::command(async)]
fn setup_fix(handle: AppHandle, step: String) -> Res<anvil::Setup> {
    let mut cfg = anvil::load().map_err(err)?;
    match step.as_str() {
        "resolver" => {
            let ok =
                |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
            if !ok(&cfg.tld) {
                return Err(format!("{} isn't a valid domain suffix", cfg.tld));
            }
            let (file, _) = anvil::resolver_file(&cfg);
            // Backslashes survive AppleScript once, so printf sees \n.
            let script = format!(
                "do shell script \"mkdir -p /etc/resolver && printf 'nameserver 127.0.0.1\\\\nport {}\\\\n' > {}\" with prompt \"Anvil sets up .{} addresses on this Mac.\" with administrator privileges",
                cfg.dns_port,
                file.display(),
                cfg.tld
            );
            anvil::run(Command::new("osascript").args(["-e", &script])).map_err(err)?;
        }
        "ports" => {
            let (file, _) = anvil::ports_daemon(&cfg);
            let tmp = anvil::stage_ports_daemon(&cfg).map_err(err)?;
            let (tmp, file, label) = (tmp.display(), file.display(), anvil::PORTS_LABEL);
            // bootout first: a changed port reloads the daemon.
            let script = format!(
                "do shell script \"launchctl bootout system/{label} 2>/dev/null; cp '{tmp}' '{file}' && chown root:wheel '{file}' && chmod 644 '{file}' && launchctl bootstrap system '{file}'\" with prompt \"Anvil lets addresses work without a port number.\" with administrator privileges"
            );
            let res = anvil::run(Command::new("osascript").args(["-e", &script]));
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
    let mut cfg = anvil::load().map_err(err)?;
    anvil::unhide(&mut cfg, &site).map_err(err)?;
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
    let mut cfg = anvil::load().map_err(err)?;
    anvil::set_site_runtime(&mut cfg, &site, version).map_err(err)?;
    let was_running = app.apps.lock().unwrap().running(&site);
    if was_running {
        app.apps.lock().unwrap().stop(&site);
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
    let mut cfg = anvil::load().map_err(err)?;
    anvil::set_site_ini(&mut cfg, &site, &key, value.as_deref()).map_err(err)?;
    sites_changed(&handle, &app, &cfg);
    snapshot(&app)
}

/// Let a Next.js app answer at `.<tld>` by adding it to `allowedDevOrigins`;
/// Next restarts itself when its config changes.
#[tauri::command(async)]
fn allow_next_origin(handle: AppHandle, app: State<App>, site: String) -> Res<Snapshot> {
    let cfg = anvil::load().map_err(err)?;
    let s = anvil::sites(&cfg)
        .into_iter()
        .find(|s| s.name == site)
        .ok_or_else(|| format!("No site named {site}"))?;
    anvil::apps::allow_next_origin(&s.path, &cfg.tld).map_err(err)?;
    changed(&handle);
    snapshot(&app)
}

/// HTTPS on or off for one site.
#[tauri::command(async)]
fn set_secure(handle: AppHandle, app: State<App>, site: String, on: bool) -> Res<Snapshot> {
    let mut cfg = anvil::load().map_err(err)?;
    anvil::set_secure(&mut cfg, &site, on).map_err(err)?;
    sites_changed(&handle, &app, &cfg);
    snapshot(&app)
}

/// Whether sites without their own choice get HTTPS.
#[tauri::command(async)]
fn set_https_default(handle: AppHandle, on: bool) -> Res<Settings> {
    apply(
        &handle,
        |cfg| {
            cfg.https_default = Some(on);
            Ok(())
        },
        true,
    )
}

#[tauri::command(async)]
fn set_group(app: State<App>, site: String, group: Option<String>) -> Res<Snapshot> {
    let mut cfg = anvil::load().map_err(err)?;
    anvil::set_group(&mut cfg, &site, group.as_deref()).map_err(err)?;
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
    let errors = app.service_errors.lock().unwrap().clone();
    Ok(app
        .services
        .lock()
        .unwrap()
        .info(&cfg)
        .into_iter()
        .map(|info| ServiceRow {
            error: errors.get(&info.id).cloned(),
            clients: anvil::databases::clients(info.kind),
            info,
        })
        .collect())
}

fn start_service(app: &App, cfg: &Config, id: &str) {
    let result = app.services.lock().unwrap().start(cfg, id);
    let mut errors = app.service_errors.lock().unwrap();
    match result {
        Ok(()) => errors.remove(id),
        Err(e) => errors.insert(id.into(), err(e)),
    };
}

/// Turning a service on also makes it start with Anvil, and vice versa.
#[tauri::command(async)]
fn set_service(handle: AppHandle, app: State<App>, id: String, on: bool) -> Res<Vec<ServiceRow>> {
    let mut cfg = anvil::load().map_err(err)?;
    anvil::services::set_enabled(&mut cfg, &id, on).map_err(err)?;
    if on {
        start_service(&app, &cfg, &id);
    } else {
        app.services.lock().unwrap().stop(&id);
        app.service_errors.lock().unwrap().remove(&id);
    }
    changed(&handle);
    services(app)
}

#[tauri::command(async)]
fn set_service_port(
    handle: AppHandle,
    app: State<App>,
    id: String,
    port: Option<u16>,
    web: Option<bool>,
) -> Res<Vec<ServiceRow>> {
    let mut cfg = anvil::load().map_err(err)?;
    if web == Some(true) {
        anvil::services::set_web_port(&mut cfg, &id, port).map_err(err)?;
    } else {
        anvil::services::set_port(&mut cfg, &id, port).map_err(err)?;
    }
    let mut svc = app.services.lock().unwrap();
    let was_running = svc.running(&id);
    svc.stop(&id);
    drop(svc);
    if was_running || cfg.services.get(&id).is_some_and(|s| s.enabled) {
        start_service(&app, &cfg, &id);
    }
    changed(&handle);
    services(app)
}

/// Downloads (and for Redis, compiles) off the main thread, then starts it.
#[tauri::command]
async fn install_service(handle: AppHandle, id: String) -> Res<Vec<ServiceRow>> {
    let (job_cfg, job_id) = (anvil::load().map_err(err)?, id.clone());
    tauri::async_runtime::spawn_blocking(move || {
        anvil::services::ensure_installed(&job_cfg, &job_id)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(err)?;
    let app = handle.state::<App>();
    let mut cfg = anvil::load().map_err(err)?;
    anvil::services::set_enabled(&mut cfg, &id, true).map_err(err)?;
    start_service(&app, &cfg, &id);
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
    let mut cfg = anvil::load().map_err(err)?;
    if action != "restart" {
        anvil::services::set_enabled(&mut cfg, &id, action == "start").map_err(err)?;
    }
    if action != "start" {
        app.services.lock().unwrap().stop(&id);
        app.service_errors.lock().unwrap().remove(&id);
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
    let mut cfg = anvil::load().map_err(err)?;
    anvil::services::set_tags(&mut cfg, &id, &tags).map_err(err)?;
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
    let mut cfg = anvil::load().map_err(err)?;
    anvil::services::set_apps(&mut cfg, &id, &apps).map_err(err)?;
    changed(&handle);
    services(app)
}

/// `what` is "log" (opens in Console) or "data" (shows the folder in Finder).
#[tauri::command(async)]
fn open_service(id: String, what: String) -> Res<()> {
    let cfg = anvil::load().map_err(err)?;
    if !cfg.services.contains_key(&id) {
        return Err(format!("There's no service called {id}"));
    }
    let (path, app) = if what == "log" {
        (anvil::services::log_file(&id), "Console")
    } else {
        (anvil::services::data(&id), "Finder")
    };
    if !path.exists() {
        return Err(format!("Nothing there yet — start {id} first"));
    }
    anvil::run(Command::new("open").arg("-a").arg(app).arg(path)).map_err(err)
}

/// The newest lines of a service's log as written (colour codes removed), oldest
/// first; none when it hasn't written one. Reads only the file's last 16 KB.
#[tauri::command(async)]
fn service_log(id: String) -> Res<Vec<String>> {
    use std::io::{Read, Seek, SeekFrom};
    let cfg = anvil::load().map_err(err)?;
    if !cfg.services.contains_key(&id) {
        return Err(format!("No service named {id}"));
    }
    let Ok(mut f) = std::fs::File::open(anvil::services::log_file(&id)) else {
        return Ok(vec![]);
    };
    let len = f.metadata().map_err(|e| e.to_string())?.len();
    let start = len.saturating_sub(16 * 1024);
    f.seek(SeekFrom::Start(start)).map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&buf);
    // From the middle of the file, the first line is likely cut: skip it.
    let lines: Vec<String> = text
        .lines()
        .skip(usize::from(start > 0))
        .map(anvil::logs::strip_ansi)
        .filter(|l| !l.trim().is_empty())
        .collect();
    Ok(lines[lines.len().saturating_sub(8)..].to_vec())
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
    anvil::run(Command::new("open").arg("-a").arg(&client).arg(url)).map_err(err)
}

#[tauri::command(async)]
fn service_kinds() -> Res<Vec<anvil::services::Kind>> {
    Ok(anvil::services::kinds(&anvil::load().map_err(err)?))
}

/// Like creating a container: downloads the program the first time, then starts it.
#[tauri::command]
async fn add_service(
    handle: AppHandle,
    kind: String,
    name: Option<String>,
    port: Option<u16>,
    version: Option<String>,
    tags: Vec<String>,
    apps: Vec<String>,
) -> Res<Vec<ServiceRow>> {
    let mut cfg = anvil::load().map_err(err)?;
    let id = anvil::services::add(&mut cfg, &kind, name.as_deref(), port, version.as_deref())
        .map_err(err)?;
    let meta = anvil::services::set_tags(&mut cfg, &id, &tags)
        .and_then(|_| anvil::services::set_apps(&mut cfg, &id, &apps));
    if let Err(e) = meta {
        let _ = anvil::services::remove(&mut cfg, &id, false);
        return Err(err(e));
    }
    let (job_cfg, job_id) = (anvil::load().map_err(err)?, id.clone());
    let installed = tauri::async_runtime::spawn_blocking(move || {
        anvil::services::ensure_installed(&job_cfg, &job_id)
    })
    .await
    .map_err(|e| e.to_string())?;
    // The install can take minutes; pick up changes made meanwhile before saving.
    let mut cfg = anvil::load().map_err(err)?;
    if let Err(e) = installed {
        let _ = anvil::services::remove(&mut cfg, &id, false);
        return Err(err(e));
    }
    anvil::services::set_enabled(&mut cfg, &id, true).map_err(err)?;
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
    app.services.lock().unwrap().stop(&id);
    app.service_errors.lock().unwrap().remove(&id);
    let mut cfg = anvil::load().map_err(err)?;
    anvil::services::remove(&mut cfg, &id, delete_data).map_err(err)?;
    changed(&handle);
    services(app)
}

#[tauri::command(async)]
fn open_url(url: String) -> Res<()> {
    if !url.starts_with("http://127.0.0.1:") && !url.starts_with("https://") {
        return Err("Only local and https links can be opened".into());
    }
    anvil::run(Command::new("open").arg(url)).map_err(err)
}

/* ---------- Settings ---------- */

#[derive(Serialize)]
struct PhpVersion {
    version: &'static str,
    installed: bool,
    default: bool,
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
    https_default: bool,
    login: bool,
    config_dir: std::path::PathBuf,
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
    let user =
        std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("Applications");
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
    Ok(Settings {
        php: anvil::PHP_VERSIONS
            .iter()
            .map(|v| PhpVersion {
                version: v,
                installed: installed.iter().any(|i| i == v),
                default: cfg.php.as_deref() == Some(*v),
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
        https_default: cfg.https_default == Some(true),
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
    let mut cfg = anvil::load().map_err(err)?;
    f(&mut cfg).map_err(err)?;
    anvil::save(&cfg).map_err(err)?;
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
            let path = path.canonicalize()?;
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
    let names = app.apps.lock().unwrap().names();
    for name in names {
        app.apps.lock().unwrap().stop(&name);
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

#[tauri::command(async)]
fn set_login(handle: AppHandle, on: bool) -> Res<Settings> {
    use tauri_plugin_autostart::ManagerExt;
    let auto = handle.autolaunch();
    if on { auto.enable() } else { auto.disable() }.map_err(|e| e.to_string())?;
    settings(handle)
}

#[tauri::command(async)]
fn open_config_dir() -> Res<()> {
    anvil::run(Command::new("open").arg(anvil::home())).map_err(err)
}

#[tauri::command(async)]
fn set_default_php(handle: AppHandle, version: String) -> Res<Settings> {
    apply(&handle, |cfg| anvil::set_default_php(cfg, &version), true)
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

#[tauri::command(async)]
fn php_uninstall(handle: AppHandle, version: String) -> Res<Settings> {
    apply(&handle, |cfg| anvil::php_uninstall(cfg, &version), false)
}

fn open_settings(handle: &AppHandle) {
    if let Some(w) = handle.get_webview_window("settings") {
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let built =
        WebviewWindowBuilder::new(handle, "settings", WebviewUrl::App("settings.html".into()))
            .title("Anvil Settings")
            .inner_size(780.0, 640.0)
            .min_inner_size(640.0, 480.0)
            .title_bar_style(TitleBarStyle::Overlay)
            .hidden_title(true)
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

/// The task-manager window: everything Anvil runs, with usage and logs.
fn open_processes(handle: &AppHandle) {
    if let Some(w) = handle.get_webview_window("processes") {
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let built = WebviewWindowBuilder::new(
        handle,
        "processes",
        WebviewUrl::App("processes.html".into()),
    )
    .title("Processes")
    .inner_size(820.0, 560.0)
    .min_inner_size(640.0, 420.0)
    .title_bar_style(TitleBarStyle::Overlay)
    .hidden_title(true)
    .build();
    if built.is_ok() {
        apply_theme(handle, anvil::load().ok().and_then(|c| c.theme).as_deref());
        update_dock(handle);
    }
}

#[tauri::command(async)]
fn show_processes(handle: AppHandle) {
    open_processes(&handle);
}

#[tauri::command(async)]
fn open(site: String, target: String) -> Res<()> {
    let cfg = anvil::load().map_err(err)?;
    let s = anvil::sites(&cfg)
        .into_iter()
        .find(|s| s.name == site)
        .ok_or_else(|| format!("No site named {site}"))?;
    let mut cmd = Command::new("open");
    match target.as_str() {
        "browser" => cmd.arg(anvil::site_url(&cfg, &s)),
        "finder" => cmd.arg(&s.path),
        "root" => cmd.arg(&s.root),
        "terminal" => cmd
            .args(["-a", cfg.terminal.as_deref().unwrap_or("Terminal")])
            .arg(&s.path),
        "editor" => {
            let editor = cfg.editor.ok_or("No editor set in ~/.anvil/config.toml")?;
            cmd.args(["-a", &editor]).arg(&s.path)
        }
        "git" => {
            let client = git_client(&cfg).ok_or("No git app installed")?;
            cmd.args(["-a", &client]).arg(&s.path)
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
        .quit()
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
        .item(&item("tab:logs", "Logs", Some("CmdOrCtrl+3"))?)
        .item(&item("tab:packages", "Packages", Some("CmdOrCtrl+4"))?)
        .item(&item("tab:resources", "Resources", Some("CmdOrCtrl+5"))?)
        .item(&item("tab:settings", "Settings", Some("CmdOrCtrl+6"))?)
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
    if action == "settings" {
        return open_settings(handle);
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
    } else if app.error.lock().unwrap().is_some() {
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
        "start" | "restart" => {
            if let Ok(cfg) = anvil::load() {
                start(&app, &cfg);
            }
        }
        "stop" => stop(&app),
        "logs" => {
            let _ = std::fs::create_dir_all(home.join("log"));
            let _ = Command::new("open").arg(home.join("log")).status();
        }
        "settings" => open_settings(handle),
        "quit" => handle.exit(0),
        _ => {
            if let Some(v) = id.strip_prefix("php:")
                && let Ok(mut cfg) = anvil::load()
            {
                let set = anvil::set_default_php(&mut cfg, v);
                if let Err(e) = set {
                    *app.error.lock().unwrap() = Some(err(e));
                } else if running(&app) {
                    start(&app, &cfg);
                }
            }
        }
    }
    changed(handle);
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(App::default())
        .invoke_handler(tauri::generate_handler![
            state,
            activity,
            activity_log,
            refresh,
            set_running,
            set_php,
            add_alias,
            link_folder,
            create_app,
            open_create_log,
            park_folder,
            remove_site,
            unhide_site,
            setup_status,
            setup_fix,
            set_group,
            set_site_ini,
            set_secure,
            set_https_default,
            allow_next_origin,
            set_runtime,
            open,
            icon,
            settings,
            show_settings,
            show_processes,
            add_path,
            remove_path,
            set_editor,
            set_default_php,
            set_ini,
            php_install,
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
            container_log,
            service_log,
            set_dock_icon,
            set_login,
            open_config_dir,
            services,
            set_service,
            set_service_port,
            install_service,
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
            resources,
            packages,
            package_updates,
            clean,
            log_sources,
            log_read,
            log_tail,
            log_search,
            log_clear,
            log_open,
            storage_anvil,
            storage_apps,
            storage_last,
            clean_anvil,
            clean_all_safe,
            runtimes,
            runtime_install,
            runtime_uninstall
        ])
        .setup(|app| {
            if let Ok(cfg) = anvil::load() {
                apply_theme(app.handle(), cfg.theme.as_deref());
                // Starting everything (and the first web server download) happens in the
                // background, so the window and menu bar icon appear right away.
                let handle = app.handle().clone();
                tauri::async_runtime::spawn_blocking(move || {
                    let state = handle.state::<App>();
                    start(&state, &cfg);
                    let failed = state.services.lock().unwrap().start_enabled(&cfg);
                    state.service_errors.lock().unwrap().extend(failed);
                    for site in anvil::sites(&cfg).iter().filter(|s| s.autostart) {
                        start_app(&state, &cfg, &site.name);
                    }
                    changed(&handle);
                });
            }
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
        .menu(app_menu)
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
                _ => {}
            }
        })
        .build(tauri::generate_context!())
        .expect("failed to build Anvil")
        .run(|handle, e| match e {
            RunEvent::Exit => {
                let app = handle.state::<App>();
                *app.apps.lock().unwrap() = Default::default();
                stop(&app);
                // Databases shut down cleanly before Anvil exits.
                *app.services.lock().unwrap() = Default::default();
            }
            #[cfg(target_os = "macos")]
            RunEvent::Reopen { .. } => show(handle),
            _ => {}
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_keys_are_parsed_and_paths_rejected() {
        let parse = |k| parse_log_key(k, &["redis".to_string()]);
        assert_eq!(parse("web:caddy"), Ok(LogKey::Caddy));
        assert_eq!(parse("web:php-8.4"), Ok(LogKey::Php("8.4")));
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
}
