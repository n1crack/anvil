// Hide the extra console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anvil::{Config, Server, Site};
use serde::Serialize;
use std::process::Command;
use std::sync::Mutex;
use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, RunEvent, State, WindowEvent, Wry};

#[derive(Default)]
struct App {
    server: Mutex<Option<Server>>,
    error: Mutex<Option<String>>,
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
    running: bool,
    error: Option<String>,
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
    Ok(Snapshot {
        sites: anvil::sites(&cfg),
        php_default: cfg.php.clone(),
        php_installed: anvil::installed_php(),
        editor: cfg.editor.clone(),
        groups: cfg.groups.clone(),
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
    if let Some(w) = handle.get_webview_window("main") {
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

#[tauri::command]
fn set_group(app: State<App>, site: String, group: Option<String>) -> Res<Snapshot> {
    let mut cfg = anvil::load().map_err(err)?;
    anvil::set_group(&mut cfg, &site, group.as_deref()).map_err(err)?;
    snapshot(&app)
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
        "terminal" => cmd.args(["-a", "Terminal"]).arg(&s.path),
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
        "settings" => {
            if let Ok(cfg) = anvil::load() {
                let _ = anvil::save(&cfg); // make sure the file exists
            }
            let _ = Command::new("open")
                .arg("-t")
                .arg(home.join("config.toml"))
                .status();
        }
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
        .manage(App::default())
        .invoke_handler(tauri::generate_handler![
            state,
            refresh,
            set_running,
            set_php,
            add_alias,
            remove_site,
            set_group,
            open,
            icon
        ])
        .setup(|app| {
            if let Ok(cfg) = anvil::load() {
                start(&app.state::<App>(), &cfg);
            }
            // ponytail: tray reuses the app icon; a monochrome template icon would follow the menu bar theme.
            TrayIconBuilder::with_id("anvil")
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("Anvil")
                .menu(&menu(app.handle())?)
                .on_menu_event(|handle, e| on_menu(handle, e.id.as_ref()))
                .build(app)?;
            Ok(())
        })
        .on_window_event(|w, e| {
            // Closing the window keeps Anvil running in the menu bar.
            if let WindowEvent::CloseRequested { api, .. } = e {
                api.prevent_close();
                let _ = w.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("failed to build Anvil")
        .run(|handle, e| match e {
            RunEvent::Exit => stop(&handle.state::<App>()),
            #[cfg(target_os = "macos")]
            RunEvent::Reopen { .. } => show(handle),
            _ => {}
        });
}
