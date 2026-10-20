//! What an application costs: disk per kind of folder (and which are safe to
//! delete), memory and CPU of its processes, and the services it talks to.

use crate::{Config, Site, apps, home, services};
use anyhow::{Result, bail};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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

/// Where a service the app uses comes from.
#[derive(Serialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Anvil,
    Docker,
    /// Another program on this Mac.
    Local,
    Remote,
    /// Nothing listens on the port, or there is no port (SQLite).
    None,
}

#[derive(Serialize)]
pub struct ServiceUse {
    /// What the app's config asks for, e.g. "MySQL" or "Redis".
    pub label: String,
    pub port: Option<u16>,
    /// The host from the app's config, when it sets one.
    pub host: Option<String>,
    pub source: Source,
    /// Container, process name, Compose service or remote host.
    pub detail: Option<String>,
    /// The Anvil service on that port, if any.
    pub anvil: Option<String>,
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
        "django" | "fastapi" | "flask" => vec![
            (
                ".venv",
                "Virtual environment",
                "Dependencies",
                Rebuild,
                "uv recreates it on the next start",
            ),
            (
                "__pycache__",
                "Compiled Python",
                "Build",
                Safe,
                "Python writes it again when it runs",
            ),
        ],
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
/// Allocated size of each path, like `du -sk` but in one parallel pass: every
/// folder is a job on a shared queue, and each file's blocks are added to the
/// counters of all requested paths above it. Nested paths cost nothing extra.
/// Symlinks aren't followed, and a hard-linked file counts once per requested
/// path, so every size equals `du -sk <path>` run on its own.
pub(crate) fn du(paths: &[PathBuf]) -> BTreeMap<PathBuf, u64> {
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Condvar, Mutex};

    let wanted: Vec<PathBuf> = paths
        .iter()
        .filter(|p| p.symlink_metadata().is_ok())
        .cloned()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let lookup: std::collections::HashMap<&Path, usize> = wanted
        .iter()
        .enumerate()
        .map(|(i, p)| (p.as_path(), i))
        .collect();
    let index = |p: &Path| lookup.get(p).copied();
    let counters: Vec<AtomicU64> = wanted.iter().map(|_| AtomicU64::new(0)).collect();
    // Hard links seen per requested path, split into shards to keep the locks short.
    let linked: Vec<Mutex<HashSet<(usize, u64, u64)>>> =
        (0..64).map(|_| Mutex::new(HashSet::new())).collect();
    let add = |targets: &[usize], m: &std::fs::Metadata| {
        let (n, link) = crate::platform::disk_usage(m);
        for &t in targets {
            if let Some((dev, ino)) = link
                && !crate::lock(&linked[(ino as usize ^ t) % 64]).insert((t, dev, ino))
            {
                continue;
            }
            counters[t].fetch_add(n, Ordering::Relaxed);
        }
    };

    // Roots are requested paths with no requested ancestor; the rest are found on the way.
    type Job = (PathBuf, Arc<Vec<usize>>);
    let mut jobs: Vec<Job> = vec![];
    for (i, p) in wanted.iter().enumerate() {
        if wanted.iter().any(|o| o != p && p.starts_with(o)) {
            continue;
        }
        let Ok(m) = p.symlink_metadata() else {
            continue;
        };
        add(&[i], &m);
        if m.is_dir() {
            jobs.push((p.clone(), Arc::new(vec![i])));
        }
    }

    // Workers take folders off the queue until it is empty and nobody is still listing one.
    let queue = (Mutex::new((jobs, 0usize)), Condvar::new());
    let threads = std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .min(16);
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let (dir, targets) = {
                        let mut q = crate::lock(&queue.0);
                        loop {
                            if let Some(job) = q.0.pop() {
                                q.1 += 1;
                                break job;
                            }
                            if q.1 == 0 {
                                queue.1.notify_all();
                                return;
                            }
                            q = queue.1.wait(q).unwrap();
                        }
                    };
                    let mut found = vec![];
                    for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                        let Ok(m) = entry.metadata() else { continue };
                        let path = entry.path();
                        let mut t = targets.clone();
                        if let Some(i) = index(&path) {
                            Arc::make_mut(&mut t).push(i);
                        }
                        add(&t, &m);
                        if m.is_dir() {
                            found.push((path, t));
                        }
                    }
                    let mut q = crate::lock(&queue.0);
                    q.0.extend(found);
                    q.1 -= 1;
                    queue.1.notify_all();
                }
            });
        }
    });
    wanted
        .into_iter()
        .zip(counters)
        .map(|(p, c)| (p, c.into_inner()))
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

/// One row of `ps`.
pub struct Ps {
    pid: u32,
    ppid: u32,
    pgid: u32,
    rss: u64,
    cpu: f32,
    command: String,
}

