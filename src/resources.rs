//! What an application costs: disk per kind of folder (and which are safe to
//! delete), memory and CPU of its processes, and the services it talks to.

use crate::{Config, Site, apps, home, services};
use anyhow::{Result, bail};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Serialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Safety {
    /// Delete any time; nothing is lost (logs, caches).
    Safe,
    /// Delete to free space; a command rebuilds it (dependencies, builds).
    Rebuild,
    /// Your data or code: don't delete.
    Keep,
}

#[derive(Serialize, Debug)]
pub struct DiskItem {
    pub label: String,
    /// Relative to the project, or absolute for files Anvil keeps elsewhere.
    pub path: String,
    pub bytes: u64,
    pub category: &'static str,
    pub safety: Safety,
    /// How it comes back, or why to keep it.
    pub note: &'static str,
}

#[derive(Serialize)]
pub struct Process {
    pub label: String,
    pub memory: u64,
    pub cpu: f32,
    pub shared: bool,
}

#[derive(Serialize)]
pub struct ServiceUse {
    /// What the app's config asks for, e.g. "MySQL" or "Redis".
    pub label: String,
    pub port: Option<u16>,
    /// The Anvil service on that port, if any.
    pub anvil: Option<&'static str>,
    pub running: bool,
    /// Size of the service's data folder (shared by every app using it).
    pub data_bytes: Option<u64>,
    pub memory: Option<u64>,
    pub cpu: Option<f32>,
}

#[derive(Serialize)]
pub struct Resources {
    pub total: u64,
    pub items: Vec<DiskItem>,
    pub processes: Vec<Process>,
    pub services: Vec<ServiceUse>,
}

type Rule = (
    &'static str,
    &'static str,
    &'static str,
    Safety,
    &'static str,
);

/// Known folders per kind: (path, label, category, safety, note).
fn rules(kind: &str) -> Vec<Rule> {
    use Safety::*;
    let node: [Rule; 1] = [(
        "node_modules",
        "Node packages",
        "Dependencies",
        Rebuild,
        "npm install brings them back",
    )];
    let mut r: Vec<Rule> = match kind {
        "laravel" | "statamic" | "php" => vec![
            (
                "vendor",
                "Composer packages",
                "Dependencies",
                Rebuild,
                "composer install brings them back",
            ),
            (
                "public/build",
                "Built assets",
                "Build",
                Rebuild,
                "npm run build recreates them",
            ),
            (
                "storage/logs",
                "Application logs",
                "Logs",
                Safe,
                "Only history; the app writes new ones",
            ),
            (
                "storage/framework/cache",
                "Cache",
                "Cache",
                Safe,
                "Rebuilt as the app runs",
            ),
            (
                "storage/framework/views",
                "Compiled views",
                "Cache",
                Safe,
                "Recompiled on the next request",
            ),
            (
                "storage/framework/sessions",
                "Sessions",
                "Cache",
                Safe,
                "Logs everyone out",
            ),
            (
                "bootstrap/cache",
                "Framework cache",
                "Cache",
                Safe,
                "Rebuilt on the next request",
            ),
            (
                "storage/app",
                "Uploaded and stored files",
                "Data",
                Keep,
                "Your application's files",
            ),
            (
                "database/database.sqlite",
                "SQLite database",
                "Data",
                Keep,
                "Your data",
            ),
        ],
        "next" => vec![(
            ".next",
            "Next.js build",
            "Build",
            Rebuild,
            "Rebuilt by the dev server",
        )],
        "nuxt" => vec![
            (
                ".nuxt",
                "Nuxt build",
                "Build",
                Rebuild,
                "Rebuilt by the dev server",
            ),
            (
                ".output",
                "Nuxt output",
                "Build",
                Rebuild,
                "npm run build recreates it",
            ),
        ],
        "vite" | "node" => vec![(
            "dist",
            "Build output",
            "Build",
            Rebuild,
            "npm run build recreates it",
        )],
        "rust" => vec![(
            "target",
            "Compiled build",
            "Build",
            Rebuild,
            "cargo rebuilds it (the first build is slow)",
        )],
        "go" => vec![(
            "vendor",
            "Vendored modules",
            "Dependencies",
            Rebuild,
            "go mod vendor brings them back",
        )],
        "dotnet" => vec![
            (
                "bin",
                "Compiled build",
                "Build",
                Rebuild,
                "dotnet build recreates it",
            ),
            (
                "obj",
                "Intermediate build",
                "Build",
                Rebuild,
                "dotnet build recreates it",
            ),
        ],
        _ => vec![],
    };
    if !matches!(kind, "rust" | "go" | "dotnet") {
        r.extend(node);
    }
    r
}

