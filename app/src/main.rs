// Hide the extra console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anvil::{Config, Server, Site};
use serde::Serialize;
use std::process::Command;
use std::sync::Mutex;
use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{
    AppHandle, Manager, RunEvent, State, TitleBarStyle, WebviewUrl, WebviewWindowBuilder,
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
    php_default: Option<String>,
    php_installed: Vec<String>,
    editor: Option<String>,
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
        groups: cfg.groups.clone(),
        ini: cfg.ini.clone(),
        running: running(app),
        error: app.error.lock().unwrap().clone(),
        tld: cfg.tld,
        https_port: cfg.https_port,
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

#[tauri::command]
fn state(app: State<App>) -> Res<Snapshot> {
    snapshot(&app)
}

/// Rescan parked folders; re-serve only if the set of sites changed.
#[tauri::command]
fn refresh(handle: AppHandle, app: State<App>) -> Res<Snapshot> {
    let cfg = anvil::load().map_err(err)?;
    if running(&app) && anvil::needs_restart(&cfg) {
        start(&app, &cfg);
        changed(&handle);
    }
    snapshot(&app)
}

#[tauri::command]
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
#[tauri::command]
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

#[tauri::command]
fn app_run(handle: AppHandle, app: State<App>, site: String, action: String) -> Res<Snapshot> {
    let cfg = anvil::load().map_err(err)?;
    if action != "start" {
        app.apps.lock().unwrap().stop(&site);
        app.app_errors.lock().unwrap().remove(&site);
    }
    if action != "stop" {
        start_app(&app, &cfg, &site);
    }
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
        let services = anvil::resources::services_used(&cfg, &s, &mut app.services.lock().unwrap());
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

/// Every app's size; slow for many apps (one `du` over all of them).
#[tauri::command]
async fn storage_apps() -> Res<Vec<anvil::resources::AppSize>> {
    tauri::async_runtime::spawn_blocking(|| {
        let cfg = anvil::load().map_err(err)?;
        Ok(anvil::resources::all_apps(&anvil::sites(&cfg)))
    })
    .await
    .map_err(|e| e.to_string())?
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
fn open_log(site: String) -> Res<()> {
    let log = anvil::apps::log_file(&site);
    if !log.exists() {
        return Err("No log yet — start the app first".into());
    }
    anvil::run(Command::new("open").arg("-a").arg("Console").arg(log)).map_err(err)
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

#[tauri::command]
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

#[tauri::command]
fn open_create_log(name: String) -> Res<()> {
    anvil::run(
        Command::new("open")
            .args(["-a", "Console"])
            .arg(anvil::create::log_file(&name)),
    )
    .map_err(err)
}

/// Serve one project folder; its name comes from the folder.
#[tauri::command]
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
#[tauri::command]
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

#[tauri::command]
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
#[tauri::command]
fn remove_site(handle: AppHandle, app: State<App>, site: String) -> Res<Snapshot> {
    let mut cfg = anvil::load().map_err(err)?;
    anvil::remove(&mut cfg, &site).map_err(err)?;
    sites_changed(&handle, &app, &cfg);
    snapshot(&app)
}

/// Pin an app to a language version (`None`: from the project, or the newest).
#[tauri::command]
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

#[tauri::command]
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

#[tauri::command]
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
}

#[tauri::command]
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
            error: errors.get(info.id).cloned(),
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
#[tauri::command]
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

#[tauri::command]
fn set_service_port(
    handle: AppHandle,
    app: State<App>,
    id: String,
    port: Option<u16>,
) -> Res<Vec<ServiceRow>> {
    let mut cfg = anvil::load().map_err(err)?;
    anvil::services::set_port(&mut cfg, &id, port).map_err(err)?;
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
    let job = id.clone();
    tauri::async_runtime::spawn_blocking(move || anvil::services::install(&job))
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

#[tauri::command]
fn uninstall_service(handle: AppHandle, app: State<App>, id: String) -> Res<Vec<ServiceRow>> {
    app.services.lock().unwrap().stop(&id);
    let mut cfg = anvil::load().map_err(err)?;
    anvil::services::set_enabled(&mut cfg, &id, false).map_err(err)?;
    anvil::services::uninstall(&id).map_err(err)?;
    changed(&handle);
    services(app)
}

#[tauri::command]
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
    theme: Option<String>,
    dock_icon: bool,
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

#[tauri::command]
fn settings(handle: AppHandle) -> Res<Settings> {
    use tauri_plugin_autostart::ManagerExt;
    let cfg = anvil::load().map_err(err)?;
    let installed = anvil::installed_php();
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
        login: handle.autolaunch().is_enabled().unwrap_or(false),
        config_dir: anvil::home(),
        terminal: cfg.terminal,
        theme: cfg.theme,
        dock_icon: cfg.dock_icon,
        paths: cfg.paths,
        tld: cfg.tld,
        http_port: cfg.http_port,
        https_port: cfg.https_port,
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

#[tauri::command]
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

#[tauri::command]
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

#[tauri::command]
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

#[tauri::command]
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

#[tauri::command]
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

#[tauri::command]
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

#[tauri::command]
fn set_login(handle: AppHandle, on: bool) -> Res<Settings> {
    use tauri_plugin_autostart::ManagerExt;
    let auto = handle.autolaunch();
    if on { auto.enable() } else { auto.disable() }.map_err(|e| e.to_string())?;
    settings(handle)
}

#[tauri::command]
fn open_config_dir() -> Res<()> {
    anvil::run(Command::new("open").arg(anvil::home())).map_err(err)
}

#[tauri::command]
fn set_default_php(handle: AppHandle, version: String) -> Res<Settings> {
    apply(&handle, |cfg| anvil::set_default_php(cfg, &version), true)
}

/// `site: None` changes the default for every site.
#[tauri::command]
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

#[tauri::command]
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

#[tauri::command]
fn show_settings(handle: AppHandle) {
    open_settings(&handle);
}

#[tauri::command]
fn open(site: String, target: String) -> Res<()> {
    let cfg = anvil::load().map_err(err)?;
    let s = anvil::sites(&cfg)
        .into_iter()
        .find(|s| s.name == site)
        .ok_or_else(|| format!("No site named {site}"))?;
    let mut cmd = Command::new("open");
    match target.as_str() {
        "browser" => cmd.arg(format!("https://{}.{}:{}", s.name, cfg.tld, cfg.https_port)),
        "finder" => cmd.arg(&s.path),
        "terminal" => cmd
            .args(["-a", cfg.terminal.as_deref().unwrap_or("Terminal")])
            .arg(&s.path),
        "editor" => {
            let editor = cfg.editor.ok_or("No editor set in ~/.anvil/config.toml")?;
            cmd.args(["-a", &editor]).arg(&s.path)
        }
        other => return Err(format!("Unknown target {other}")),
    };
    anvil::run(&mut cmd).map_err(err)
}

/// The site's favicon as a data URL, so the webview never needs file access.
#[tauri::command]
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

fn menu(handle: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let app = handle.state::<App>();
    let on = running(&app);
    let cfg = anvil::load().unwrap_or_default();

    let status = if on {
        format!("Running {} sites", anvil::sites(&cfg).len())
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
        Box::new(item("sites", "Sites", true, None)?),
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
    for (i, v) in anvil::installed_php().iter().rev().enumerate() {
        let key = (i < 9).then(|| format!("CmdOrCtrl+{}", i + 1));
        items.push(Box::new(CheckMenuItem::with_id(
            handle,
            format!("php:{v}"),
            format!("Use PHP {v}"),
            true,
            cfg.php.as_deref() == Some(v.as_str()),
            key.as_deref(),
        )?));
    }
    items.extend([
        Box::new(sep()?) as Box<dyn IsMenuItem<Wry>>,
        Box::new(item("logs", "Logs", true, None)?),
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
            refresh,
            set_running,
            set_php,
            add_alias,
            link_folder,
            create_app,
            open_create_log,
            park_folder,
            remove_site,
            set_group,
            set_site_ini,
            set_runtime,
            open,
            icon,
            settings,
            show_settings,
            add_path,
            remove_path,
            set_editor,
            set_default_php,
            set_ini,
            php_install,
            php_uninstall,
            set_terminal,
            set_theme,
            set_dock_icon,
            set_login,
            open_config_dir,
            services,
            set_service,
            set_service_port,
            install_service,
            uninstall_service,
            open_url,
            app_run,
            set_app,
            open_log,
            resources,
            clean,
            storage_anvil,
            storage_apps,
            clean_anvil,
            clean_all_safe,
            runtimes,
            runtime_install,
            runtime_uninstall
        ])
        .setup(|app| {
            if let Ok(cfg) = anvil::load() {
                let state = app.state::<App>();
                start(&state, &cfg);
                let failed = state.services.lock().unwrap().start_enabled(&cfg);
                state.service_errors.lock().unwrap().extend(failed);
                for site in anvil::sites(&cfg).iter().filter(|s| s.autostart) {
                    start_app(&state, &cfg, &site.name);
                }
                apply_theme(app.handle(), cfg.theme.as_deref());
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