#[cfg(windows)]
pub fn ps() -> Vec<Ps> {
    let table = crate::platform::process_table();
    let parent: std::collections::HashMap<u32, u32> = table.iter().map(|p| (p.0, p.1)).collect();
    // No process groups: a process's "group" is the ancestor Anvil started (its parent is
    // this process), else its oldest known ancestor. A pid can be reused by an unrelated
    // process, so the walk is bounded.
    let me = std::process::id();
    let group = |mut pid: u32| {
        for _ in 0..64 {
            match parent.get(&pid) {
                Some(&p) if p != me && p != 0 && p != pid && parent.contains_key(&p) => pid = p,
                _ => break,
            }
        }
        pid
    };
    table
        .iter()
        .map(|(pid, ppid, rss, cpu, name)| Ps {
            pid: *pid,
            ppid: *ppid,
            pgid: group(*pid),
            rss: *rss,
            cpu: *cpu,
            command: name.clone(),
        })
        .collect()
}

#[cfg(not(windows))]
pub fn ps() -> Vec<Ps> {
    table(&["-A"])
}

/// `ps` of the process groups `groups`, the pids `pids` and Anvil's own group (where the
/// web server's and services' children are), not of every process: what the Processes
/// window measures. Nothing to measure, nothing run.
#[cfg(not(windows))]
pub fn ps_of(groups: &[u32], pids: &[u32]) -> Vec<Ps> {
    if groups.is_empty() && pids.is_empty() {
        return vec![];
    }
    let list = |ids: &mut dyn Iterator<Item = u32>| {
        ids.map(|i| i.to_string()).collect::<Vec<_>>().join(",")
    };
    let own = nix::unistd::getpgrp().as_raw() as u32;
    let groups = list(&mut groups.iter().copied().chain([own]));
    let pids = list(&mut pids.iter().copied());
    let mut select = vec!["-g", &groups];
    if !pids.is_empty() {
        select.extend(["-p", &pids]);
    }
    table(&select)
}

/// The whole table on Windows, read without running anything.
#[cfg(windows)]
pub fn ps_of(_groups: &[u32], _pids: &[u32]) -> Vec<Ps> {
    ps()
}