/// Folders known from marker files in the project (Flutter, CocoaPods, Gradle,
/// Python), whatever its kind. A folder that could hold source (`build`) is
/// only listed when a marker says a tool owns it.
fn marker_rules(dir: &Path) -> Vec<Rule> {
    use Safety::*;
    let has = |p: &str| dir.join(p).exists();
    let gradle = |d: &str| {
        [
            "build.gradle",
            "build.gradle.kts",
            "settings.gradle",
            "settings.gradle.kts",
        ]
        .iter()
        .any(|f| dir.join(d).join(f).exists())
    };
    let mut r: Vec<Rule> = vec![];
    let flutter = has("pubspec.yaml");
    if flutter {
        r.push((
            "build",
            "Flutter build",
            "Build",
            Rebuild,
            "flutter build recreates it",
        ));
        r.push((
            ".dart_tool",
            "Dart tool cache",
            "Build",
            Rebuild,
            "flutter pub get recreates it",
        ));
    }
    if has("ios/Podfile") {
        r.push((
            "ios/Pods",
            "CocoaPods",
            "Dependencies",
            Rebuild,
            "pod install brings them back",
        ));
        r.push((
            "ios/build",
            "iOS build",
            "Build",
            Rebuild,
            "The next iOS build recreates it",
        ));
    }
    if has("Podfile") {
        r.push((
            "Pods",
            "CocoaPods",
            "Dependencies",
            Rebuild,
            "pod install brings them back",
        ));
    }
    if gradle("android") {
        for (p, label) in [
            ("android/.gradle", "Gradle cache (Android)"),
            ("android/app/build", "Android app build"),
            ("android/build", "Android build"),
        ] {
            r.push((p, label, "Build", Rebuild, "Gradle rebuilds it"));
        }
    }
    if gradle("") {
        r.push((
            ".gradle",
            "Gradle cache",
            "Build",
            Rebuild,
            "Gradle rebuilds it",
        ));
        if !flutter {
            r.push((
                "build",
                "Gradle build",
                "Build",
                Rebuild,
                "Gradle rebuilds it",
            ));
        }
    }
    if has("pyproject.toml") || has("requirements.txt") || has("setup.py") {
        for p in [".venv", "venv"] {
            r.push((
                p,
                "Python virtual environment",
                "Dependencies",
                Rebuild,
                "recreate the virtual environment and reinstall",
            ));
        }
        r.push((
            "__pycache__",
            "Python bytecode",
            "Cache",
            Safe,
            "Recompiled on the next run",
        ));
        for (p, label) in [
            (".pytest_cache", "pytest cache"),
            (".mypy_cache", "mypy cache"),
            (".ruff_cache", "Ruff cache"),
        ] {
            r.push((p, label, "Cache", Safe, "Rebuilt on the next run"));
        }
    }
    r.push((
        ".cache",
        "Tool cache",
        "Cache",
        Safe,
        "Rebuilt by the tools that use it",
    ));
    r.push((
        "coverage",
        "Coverage report",
        "Cache",
        Safe,
        "test coverage output",
    ));
    r
}

