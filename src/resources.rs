//! What an application costs: disk per kind of folder (and which are safe to
//! delete), memory and CPU of its processes, and the services it talks to.

use crate::{Config, Site, apps, home, services};
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
    pub label: &'static str,
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
    let rules = rules(&site.kind);
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
                label,
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
            label: "Anvil run log",
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
        label: "Source and everything else",
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

#[cfg(test)]
mod tests {
    use super::*;

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
    }
}