#[cfg(not(windows))]
fn table(select: &[&str]) -> Vec<Ps> {
    let out = crate::platform::command("ps")
        .args(select)
        .args(["-o", "pid=,ppid=,pgid=,rss=,%cpu=,command="])
        // A Turkish terminal's "0,5" isn't a number to `parse`; every row would be dropped.
        .env("LC_ALL", "C")
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

/// CPU % and RSS bytes of a process group (`group`), or of a pid and its children.
pub fn usage(table: &[Ps], pid: u32, group: bool) -> (f32, u64) {
    table
        .iter()
        .filter(|p| {
            if group {
                p.pgid == pid
            } else {
                p.pid == pid || p.ppid == pid
            }
        })
        .fold((0.0, 0), |(c, m), p| (c + p.cpu, m + p.rss))
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
        let own = crate::own_pool(site);
        let pool = if own {
            format!("pool {}", crate::pool(site))
        } else {
            "pool www".to_string()
        };
        let (memory, cpu, n) =
            sum(&|p| Some(p.ppid) == master && p.command.trim_end().ends_with(&pool));
        out.push(if !own {
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

/// The Anvil Mailpit an app's .env sends mail to: SMTP to this computer on its port.
pub fn mailpit_for(cfg: &Config, dir: &Path) -> Option<String> {
    let env = dotenv(dir);
    if env.get("MAIL_MAILER").map(String::as_str) != Some("smtp")
        || !is_local(env.get("MAIL_HOST").map(String::as_str))
    {
        return None;
    }
    let port: u16 = env.get("MAIL_PORT")?.parse().ok()?;
    services::instances(cfg).into_iter().find(|id| {
        services::kind_of(cfg, id).is_ok_and(|k| k == "mailpit") && services::port(cfg, id) == port
    })
}

/// Read `KEY=value` pairs from the project's .env.
pub(crate) fn dotenv(dir: &Path) -> BTreeMap<String, String> {
    dotenv_file(&dir.join(".env"))
}

/// `KEY=value` pairs of one env file; `export KEY=value` lines count too.
pub(crate) fn dotenv_file(file: &Path) -> BTreeMap<String, String> {
    std::fs::read_to_string(file)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| {
            let (k, v) = l.split_once('=')?;
            let k = k.trim();
            Some((
                k.strip_prefix("export ").unwrap_or(k).trim().to_string(),
                v.trim().trim_matches(['"', '\'']).to_string(),
            ))
        })
        .collect()
}

/// The `host:port` in a URL like `postgres://user:pw@127.0.0.1:5433/db`.
fn authority(url: &str) -> Option<&str> {
    crate::validate::authority(url)?.rsplit('@').next()
}

pub(crate) fn url_port(url: &str) -> Option<u16> {
    authority(url)?.rsplit_once(':')?.1.parse().ok()
}

pub(crate) fn url_host(url: &str) -> Option<String> {
    let a = authority(url)?;
    let host = match a.rsplit_once(':') {
        Some((h, p)) if p.parse::<u16>().is_ok() => h,
        _ => a,
    };
    Some(host.trim_matches(['[', ']']).to_string())
}

pub(crate) fn is_local(host: Option<&str>) -> bool {
    matches!(
        host,
        None | Some("" | "127.0.0.1" | "localhost" | "::1" | "0.0.0.0")
    )
}

/// Port -> name of the process listening on it, from `lsof -Fpcn` output.
fn parse_lsof(out: &str) -> BTreeMap<u16, String> {
    let mut map = BTreeMap::new();
    let mut command = "";
    for l in out.lines() {
        if let Some(c) = l.strip_prefix('c') {
            command = c;
        } else if let Some(n) = l.strip_prefix('n')
            && let Some(port) = n.rsplit(':').next().and_then(|p| p.parse().ok())
        {
            map.entry(port).or_insert_with(|| command.to_string());
        }
    }
    map
}

/// Host port -> container name, from `docker ps --format '{{.Names}}\t{{.Ports}}'`.
fn parse_docker_ps(out: &str) -> BTreeMap<u16, String> {
    let mut map = BTreeMap::new();
    for l in out.lines() {
        let Some((name, ports)) = l.split_once('\t') else {
            continue;
        };
        for p in ports.split(", ") {
            if let Some(port) = p
                .split_once("->")
                .and_then(|(h, _)| h.rsplit(':').next()?.parse().ok())
            {
                map.entry(port).or_insert_with(|| name.to_string());
            }
        }
    }
    map
}

fn is_docker(process: &str) -> bool {
    let p = process.to_lowercase();
    [
        "com.docke",
        "docker",
        "vpnkit",
        "orbstack",
        "limactl",
        "colima",
    ]
    .iter()
    .any(|d| p.starts_with(d))
}

/// Who listens on these ports, with one `lsof` call.
fn listeners(ports: &[u16]) -> BTreeMap<u16, String> {
    if ports.is_empty() {
        return BTreeMap::new();
    }
    let list: Vec<String> = ports.iter().map(u16::to_string).collect();
    let out = crate::platform::command("lsof")
        .args(["-nP", "-sTCP:LISTEN", "-Fpcn"])
        .arg(format!("-iTCP:{}", list.join(",")))
        .output();
    out.map(|o| parse_lsof(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default()
}

/// Container names by published port; empty if Docker isn't there or doesn't answer in 3s.
fn containers() -> BTreeMap<u16, String> {
    use crate::platform::OwnGroup;
    let child = crate::platform::command(crate::compose::docker_bin())
        .args(["ps", "--format", "{{.Names}}\t{{.Ports}}"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .own_group() // a timeout ends whatever docker started too
        .spawn();
    let Ok(mut child) = child else {
        return BTreeMap::new();
    };
    for _ in 0..60 {
        if let Ok(Some(_)) = child.try_wait() {
            let out = child
                .stdout
                .take()
                .map(crate::read_lossy)
                .unwrap_or_default();
            return parse_docker_ps(&out);
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    crate::terminate(&mut child, std::time::Duration::ZERO); // the whole group, reaped
    BTreeMap::new()
}

/// Whether a Compose file in the project defines this service.
fn compose_service(dir: &Path, name: &str) -> bool {
    let key = format!("{name}:");
    [
        "docker-compose.yml",
        "docker-compose.yaml",
        "compose.yaml",
        "compose.yml",
    ]
    .iter()
    .filter_map(|f| std::fs::read_to_string(dir.join(f)).ok())
    .any(|s| s.lines().any(|l| l.trim() == key))
}

/// Services the app's .env points at, matched to Anvil services by port.
/// `live` is the running Anvil services and their pids (`Services::pids`), taken
/// beforehand so no lock is held while this runs lsof, docker and du.
pub fn services_used(cfg: &Config, site: &Site, live: &BTreeMap<String, u32>) -> Vec<ServiceUse> {
    let env = dotenv(&site.path);
    let get = |k: &str| env.get(k).map(String::as_str);
    let host = |k: &str| get(k).map(String::from);
    let mut wanted: Vec<(String, Option<u16>, Option<String>)> = vec![];
    match get("DB_CONNECTION") {
        Some("mysql" | "mariadb") => wanted.push((
            "MySQL".into(),
            get("DB_PORT").and_then(|p| p.parse().ok()).or(Some(3306)),
            host("DB_HOST"),
        )),
        Some("pgsql") => wanted.push((
            "PostgreSQL".into(),
            get("DB_PORT").and_then(|p| p.parse().ok()).or(Some(5432)),
            host("DB_HOST"),
        )),
        Some("sqlite") => wanted.push(("SQLite (file in the project)".into(), None, None)),
        Some(other) => wanted.push((
            format!("Database ({other})"),
            get("DB_PORT").and_then(|p| p.parse().ok()),
            host("DB_HOST"),
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
        wanted.push((label.into(), url_port(url), url_host(url)));
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
        let h = host("REDIS_HOST").or_else(|| get("REDIS_URL").and_then(url_host));
        wanted.push(("Redis".into(), port, h));
    }
    if matches!(get("MAIL_MAILER"), Some("smtp")) {
        wanted.push((
            "Mail (SMTP)".into(),
            get("MAIL_PORT").and_then(|p| p.parse().ok()),
            host("MAIL_HOST"),
        ));
    }
    if let Some(url) = get("AWS_ENDPOINT") {
        wanted.push(("S3 storage".into(), url_port(url), url_host(url)));
    }

    let local_ports: Vec<u16> = wanted
        .iter()
        .filter(|w| is_local(w.2.as_deref()))
        .filter_map(|w| w.1)
        .collect();
    let listening = listeners(&local_ports);
    let docker = if listening.values().any(|p| is_docker(p)) {
        containers()
    } else {
        BTreeMap::new()
    };
    let table = ps();
    let ids = services::instances(cfg);
    let data_sizes = du(&ids
        .iter()
        .map(|id| home().join("data").join(id))
        .collect::<Vec<_>>());
    let mut found = wanted
        .into_iter()
        .map(|(label, port, host)| {
            let anvil = port.and_then(|p| {
                ids.iter()
                    .find(|id| {
                        services::kind_of(cfg, id)
                            .is_ok_and(|k| services::installed(k, &services::version(cfg, id)))
                            && services::port(cfg, id) == p
                    })
                    .cloned()
            });
            let proc = anvil
                .as_deref()
                .and_then(|id| live.get(id).copied())
                .and_then(|pid| table.iter().find(|p| p.pid == pid));
            let running = anvil.as_deref().is_some_and(|id| live.contains_key(id));
            let listener = port.and_then(|p| listening.get(&p));
            let (source, detail) = match (&host, port) {
                (Some(h), _) if !is_local(Some(h)) => {
                    if compose_service(&site.path, h) {
                        (Source::Docker, Some(format!("compose service {h}")))
                    } else {
                        let at = port.map_or(h.clone(), |p| format!("{h}:{p}"));
                        (Source::Remote, Some(at))
                    }
                }
                // A stopped Anvil service whose port another program holds isn't what the app reaches.
                _ if anvil.is_some() && (running || listener.is_none()) => {
                    (Source::Anvil, anvil.clone())
                }
                (_, Some(p)) => match listener {
                    Some(l) if is_docker(l) => (Source::Docker, docker.get(&p).cloned()),
                    Some(l) => (Source::Local, Some(l.clone())),
                    None => (Source::None, None),
                },
                _ => (Source::None, None),
            };
            ServiceUse {
                memory: proc.map(|p| p.rss),
                cpu: proc.map(|p| p.cpu),
                running,
                host,
                source,
                detail,
                data_bytes: anvil
                    .as_deref()
                    .and_then(|id| data_sizes.get(&home().join("data").join(id)).copied()),
                label,
                port,
                anvil,
            }
        })
        .collect::<Vec<_>>();
    // Services attached to this app in Anvil that its .env doesn't mention (yet).
    for id in ids {
        let attached = cfg.services[&id].apps.contains(&site.name);
        if !attached
            || found
                .iter()
                .any(|u| u.anvil.as_deref() == Some(id.as_str()))
        {
            continue;
        }
        let proc = live
            .get(&id)
            .and_then(|pid| table.iter().find(|p| p.pid == *pid));
        found.push(ServiceUse {
            label: services::name(services::kind_of(cfg, &id).unwrap_or("redis")).into(),
            port: Some(services::port(cfg, &id)),
            host: None,
            source: Source::Anvil,
            detail: Some(id.clone()),
            running: live.contains_key(&id),
            memory: proc.map(|p| p.rss),
            cpu: proc.map(|p| p.cpu),
            data_bytes: data_sizes.get(&home().join("data").join(&id)).copied(),
            anvil: Some(id),
        });
    }
    found
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
            "Remove versions in Settings › PHP",
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
            services::name(id).to_string(),
            h.join("services").join(id),
            "Services",
            Safety::Keep,
            "The program, shared by every service of this kind",
        ));
    }
    // Every instance's data, including ones removed from Anvil (kept on purpose).
    for id in dirs(h.join("data")).into_iter().filter(|d| d != "caddy") {
        entries.push((
            format!("{id} data"),
            h.join("data").join(&id),
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
        "Build tools".into(),
        crate::phpext::tools_dir(),
        "Cache",
        Safety::Safe,
        "Built again when a PHP extension needs them",
    ));
    entries.push((
        "Octane servers".into(),
        h.join("octane"),
        "Cache",
        Safety::Safe,
        "Downloaded again when an app's Octane starts",
    ));
    entries.push((
        "Tunnel (cloudflared)".into(),
        h.join("tunnel"),
        "Cache",
        Safety::Safe,
        "Downloaded again when you share an app",
    ));
    entries.push((
        "Database viewer (Adminer)".into(),
        crate::dbviewer::root(),
        "Cache",
        Safety::Safe,
        "Downloaded again when the Database viewer opens or Anvil starts",
    ));
    entries.push((
        "Logs".into(),
        h.join("log"),
        "Logs",
        Safety::Safe,
        "Only history; new lines keep coming",
    ));
    entries.push((
        "Caught mail".into(),
        crate::mail::root(),
        "Logs",
        Safety::Safe,
        "The mail catcher's inboxes; each keeps its newest 500 or 50 MB",
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
/// Anvil's cache folders as they exist on disk; only these exact paths can be
/// cleaned, so `cache/..` or a symlink trick never matches.
fn cache_dirs(h: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(h.join("cache"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| h.join("cache").join(e.file_name()))
        .collect()
}

pub fn clean_anvil(path: &str) -> Result<u64> {
    let h = home();
    let p = PathBuf::from(path);
    let is_cache = [
        h.join("build"),
        h.join("octane"),
        h.join("tunnel"),
        crate::dbviewer::root(),
        crate::mail::root(),
        crate::phpext::tools_dir(),
    ]
    .contains(&p)
        || cache_dirs(&h).contains(&p);
    if p != h.join("log") && !is_cache {
        bail!("Anvil doesn't clean {path}");
    }
    // One of those folders replaced by a symlink is not followed out of Anvil's home.
    if std::fs::symlink_metadata(&p).is_ok() && crate::validate::path_in(&h, &p).is_err() {
        bail!("{path} leads outside Anvil's folder; Anvil won't clean it");
    }
    let freed: u64 = du(std::slice::from_ref(&p)).values().sum();
    if p == h.join("log") {
        for e in std::fs::read_dir(&p)?.flatten() {
            // file_type doesn't follow links: a link here is not emptied through.
            if e.file_type().is_ok_and(|t| t.is_file()) {
                std::fs::write(e.path(), "")?;
            }
        }
        // Apps' request logs: Caddy writes on in the current one (emptied); the copies
        // it rolled are only history.
        for e in std::fs::read_dir(p.join("requests"))
            .into_iter()
            .flatten()
            .flatten()
        {
            if !e.file_type().is_ok_and(|t| t.is_file()) {
                continue;
            }
            if e.file_name().to_string_lossy().ends_with(".log") {
                std::fs::write(e.path(), "")?;
            } else {
                std::fs::remove_file(e.path())?;
            }
        }
    } else {
        remove(&p)?;
    }
    Ok(freed)
}

#[derive(Serialize, serde::Deserialize, Clone)]
pub struct AppSize {
    pub name: String,
    pub kind: String,
    pub total: u64,
    pub safe: u64,
    pub rebuild: u64,
}

/// One entry per project folder: extra linked domains share their app's folder.
fn unique_apps(sites: &[Site]) -> Vec<&Site> {
    let mut seen = std::collections::BTreeSet::new();
    sites
        .iter()
        .filter(|s| !s.linked || !sites.iter().any(|o| !o.linked && o.path == s.path))
        .filter(|s| seen.insert(s.path.clone()))
        .collect()
}

fn app_size(s: &Site) -> Option<AppSize> {
    let rules = rules_for(s);
    let mut paths = vec![s.path.clone()];
    paths.extend(rules.iter().map(|r| s.path.join(r.0)));
    let sizes = du(&paths);
    let sum = |want: Safety| -> u64 {
        rules
            .iter()
            .filter(|r| r.3 == want)
            .filter_map(|r| sizes.get(&s.path.join(r.0)))
            .sum()
    };
    Some(AppSize {
        name: s.name.clone(),
        kind: s.kind.clone(),
        total: *sizes.get(&s.path)?,
        safe: sum(Safety::Safe),
        rebuild: sum(Safety::Rebuild),
    })
}

/// Sizes of every app, biggest first. `each` hears about every app as soon as
/// it is measured, so a list can fill in while the rest are still counted.
pub fn all_apps(sites: &[Site], mut each: impl FnMut(&AppSize)) -> Vec<AppSize> {
    let mut out: Vec<AppSize> = unique_apps(sites)
        .into_iter()
        .filter_map(app_size)
        .inspect(|a| each(a))
        .collect();
    out.sort_by_key(|a| std::cmp::Reverse(a.total));
    out
}

/// Keep a measurement so the next look starts from it.
pub fn save_app_sizes(sizes: &[AppSize]) {
    let _ = std::fs::create_dir_all(home().join("cache"));
    let _ = serde_json::to_vec(sizes).map(|j| std::fs::write(sizes_file(), j));
}

fn sizes_file() -> PathBuf {
    home().join("cache").join("app-sizes.json")
}

/// The last measurement and when it was taken (Unix seconds), if any.
pub fn last_app_sizes() -> Option<(u64, Vec<AppSize>)> {
    let f = sizes_file();
    let when = std::fs::metadata(&f)
        .ok()?
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some((when, serde_json::from_slice(&std::fs::read(f).ok()?).ok()?))
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
    let inside = Path::new(path)
        .parent()
        .is_some_and(|p| crate::validate::path_in(&site.path, p).is_ok());
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

/// The newest file change under `dir` in unix seconds: when the code was last
/// edited. Dependency, build and hidden folders are skipped. It stops after 5000
/// entries or 300 ms and returns the newest seen so far.
pub fn touched(dir: &Path) -> Option<u64> {
    const SKIP: [&str; 9] = [
        "node_modules",
        "vendor",
        ".git",
        "storage",
        "dist",
        "build",
        "target",
        ".next",
        ".nuxt",
    ];
    let start = std::time::Instant::now();
    let mut seen = 0;
    let mut newest = None;
    let mut dirs = vec![dir.to_path_buf()];
    while let Some(d) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            seen += 1;
            if seen > 5000 || start.elapsed() > std::time::Duration::from_millis(300) {
                return newest;
            }
            let Ok(ty) = e.file_type() else { continue };
            if ty.is_dir() {
                let name = e.file_name();
                let name = name.to_string_lossy();
                if !name.starts_with('.') && !SKIP.contains(&&*name) {
                    dirs.push(e.path());
                }
            } else if let Some(t) = e
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            {
                newest = newest.max(Some(t.as_secs()));
            }
        }
    }
    newest
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn ps_of_reads_only_the_groups_and_pids_asked_for() {
        use std::os::unix::process::CommandExt;
        assert!(ps_of(&[], &[]).is_empty());
        let mut leader = crate::platform::command("sh")
            .args(["-c", "sleep 30 & wait"])
            .process_group(0)
            .spawn()
            .unwrap();
        let g = leader.id();
        let mut single = crate::platform::command("sleep").arg("30").spawn().unwrap();
        // Its child, in its group, may take a moment to start.
        let mut table = vec![];
        for _ in 0..100 {
            table = ps_of(&[g], &[single.id()]);
            if table.iter().filter(|p| p.pgid == g).count() == 2 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let _ = leader.kill();
        let _ = single.kill();
        let _ = (leader.wait(), single.wait());
        assert_eq!(table.iter().filter(|p| p.pgid == g).count(), 2);
        assert!(table.iter().any(|p| p.pid == single.id() && p.rss > 0));
        // Not every process: launchd/init (pid 1) is in none of them.
        assert!(!table.iter().any(|p| p.pid == 1));
        let (_, memory) = usage(&table, g, true);
        assert!(memory > 0);
    }

    #[test]
    fn mail_goes_to_the_mailpit_on_its_smtp_port() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        for (id, kind, port) in [("redis", "redis", 1025), ("mail", "mailpit", 1026)] {
            cfg.services.insert(
                id.into(),
                services::ServiceConfig {
                    kind: Some(kind.into()),
                    port: Some(port),
                    ..Default::default()
                },
            );
        }
        let to = |env: &str| {
            std::fs::write(dir.path().join(".env"), env).unwrap();
            mailpit_for(&cfg, dir.path())
        };
        let smtp = "MAIL_MAILER=smtp\nMAIL_HOST=127.0.0.1\nMAIL_PORT=";
        assert_eq!(to(&format!("{smtp}1026")).as_deref(), Some("mail"));
        assert_eq!(
            to("MAIL_MAILER=smtp\nMAIL_PORT=1026").as_deref(),
            Some("mail")
        );
        // Another kind on that port, another host, another mailer, no port: not Mailpit.
        assert_eq!(to(&format!("{smtp}1025")), None);
        assert_eq!(
            to("MAIL_MAILER=smtp\nMAIL_HOST=mailpit\nMAIL_PORT=1026"),
            None
        );
        assert_eq!(to("MAIL_MAILER=log\nMAIL_PORT=1026"), None);
        assert_eq!(to("MAIL_MAILER=smtp"), None);
    }

    #[test]
    fn build_tools_are_cleanable() {
        let dir = tempfile::tempdir().unwrap();
        let _env = crate::lock(&crate::ENV_LOCK);
        // SAFETY: every test that sets ANVIL_HOME holds ENV_LOCK.
        unsafe { std::env::set_var("ANVIL_HOME", dir.path()) };
        let tools = crate::phpext::tools_dir();
        std::fs::create_dir_all(tools.join("bin")).unwrap();
        std::fs::write(tools.join("bin/m4"), vec![1u8; 5_000]).unwrap();
        let rr = dir.path().join("octane/roadrunner/2025.1.15");
        std::fs::create_dir_all(&rr).unwrap();
        std::fs::write(rr.join("rr"), vec![1u8; 5_000]).unwrap();
        let cf = dir.path().join("tunnel/2026.10.2");
        std::fs::create_dir_all(&cf).unwrap();
        std::fs::write(cf.join("cloudflared"), vec![1u8; 5_000]).unwrap();
        let inbox = dir.path().join("mail/shop");
        std::fs::create_dir_all(&inbox).unwrap();
        std::fs::write(inbox.join("1-0.eml"), vec![1u8; 5_000]).unwrap();
        let adminer = dir.path().join("adminer/6.1.2");
        std::fs::create_dir_all(&adminer).unwrap();
        std::fs::write(adminer.join("adminer.php"), vec![1u8; 5_000]).unwrap();
        let (_, items) = anvil_disk();
        for (label, gone) in [
            ("Build tools", &tools),
            ("Octane servers", &rr),
            ("Tunnel (cloudflared)", &cf),
            ("Database viewer (Adminer)", &adminer),
            ("Caught mail", &inbox),
        ] {
            let item = items.iter().find(|i| i.label == label).unwrap();
            assert!(item.safety == Safety::Safe);
            assert!(clean_anvil(&item.path).unwrap() > 0);
            assert!(!gone.exists());
        }
    }

    #[test]
    fn cleaning_logs_empties_request_logs_and_drops_their_rolled_copies() {
        let dir = tempfile::tempdir().unwrap();
        let _env = crate::lock(&crate::ENV_LOCK);
        // SAFETY: every test that sets ANVIL_HOME holds ENV_LOCK.
        unsafe { std::env::set_var("ANVIL_HOME", dir.path()) };
        let current = crate::requests::file("shop");
        let rolled = current.with_file_name("shop-2026-10-20T13-20-36.452-size.log.gz");
        std::fs::create_dir_all(current.parent().unwrap()).unwrap();
        std::fs::write(&current, vec![b'x'; 5_000]).unwrap();
        std::fs::write(&rolled, vec![1u8; 5_000]).unwrap();
        let (_, items) = anvil_disk();
        let logs = items.iter().find(|i| i.label == "Logs").unwrap();
        assert!(logs.bytes >= 10_000);
        assert!(clean_anvil(&logs.path).unwrap() >= 10_000);
        // Caddy keeps the current one open: emptied, not deleted.
        assert_eq!(std::fs::metadata(&current).unwrap().len(), 0);
        assert!(!rolled.exists());
    }

    #[test]
    fn touched_skips_dependencies_and_hidden_folders() {
        let dir = std::env::temp_dir().join(format!("anvil-touched-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let now = std::time::SystemTime::now();
        let old = now - std::time::Duration::from_secs(3600);
        for (file, at) in [
            ("src/app.rs", old),
            ("node_modules/x/index.js", now),
            (".idea/workspace.xml", now),
            ("vendor/a.php", now),
        ] {
            let path = dir.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::File::create(&path)
                .unwrap()
                .set_modified(at)
                .unwrap();
        }
        let secs =
            |t: std::time::SystemTime| t.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        assert_eq!(touched(&dir), Some(secs(old)));
        assert_eq!(touched(&dir.join("missing")), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn usage_sums_a_group() {
        let p = |pid, ppid, pgid, rss, cpu| Ps {
            pid,
            ppid,
            pgid,
            rss,
            cpu,
            command: String::new(),
        };
        let mb = 1024 * 1024;
        let table = [
            p(10, 1, 10, 100 * mb, 30.0),
            p(11, 10, 10, 20 * mb, 5.0),
            p(99, 1, 99, 500 * mb, 50.0),
        ];
        assert_eq!(usage(&table, 10, true), (35.0, 120 * mb));
        assert_eq!(usage(&table, 10, false), (35.0, 120 * mb));
        assert_eq!(usage(&table, 99, false), (50.0, 500 * mb));
    }

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
            host: String::new(),
            secure: false,
            path: dir.clone(),
            root: dir.clone(),
            root_custom: None,
            root_detected: ".".into(),
            php: None,
            php_pinned: false,
            ext: vec![],
            own_master: false,
            octane_ports: None,
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
            worktree_of: None,
            branch: None,
            aliases: vec![],
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
            host: String::new(),
            secure: false,
            name: "n".into(),
            path: project.clone(),
            root: project.clone(),
            root_custom: None,
            root_detected: ".".into(),
            php: None,
            php_pinned: false,
            ext: vec![],
            own_master: false,
            octane_ports: None,
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
            worktree_of: None,
            branch: None,
            aliases: vec![],
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
        // `..` after the cache folder used to pass the parent check and resolve to ~/.anvil.
        let up = format!("{}/cache/..", home().display());
        assert!(clean_anvil(&up).is_err());
        assert!(clean_anvil(&format!("{}/cache/../data", home().display())).is_err());
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
            host: String::new(),
            secure: false,
            name: name.into(),
            path: dir.clone(),
            root: dir,
            root_custom: None,
            root_detected: ".".into(),
            php: None,
            php_pinned: false,
            ext: vec![],
            own_master: false,
            octane_ports: None,
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
            worktree_of: None,
            branch: None,
            aliases: vec![],
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
        assert!(all_apps(std::slice::from_ref(&site), |_| {})[0].safe > 0);
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

    #[test]
    fn service_sources() {
        assert_eq!(
            url_host("postgres://u:p@db.example.com:5432/app").as_deref(),
            Some("db.example.com")
        );
        assert_eq!(url_host("redis://127.0.0.1").as_deref(), Some("127.0.0.1"));
        assert_eq!(url_host("redis://[::1]:6380?x=1").as_deref(), Some("::1"));
        assert_eq!(url_port("redis://[::1]:6380?x=1"), Some(6380));
        assert!(is_local(None) && is_local(Some("localhost")) && is_local(Some("::1")));
        assert!(!is_local(Some("mysql")) && !is_local(Some("db.example.com")));

        let lsof = "p4437\ncmariadbd\nf643\nn*:3306\nf644\nn*:3306\n\
                    p9919\nccom.docker.backend\nf629\nn127.0.0.1:6379\nf631\nn127.0.0.1:3307\n";
        let l = parse_lsof(lsof);
        assert_eq!(l.get(&3306).map(String::as_str), Some("mariadbd"));
        assert_eq!(l.get(&3307).map(String::as_str), Some("com.docker.backend"));
        assert!(is_docker("com.docke") && is_docker("OrbStack") && !is_docker("mariadbd"));

        let ps = "shop-db\t0.0.0.0:3307->3306/tcp, [::]:3307->3306/tcp\n\
                  cache\t127.0.0.1:6379->6379/tcp\nworker\t\nother\t6380/tcp\n";
        let d = parse_docker_ps(ps);
        assert_eq!(d.get(&3307).map(String::as_str), Some("shop-db"));
        assert_eq!(d.get(&6379).map(String::as_str), Some("cache"));
        assert_eq!(d.len(), 2);
    }

    #[test]
    fn du_matches_the_system_du() {
        let root = std::env::temp_dir().join(format!("anvil-du-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let deep = root.join("node_modules/a/b");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(deep.join("big"), vec![1u8; 300_000]).unwrap();
        std::fs::write(root.join("src/main.rs"), vec![2u8; 5_000]).unwrap();
        // A hard link counts once, a symlink isn't followed.
        std::fs::hard_link(deep.join("big"), root.join("src/big-link")).unwrap();
        std::os::unix::fs::symlink("/usr", root.join("usr-link")).unwrap();

        let paths = [
            root.clone(),
            root.join("node_modules"),
            root.join("node_modules/a/b"),
        ];
        let ours = du(&paths);
        let out = crate::platform::command("du")
            .arg("-sk")
            .args(&paths)
            .output()
            .unwrap();
        // The system du counts a hard link once per run, so ask it about each path alone.
        for p in &paths {
            let out = crate::platform::command("du")
                .arg("-sk")
                .arg(p)
                .output()
                .unwrap();
            let kb: u64 = String::from_utf8_lossy(&out.stdout)
                .split_whitespace()
                .next()
                .unwrap()
                .parse()
                .unwrap();
            assert_eq!(ours[p], kb * 1024, "{}", p.display());
        }
        assert!(out.status.success());
        std::fs::remove_dir_all(root).unwrap();
    }
}