/// Every rule for this project: its kind's rules, then marker-based ones for
/// paths the kind doesn't already cover. `disk`, `clean` and friends all use this.
fn rules_for(site: &Site) -> Vec<Rule> {
    let mut r = rules(&site.kind);
    for m in marker_rules(&site.path) {
        if !r.iter().any(|k| k.0 == m.0) {
            r.push(m);
        }
    }
    r
}

/// Sizes in bytes for several paths with one `du` call (missing paths are skipped).
fn du(paths: &[PathBuf]) -> BTreeMap<PathBuf, u64> {
    let existing: Vec<&PathBuf> = paths.iter().filter(|p| p.exists()).collect();
    if existing.is_empty() {
        return BTreeMap::new();
    }
    let out = Command::new("du").arg("-sk").args(&existing).output();
    let Ok(out) = out else {
        return BTreeMap::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let (kb, path) = l.split_once('\t')?;
            Some((PathBuf::from(path), kb.trim().parse::<u64>().ok()? * 1024))
        })
        .collect()
}

pub fn disk(site: &Site) -> (u64, Vec<DiskItem>) {
    let rules = rules_for(site);
    let anvil_log = apps::log_file(&site.name);
    let mut paths: Vec<PathBuf> = rules.iter().map(|r| site.path.join(r.0)).collect();
    paths.push(site.path.clone());
    paths.push(anvil_log.clone());
    let sizes = du(&paths);

    let mut items: Vec<DiskItem> = rules
        .iter()
        .filter_map(|(rel, label, category, safety, note)| {
            let bytes = *sizes.get(&site.path.join(rel))?;
            Some(DiskItem {
                label: label.to_string(),
                path: rel.to_string(),
                bytes,
                category,
                safety: *safety,
                note,
            })
        })
        .collect();
    if let Some(&bytes) = sizes.get(&anvil_log) {
        items.push(DiskItem {
            label: "Anvil run log".into(),
            path: anvil_log.display().to_string(),
            bytes,
            category: "Logs",
            safety: Safety::Safe,
            note: "Output of the app's dev server",
        });
    }
    let total = sizes.get(&site.path).copied().unwrap_or(0);
    let known: u64 = items
        .iter()
        .filter(|i| !i.path.starts_with('/'))
        .map(|i| i.bytes)
        .sum();
    items.push(DiskItem {
        label: "Source and everything else".into(),
        path: String::new(),
        bytes: total.saturating_sub(known),
        category: "Source",
        safety: Safety::Keep,
        note: "Your code",
    });
    items.sort_by_key(|i| std::cmp::Reverse(i.bytes));
    (total, items)
}

struct Ps {
    pid: u32,
    ppid: u32,
    pgid: u32,
    rss: u64,
    cpu: f32,
    command: String,
}

fn ps() -> Vec<Ps> {
    let out = Command::new("ps")
        .args(["-A", "-o", "pid=,ppid=,pgid=,rss=,%cpu=,command="])
        .output();
    let Ok(out) = out else { return vec![] };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let mut f = l.split_whitespace();
            Some(Ps {
                pid: f.next()?.parse().ok()?,
                ppid: f.next()?.parse().ok()?,
                pgid: f.next()?.parse().ok()?,
                rss: f.next()?.parse::<u64>().ok()? * 1024,
                cpu: f.next()?.parse().ok()?,
                command: f.collect::<Vec<_>>().join(" "),
            })
        })
        .collect()
}

pub fn processes(site: &Site, app_pid: Option<u32>) -> Vec<Process> {
    let table = ps();
    let sum = |pred: &dyn Fn(&Ps) -> bool| -> (u64, f32, usize) {
        table
            .iter()
            .filter(|p| pred(p))
            .fold((0, 0.0, 0), |(m, c, n), p| (m + p.rss, c + p.cpu, n + 1))
    };
    let mut out = vec![];
    if let Some(pid) = app_pid {
        let (memory, cpu, _) = sum(&|p| p.pgid == pid);
        out.push(Process {
            label: format!("{} dev server", apps::label(&site.kind)),
            memory,
            cpu,
            shared: false,
        });
    }
    if let Some(v) = &site.php {
        // Our PHP-FPM master for this version, and its workers (other tools' FPMs look alike).
        let conf = home().join("run").join(format!("php-{v}-fpm.conf"));
        let conf = conf.display().to_string();
        let master = table
            .iter()
            .find(|p| p.command.contains(&conf))
            .map(|p| p.pid);
        let pool = if site.ini.is_empty() {
            "pool www".to_string()
        } else {
            format!("pool site-{}", site.name.replace('.', "_"))
        };
        let (memory, cpu, n) = sum(&|p| Some(p.ppid) == master && p.command.contains(&pool));
        out.push(if site.ini.is_empty() {
            Process {
                label: format!("PHP {v}, {n} workers shared by all PHP apps"),
                memory,
                cpu,
                shared: true,
            }
        } else {
            Process {
                label: format!("PHP {v}, own pool, {n} workers"),
                memory,
                cpu,
                shared: false,
            }
        });
    }
    out
}

/// Read `KEY=value` pairs from the project's .env.
fn dotenv(dir: &Path) -> BTreeMap<String, String> {
    std::fs::read_to_string(dir.join(".env"))
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| {
            let (k, v) = l.split_once('=')?;
            Some((
                k.trim().to_string(),
                v.trim().trim_matches(['"', '\'']).to_string(),
            ))
        })
        .collect()
}

/// The port in a URL like `postgres://user:pw@127.0.0.1:5433/db`.
fn url_port(url: &str) -> Option<u16> {
    let host = url.split("://").nth(1)?.split('/').next()?;
    let host = host.rsplit('@').next()?;
    host.rsplit_once(':')?.1.parse().ok()
}

/// Services the app's .env points at, matched to Anvil services by port.
pub fn services_used(cfg: &Config, site: &Site, svc: &mut services::Services) -> Vec<ServiceUse> {
    let env = dotenv(&site.path);
    let get = |k: &str| env.get(k).map(String::as_str);
    let mut wanted: Vec<(String, Option<u16>)> = vec![];
    match get("DB_CONNECTION") {
        Some("mysql" | "mariadb") => wanted.push((
            "MySQL".into(),
            get("DB_PORT").and_then(|p| p.parse().ok()).or(Some(3306)),
        )),
        Some("pgsql") => wanted.push((
            "PostgreSQL".into(),
            get("DB_PORT").and_then(|p| p.parse().ok()).or(Some(5432)),
        )),
        Some("sqlite") => wanted.push(("SQLite (file in the project)".into(), None)),
        Some(other) => wanted.push((
            format!("Database ({other})"),
            get("DB_PORT").and_then(|p| p.parse().ok()),
        )),
        None => {}
    }
    if let Some(url) = get("DATABASE_URL") {
        let label = if url.starts_with("postgres") {
            "PostgreSQL"
        } else if url.starts_with("mysql") {
            "MySQL"
        } else {
            "Database"
        };
        wanted.push((label.into(), url_port(url)));
    }
    let uses_redis = [
        "CACHE_STORE",
        "CACHE_DRIVER",
        "QUEUE_CONNECTION",
        "SESSION_DRIVER",
    ]
    .iter()
    .any(|k| get(k) == Some("redis"))
        || get("REDIS_URL").is_some();
    if uses_redis {
        let port = get("REDIS_PORT")
            .and_then(|p| p.parse().ok())
            .or_else(|| get("REDIS_URL").and_then(url_port))
            .or(Some(6379));
        wanted.push(("Redis".into(), port));
    }
    if matches!(get("MAIL_MAILER"), Some("smtp")) {
        wanted.push((
            "Mail (SMTP)".into(),
            get("MAIL_PORT").and_then(|p| p.parse().ok()),
        ));
    }

    let table = ps();
    let data_sizes = du(&services::SERVICES
        .iter()
        .map(|id| home().join("data").join(id))
        .collect::<Vec<_>>());
    wanted
        .into_iter()
        .map(|(label, port)| {
            let anvil = port.and_then(|p| {
                services::SERVICES
                    .into_iter()
                    .find(|id| services::installed(id) && services::port(cfg, id) == p)
            });
            let proc = anvil
                .and_then(|id| svc.pid(id))
                .and_then(|pid| table.iter().find(|p| p.pid == pid));
            ServiceUse {
                memory: proc.map(|p| p.rss),
                cpu: proc.map(|p| p.cpu),
                running: anvil.is_some_and(|id| svc.running(id)),
                data_bytes: anvil
                    .and_then(|id| data_sizes.get(&home().join("data").join(id)).copied()),
                label,
                port,
                anvil,
            }
        })
        .collect()
}

/// Anvil's own footprint: installed versions, service data, caches and logs.
pub fn anvil_disk() -> (u64, Vec<DiskItem>) {
    let h = home();
    let mut entries: Vec<(String, PathBuf, &'static str, Safety, &'static str)> = vec![];
    let dirs = |p: PathBuf| -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(p)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().is_dir())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    };
    for v in dirs(h.join("php")) {
        entries.push((
            format!("PHP {v}"),
            h.join("php").join(&v),
            "PHP",
            Safety::Keep,
            "Remove it in Settings › PHP",
        ));
    }
    for id in crate::runtimes::RUNTIMES {
        let name = crate::runtimes::name(id);
        entries.push((
            name.to_string(),
            h.join("runtimes").join(id),
            "Languages",
            Safety::Keep,
            "Remove versions in Settings › Languages",
        ));
    }
    for id in services::SERVICES {
        entries.push((
            format!("{id} program"),
            h.join("services").join(id),
            "Services",
            Safety::Keep,
            "Uninstall it in Settings › Services",
        ));
        entries.push((
            format!("{id} data"),
            h.join("data").join(id),
            "Data",
            Safety::Keep,
            "Your databases and mail",
        ));
    }
    entries.push((
        "Web server".into(),
        h.join("bin"),
        "Anvil",
        Safety::Keep,
        "Needed to serve your apps",
    ));
    entries.push((
        "Certificates".into(),
        h.join("data").join("caddy"),
        "Anvil",
        Safety::Keep,
        "Your local HTTPS certificates",
    ));
    for c in dirs(h.join("cache")) {
        entries.push((
            format!("{c} cache"),
            h.join("cache").join(&c),
            "Cache",
            Safety::Safe,
            "Downloaded again when a build needs it",
        ));
    }
    entries.push((
        "Logs".into(),
        h.join("log"),
        "Logs",
        Safety::Safe,
        "Only history; new lines keep coming",
    ));
    entries.push((
        "Build leftovers".into(),
        h.join("build"),
        "Cache",
        Safety::Safe,
        "Temporary files from installs",
    ));

    let sizes = du(&entries.iter().map(|e| e.1.clone()).collect::<Vec<_>>());
    let mut items: Vec<DiskItem> = entries
        .into_iter()
        .filter_map(|(label, path, category, safety, note)| {
            let bytes = *sizes.get(&path)?;
            Some(DiskItem {
                label,
                path: path.display().to_string(),
                bytes,
                category,
                safety,
                note,
            })
        })
        .filter(|i| i.bytes > 0)
        .collect();
    items.sort_by_key(|i| std::cmp::Reverse(i.bytes));
    (items.iter().map(|i| i.bytes).sum(), items)
}

/// Delete one of Anvil's own cleanable items: a cache folder, leftovers, or logs.
/// Logs are truncated, not deleted, so running programs keep a valid file.
pub fn clean_anvil(path: &str) -> Result<u64> {
    let h = home();
    let p = PathBuf::from(path);
    let freed: u64 = du(std::slice::from_ref(&p)).values().sum();
    if p == h.join("log") {
        for e in std::fs::read_dir(&p)?.flatten() {
            if e.path().is_file() {
                std::fs::write(e.path(), "")?;
            }
        }
    } else if p == h.join("build") || p.parent() == Some(h.join("cache").as_path()) {
        remove(&p)?;
    } else {
        bail!("Anvil doesn't clean {path}");
    }
    Ok(freed)
}

#[derive(Serialize)]
pub struct AppSize {
    pub name: String,
    pub kind: String,
    pub total: u64,
    pub safe: u64,
    pub rebuild: u64,
}

/// Sizes of every app at once (one `du` run), biggest first.
pub fn all_apps(sites: &[Site]) -> Vec<AppSize> {
    let mut paths = vec![];
    for s in sites
        .iter()
        .filter(|s| !s.linked || !sites.iter().any(|o| !o.linked && o.path == s.path))
    {
        paths.push(s.path.clone());
        paths.extend(rules_for(s).iter().map(|r| s.path.join(r.0)));
    }
    let sizes = du(&paths);
    let mut seen = std::collections::BTreeSet::new();
    let mut out: Vec<AppSize> = sites
        .iter()
        .filter(|s| seen.insert(s.path.clone()))
        .filter_map(|s| {
            let total = *sizes.get(&s.path)?;
            let sum = |want: Safety| -> u64 {
                rules_for(s)
                    .iter()
                    .filter(|r| r.3 == want)
                    .filter_map(|r| sizes.get(&s.path.join(r.0)))
                    .sum()
            };
            Some(AppSize {
                name: s.name.clone(),
                kind: s.kind.clone(),
                total,
                safe: sum(Safety::Safe),
                rebuild: sum(Safety::Rebuild),
            })
        })
        .collect();
    out.sort_by_key(|a| std::cmp::Reverse(a.total));
    out
}

/// Delete the "safe" items (logs, caches) of every app. Returns bytes freed and
/// the apps where something failed.
pub fn clean_all_safe(sites: &[Site]) -> (u64, Vec<String>) {
    let mut freed = 0;
    let mut failed = vec![];
    let mut seen = std::collections::BTreeSet::new();
    for s in sites.iter().filter(|s| seen.insert(s.path.clone())) {
        for r in rules_for(s).iter().filter(|r| r.3 == Safety::Safe) {
            match clean(s, r.0, false) {
                Ok(n) => freed += n,
                Err(_) => failed.push(s.name.clone()),
            }
        }
    }
    (freed, failed)
}

/// Empty a folder but keep it and its `.gitignore`/`.gitkeep` (Laravel expects
/// `storage/*` and `bootstrap/cache` to exist).
fn empty_dir(dir: &Path) -> Result<()> {
    for e in std::fs::read_dir(dir)?.flatten() {
        let name = e.file_name();
        if name == ".gitignore" || name == ".gitkeep" {
            continue;
        }
        remove(&e.path())?;
    }
    Ok(())
}

/// Remove a file or folder; a symlink is removed itself, never what it points to.
fn remove(path: &Path) -> Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.is_dir() {
        std::fs::remove_dir_all(path)?;
    } else {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

/// Delete one item from `disk()`. Only paths Anvil knows for this kind of app and
/// marks safe or rebuildable are accepted. Returns the bytes freed.
pub fn clean(site: &Site, path: &str, app_running: bool) -> Result<u64> {
    let anvil_log = apps::log_file(&site.name);
    if Path::new(path) == anvil_log {
        let freed = std::fs::metadata(&anvil_log).map_or(0, |m| m.len());
        std::fs::write(&anvil_log, "")?;
        return Ok(freed);
    }
    let Some(rule) = rules_for(site).into_iter().find(|r| r.0 == path) else {
        bail!("Anvil doesn't clean {path} for this app");
    };
    if rule.3 == Safety::Keep {
        bail!("{} holds your data; Anvil won't delete it", rule.1);
    }
    if rule.3 == Safety::Rebuild && app_running {
        bail!("Stop the app before deleting {}", rule.1.to_lowercase());
    }
    let target = site.path.join(path);
    if std::fs::symlink_metadata(&target).is_err() {
        return Ok(0);
    }
    // A folder on the way (e.g. `ios` in `ios/Pods`) may itself be a symlink; never
    // follow one out of the project.
    let inside = target
        .parent()
        .and_then(|p| p.canonicalize().ok())
        .zip(site.path.canonicalize().ok())
        .is_some_and(|(parent, project)| parent.starts_with(project));
    if !inside {
        bail!("{path} leads outside the project; Anvil won't delete it");
    }
    let freed = du(std::slice::from_ref(&target)).values().sum();
    let contents_only = path.starts_with("storage/") || path == "bootstrap/cache";
    if contents_only && !std::fs::symlink_metadata(&target)?.file_type().is_symlink() {
        empty_dir(&target)?;
    } else {
        remove(&target)?;
    }
    Ok(freed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_keeps_data_and_gitignores() {
        let dir = std::env::temp_dir().join(format!("anvil-clean-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (f, body) in [
            ("storage/logs/laravel.log", "x".repeat(5000)),
            ("storage/logs/.gitignore", "*\n".into()),
            ("storage/app/upload.png", "keep".into()),
            ("vendor/pkg/a.php", "<?php".into()),
            ("outside/secret.txt", "s".into()),
        ] {
            let p = dir.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        }
        // node_modules as a symlink to a folder outside: only the link may go.
        std::os::unix::fs::symlink(dir.join("outside"), dir.join("node_modules")).unwrap();
        let site = Site {
            name: "t".into(),
            path: dir.clone(),
            root: dir.clone(),
            php: None,
            php_pinned: false,
            ini: BTreeMap::new(),
            linked: true,
            parked_in: None,
            modified: 0,
            kind: "laravel".into(),
            kind_detected: "laravel".into(),
            command: None,
            command_custom: false,
            port: None,
            autostart: false,
            runtime_version: None,
            runtime_pinned: false,
        };

        assert!(clean(&site, "storage/logs", false).unwrap() > 0);
        assert!(!dir.join("storage/logs/laravel.log").exists());
        assert!(dir.join("storage/logs/.gitignore").exists());

        assert!(clean(&site, "storage/app", false).is_err());
        assert!(clean(&site, "../outside", false).is_err());
        assert!(clean(&site, "/etc", false).is_err());
        assert!(clean(&site, "vendor", true).is_err()); // running app
        clean(&site, "vendor", false).unwrap();
        assert!(!dir.join("vendor").exists());

        clean(&site, "node_modules", false).unwrap();
        assert!(!dir.join("node_modules").exists());
        assert!(dir.join("outside/secret.txt").exists());
        assert!(dir.join("storage/app/upload.png").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn nested_rule_through_symlink_is_refused() {
        let base = std::env::temp_dir().join(format!("anvil-nested-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let project = base.join("app");
        let outside = base.join("elsewhere");
        std::fs::create_dir_all(outside.join("Pods")).unwrap();
        std::fs::write(outside.join("Pods/keep.txt"), "x").unwrap();
        std::fs::write(outside.join("Podfile"), "").unwrap();
        std::fs::create_dir_all(&project).unwrap();
        // `ios` points outside the project, so `ios/Pods` must not be deleted.
        std::os::unix::fs::symlink(&outside, project.join("ios")).unwrap();
        let site = Site {
            name: "n".into(),
            path: project.clone(),
            root: project.clone(),
            php: None,
            php_pinned: false,
            ini: BTreeMap::new(),
            linked: true,
            parked_in: None,
            modified: 0,
            kind: "static".into(),
            kind_detected: "static".into(),
            command: None,
            command_custom: false,
            port: None,
            autostart: false,
            runtime_version: None,
            runtime_pinned: false,
        };
        assert!(clean(&site, "ios/Pods", false).is_err());
        assert!(outside.join("Pods/keep.txt").exists());
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn rules_and_env() {
        let rust = rules("rust");
        assert!(
            rust.iter()
                .any(|r| r.0 == "target" && r.3 == Safety::Rebuild)
        );
        assert!(!rust.iter().any(|r| r.0 == "node_modules"));
        let laravel = rules("laravel");
        assert!(
            laravel
                .iter()
                .any(|r| r.0 == "storage/app" && r.3 == Safety::Keep)
        );
        assert!(
            laravel
                .iter()
                .any(|r| r.0 == "storage/logs" && r.3 == Safety::Safe)
        );
        assert_eq!(url_port("postgres://u:p@127.0.0.1:5433/app"), Some(5433));
        assert_eq!(url_port("redis://127.0.0.1"), None);
        assert!(clean_anvil("/etc").is_err());
        assert!(clean_anvil("/tmp/../etc").is_err());
    }

    fn project(name: &str, kind: &str, files: &[&str]) -> Site {
        let dir = std::env::temp_dir().join(format!("anvil-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for f in files {
            let p = dir.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, "x".repeat(5000)).unwrap();
        }
        Site {
            name: name.into(),
            path: dir.clone(),
            root: dir,
            php: None,
            php_pinned: false,
            ini: BTreeMap::new(),
            linked: true,
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

    fn safety(items: &[DiskItem], path: &str) -> Option<Safety> {
        items.iter().find(|i| i.path == path).map(|i| i.safety)
    }

    #[test]
    fn flutter_markers() {
        let site = project(
            "flutter",
            "static",
            &[
                "pubspec.yaml",
                "lib/main.dart",
                "build/app.dill",
                ".dart_tool/cache",
                "ios/Podfile",
                "ios/Pods/Firebase/a.h",
            ],
        );
        let (_, items) = disk(&site);
        assert_eq!(safety(&items, "build"), Some(Safety::Rebuild));
        assert_eq!(safety(&items, ".dart_tool"), Some(Safety::Rebuild));
        assert_eq!(safety(&items, "ios/Pods"), Some(Safety::Rebuild));
        assert_eq!(safety(&items, "lib"), None);

        assert!(clean(&site, "build", false).unwrap() > 0);
        assert!(!site.path.join("build").exists());
        assert!(clean(&site, "lib", false).is_err());
        assert!(clean(&site, "ios", false).is_err());
        assert!(site.path.join("lib/main.dart").exists());
        std::fs::remove_dir_all(&site.path).unwrap();
    }

    #[test]
    fn python_markers() {
        let site = project(
            "python",
            "static",
            &[
                "pyproject.toml",
                "app/main.py",
                ".venv/lib/site.py",
                "__pycache__/main.cpython-312.pyc",
            ],
        );
        let (_, items) = disk(&site);
        assert_eq!(safety(&items, ".venv"), Some(Safety::Rebuild));
        assert_eq!(safety(&items, "__pycache__"), Some(Safety::Safe));
        assert!(all_apps(std::slice::from_ref(&site))[0].safe > 0);
        let (freed, failed) = clean_all_safe(std::slice::from_ref(&site));
        assert!(freed > 0 && failed.is_empty());
        assert!(!site.path.join("__pycache__").exists());
        assert!(site.path.join(".venv/lib/site.py").exists());
        assert!(clean(&site, "app", false).is_err());
        std::fs::remove_dir_all(&site.path).unwrap();
    }

    #[test]
    fn build_without_markers_is_kept() {
        let site = project("plain", "static", &["index.html", "build/index.html"]);
        let (_, items) = disk(&site);
        assert_eq!(safety(&items, "build"), None);
        assert!(clean(&site, "build", false).is_err());
        assert!(site.path.join("build/index.html").exists());
        std::fs::remove_dir_all(&site.path).unwrap();
    }
}
