//! Parallel workspaces: git worktrees of an app, each served on its own address.

/// `<first>--<rest>`: the address label of an app's extra server or worktree.
pub fn joined(first: &str, rest: &str) -> String {
    format!("{first}--{rest}")
}

/// A worktree's own part of its name: the folder slugged like site names, less a
/// leading `<app>--` (an Anvil-made `myapp--feature-x` is `feature-x`).
pub fn worktree_name(app: &str, folder: &Path) -> String {
    let name = crate::site_name(&folder.file_name().unwrap_or_default().to_string_lossy());
    match name.strip_prefix(&format!("{app}--")) {
        Some(rest) if !rest.is_empty() => rest.into(),
        _ => name,
    }
}

use crate::{Config, Site};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

/// Which database a worktree uses.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum DbChoice {
    /// Its own database, a copy of the main app's.
    #[default]
    Copy,
    /// Its own database, empty (filled by migrations).
    Empty,
    /// The main app's database.
    Share,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct WorktreeConfig {
    pub db: DbChoice,
    /// Copies Anvil made for this worktree: (service, database).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub copies: Vec<(String, String)>,
}

/// A worktree's own database name: `<app>_<name>` with `_` for anything else, at most 63 bytes.
pub fn db_name(site: &Site) -> String {
    let rest = site
        .worktree_of
        .as_ref()
        .and_then(|main| Some((main, site.name.strip_prefix(&format!("{main}--"))?)));
    let raw = match rest {
        Some((main, rest)) => format!("{main}_{rest}"),
        None => site.name.clone(),
    };
    let mut name: String = raw
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    // Postgres' limit is 63 bytes (MySQL's 64): a longer one keeps its start and gets a
    // hash of the whole, so two long names that start alike still differ.
    if name.len() > 63 {
        let hash = crate::short_hash(&name);
        name.truncate(63 - 1 - hash.len());
        name = format!("{name}_{hash}");
    }
    name
}

/// Where the main app keeps its data, as far as a worktree's own copy goes.
#[derive(Debug, PartialEq)]
pub enum MainDb {
    /// No database settings, or none Anvil can copy (`:memory:`, a SQLite URL).
    None,
    /// A SQLite file, and where the worktree's copy goes (the file `env` points it at).
    Sqlite { from: PathBuf, to: PathBuf },
    /// A database in an Anvil MySQL or PostgreSQL service.
    Anvil { service: String, database: String },
    /// A server Anvil doesn't manage: it can't be copied.
    External,
}

impl MainDb {
    /// What a worktree can choose: an external database can only be shared or made empty.
    pub fn choices(&self) -> &'static [DbChoice] {
        match self {
            MainDb::External => &[DbChoice::Share, DbChoice::Empty],
            _ => &[DbChoice::Copy, DbChoice::Empty, DbChoice::Share],
        }
    }
}

/// Laravel's SQLite file when `DB_DATABASE` isn't set.
const DEFAULT_SQLITE: &str = "database/database.sqlite";

/// The main app's database for the worktree `site`, read from main's `.env`.
pub fn main_db(cfg: &Config, site: &Site) -> MainDb {
    use crate::resources::{is_local, url_host, url_port};
    let Some(main) = main_of(&site.path) else {
        return MainDb::None;
    };
    let env = crate::resources::dotenv(&main);
    let get = |k: &str| env.get(k).map(String::as_str).filter(|v| !v.is_empty());
    let conn = get("DB_CONNECTION");
    if conn == Some("sqlite") {
        let (from, to) = match get("DB_DATABASE") {
            Some(":memory:") => return MainDb::None,
            Some(f) => match sqlite_file(&main, &site.path, f) {
                Some(to) => (PathBuf::from(f), PathBuf::from(to)),
                None => (main.join(f), site.path.join(f)),
            },
            None => (main.join(DEFAULT_SQLITE), site.path.join(DEFAULT_SQLITE)),
        };
        return MainDb::Sqlite { from, to };
    }
    let kind = |scheme: &str| match scheme {
        "postgres" | "postgresql" | "pgsql" => Some(("postgres", 5432)),
        "mysql" | "mysql2" | "mariadb" => Some(("mysql", 3306)),
        _ => None,
    };
    let (kind, host, port, database) = match get("DB_URL").or(get("DATABASE_URL")) {
        Some(url) => {
            let Some((scheme, rest)) = url.split_once("://") else {
                return MainDb::None; // `file:`, SQLite through a URL
            };
            let Some((kind, default)) = kind(scheme) else {
                return MainDb::External;
            };
            let path = rest[..rest.find(['?', '#']).unwrap_or(rest.len())]
                .split_once('/')
                .map(|(_, db)| db.to_string());
            let port = url_port(url).unwrap_or(default);
            (kind, url_host(url), port, path)
        }
        None => {
            let Some(c) = conn else {
                return MainDb::None;
            };
            let Some((kind, default)) = kind(c) else {
                return MainDb::External;
            };
            let port = get("DB_PORT")
                .and_then(|p| p.parse().ok())
                .unwrap_or(default);
            // Laravel's own default when DB_DATABASE isn't set.
            let db = get("DB_DATABASE").unwrap_or("laravel");
            (
                kind,
                get("DB_HOST").map(String::from),
                port,
                Some(db.to_string()),
            )
        }
    };
    let service = crate::services::instances(cfg).into_iter().find(|id| {
        crate::services::kind_of(cfg, id).is_ok_and(|k| k == kind)
            && crate::services::port(cfg, id) == port
    });
    match (service, database.filter(|d| !d.is_empty())) {
        (Some(service), Some(database)) if is_local(host.as_deref()) => {
            MainDb::Anvil { service, database }
        }
        _ => MainDb::External,
    }
}

/// What `prepare` asks of a database server.
pub(crate) enum Op<'a> {
    /// Whether the service answers on its port.
    Running,
    Exists(&'a str),
    Clone {
        from: &'a str,
        to: &'a str,
    },
    Create(&'a str),
    Drop(&'a str),
}

/// What's left to do for the worktree `site`: its choice and main's database, or
/// `None` when it shares, has its copy already, or there's nothing Anvil can make.
fn pending(cfg: &Config, site: &Site) -> Option<(DbChoice, MainDb)> {
    site.worktree_of.as_ref()?;
    let w = cfg.worktrees.get(&site.name).cloned().unwrap_or_default();
    if w.db == DbChoice::Share || !w.copies.is_empty() {
        return None;
    }
    match main_db(cfg, site) {
        MainDb::None | MainDb::External => None,
        MainDb::Sqlite { to, .. } if to.exists() => None,
        db => Some((w.db, db)),
    }
}

/// `prepare` with the server reached through `run(cfg, service, op)`, and `marker`
/// the file that stays while a copy is under way (an interrupted one is redone).
pub(crate) fn prepare_with(
    cfg: &mut Config,
    site: &Site,
    marker: &Path,
    say: &mut dyn FnMut(&str),
    run: &mut dyn FnMut(&Config, &str, Op) -> anyhow::Result<bool>,
) -> anyhow::Result<bool> {
    let Some((choice, db)) = pending(cfg, site) else {
        return Ok(false);
    };
    let name = db_name(site);
    match db {
        MainDb::Sqlite { from, to } => {
            if choice == DbChoice::Copy && !from.exists() {
                return Ok(false);
            }
            if let Some(dir) = to.parent() {
                fs::create_dir_all(dir)?;
            }
            let shown = to.strip_prefix(&site.path).unwrap_or(&to).display();
            let main = site.worktree_of.as_deref().unwrap_or("main");
            // Through a temporary file: nothing ever opens a half-copied database.
            let tmp = to.with_file_name(format!(
                ".{}.anvil",
                to.file_name().unwrap_or_default().to_string_lossy()
            ));
            if choice == DbChoice::Copy {
                say(&format!("Anvil: preparing {shown} (copy of {main}'s)"));
                fs::copy(&from, &tmp)?;
            } else {
                say(&format!("Anvil: preparing {shown} (empty)"));
                fs::File::create(&tmp)?;
            }
            fs::rename(&tmp, &to)?;
            // It lives in the worktree folder and goes with it: nothing to record.
            Ok(true)
        }
        MainDb::Anvil { service, database } => {
            if !run(cfg, &service, Op::Running)? {
                anyhow::bail!("Start {service} to prepare {name}");
            }
            if run(cfg, &service, Op::Exists(&name))? {
                if !marker.exists() {
                    // Made before but not recorded (or by hand): kept, and not Anvil's to drop.
                    return Ok(false);
                }
                // A copy that was cut off: half a database, made again.
                run(cfg, &service, Op::Drop(&name))?;
            }
            let how = match choice {
                DbChoice::Copy => format!("copy of {database}"),
                _ => "empty".into(),
            };
            say(&format!("Anvil: preparing {name} ({how})"));
            fs::write(marker, &name)?;
            if choice == DbChoice::Copy {
                let op = Op::Clone {
                    from: &database,
                    to: &name,
                };
                run(cfg, &service, op)?;
            } else {
                run(cfg, &service, Op::Create(&name))?;
            }
            let w = cfg.worktrees.entry(site.name.clone()).or_default();
            w.copies.push((service, name));
            fs::remove_file(marker)?;
            Ok(true)
        }
        MainDb::None | MainDb::External => Ok(false),
    }
}

fn on_server(cfg: &Config, id: &str, op: Op) -> anyhow::Result<bool> {
    use crate::databases as d;
    match op {
        Op::Running => {
            let at = std::net::SocketAddr::from(([127, 0, 0, 1], crate::services::port(cfg, id)));
            let wait = std::time::Duration::from_secs(1);
            Ok(std::net::TcpStream::connect_timeout(&at, wait).is_ok())
        }
        Op::Exists(n) => d::exists(cfg, id, n),
        Op::Clone { from, to } => d::clone_database(cfg, id, from, to).map(|()| true),
        Op::Create(n) => d::create(cfg, id, n).map(|()| true),
        Op::Drop(n) => d::drop_database(cfg, id, n).map(|()| true),
    }
}

/// `prepare` on the config as saved, for any Start, process or script of `site` (a no-op
/// for anything but a worktree): what's recorded on disk is what counts. Run it before
/// taking any lock, such as the app's on `Apps`.
pub fn prepare_site(site: &Site, say: &mut dyn FnMut(&str)) -> anyhow::Result<()> {
    if site.worktree_of.is_some() {
        prepare(&mut crate::load()?, site, say)?;
    }
    Ok(())
}

/// Make a worktree's own database on its first use (a Start, a terminal shim): a copy
/// of main's, or an empty one, as chosen. `say` gets the line to show before the work
/// starts; `true` when it made one. What it made on a server is recorded in
/// `cfg.worktrees` (and saved). Slow: hold no other lock across it.
pub fn prepare(cfg: &mut Config, site: &Site, say: &mut dyn FnMut(&str)) -> anyhow::Result<bool> {
    if pending(cfg, site).is_none() {
        return Ok(false);
    }
    let run = crate::home().join("run");
    crate::state_dirs_in(&crate::home())?;
    // One at a time per worktree: a Start and a shim racing don't copy twice.
    let lock = fs::File::create(run.join(format!("prepare-{}.lock", site.name)))?;
    lock.lock()?;
    // The other one may have made it while this one waited.
    if let Some(w) = crate::load()?.worktrees.remove(&site.name) {
        cfg.worktrees.insert(site.name.clone(), w);
    }
    let marker = run.join(format!("prepare-{}.copying", site.name));
    let made = prepare_with(cfg, site, &marker, say, &mut on_server)?;
    let copies = cfg
        .worktrees
        .get(&site.name)
        .map(|w| w.copies.clone())
        .unwrap_or_default();
    if made && !copies.is_empty() {
        let (saved, ()) = crate::update(|c| {
            let w = c.worktrees.entry(site.name.clone()).or_default();
            for copy in &copies {
                if !w.copies.contains(copy) {
                    w.copies.push(copy.clone());
                }
            }
            Ok(())
        })?;
        *cfg = saved;
    }
    Ok(made)
}

/// `url` pointing at database `db` instead: `postgres://u@h:5433/shop?x` -> `.../<db>?x`.
/// `None` for a scheme Anvil can't rewrite. A SQLite `file:` URL with an absolute path
/// is mapped like `sqlite_file`; a relative one already lives in the worktree.
fn with_db(url: &str, db: &str, main: &Path, wt: &Path) -> Option<String> {
    if let Some(rest) = url.strip_prefix("file:") {
        let (path, query) = rest.split_at(rest.find('?').unwrap_or(rest.len()));
        return Some(match sqlite_file(main, wt, path) {
            Some(file) => format!("file:{file}{query}"),
            None => url.into(),
        });
    }
    let (scheme, rest) = url.split_once("://")?;
    if scheme == "sqlserver" {
        // Prisma: `sqlserver://host:1433;database=shop;user=sa`
        let mut parts: Vec<String> = rest.split(';').map(Into::into).collect();
        let is_db = |p: &str| {
            p.split_once('=')
                .is_some_and(|(k, _)| k.trim().eq_ignore_ascii_case("database"))
        };
        match parts.iter_mut().find(|p| is_db(p)) {
            Some(p) => *p = format!("database={db}"),
            None => parts.insert(1.min(parts.len()), format!("database={db}")),
        }
        return Some(format!("sqlserver://{}", parts.join(";")));
    }
    let path_based = [
        "postgres",
        "postgresql",
        "pgsql",
        "mysql",
        "mysql2",
        "mariadb",
        "sqlsrv",
        "mssql",
    ];
    if !path_based.contains(&scheme) {
        return None;
    }
    let tail = rest.find(['?', '#']).unwrap_or(rest.len());
    let host_end = rest[..tail].find('/').unwrap_or(tail);
    Some(format!(
        "{scheme}://{}/{db}{}",
        &rest[..host_end],
        &rest[tail..]
    ))
}

/// The worktree's own copy of an absolute SQLite path: the same place inside the
/// worktree when the file is in the main checkout, else `<worktree>/database/<file>`.
/// `None` for a relative path (or `:memory:`), which already resolves in the worktree.
fn sqlite_file(main: &Path, wt: &Path, file: &str) -> Option<String> {
    let file = Path::new(file);
    if !file.is_absolute() {
        return None;
    }
    // Canonical, so a symlinked folder in `.env` still matches; the file may not exist yet.
    let real = fs::canonicalize(file).ok().or_else(|| {
        Some(
            fs::canonicalize(file.parent()?)
                .ok()?
                .join(file.file_name()?),
        )
    });
    let real = real.unwrap_or_else(|| file.to_path_buf());
    let main = fs::canonicalize(main).unwrap_or_else(|_| main.to_path_buf());
    let to = match real.strip_prefix(&main) {
        Ok(rel) => wt.join(rel),
        Err(_) => wt.join("database").join(real.file_name()?),
    };
    Some(to.display().to_string())
}

/// What a worktree's processes get on top of their usual environment, so they never
/// reach the main app's database or Redis keys. Empty for anything but a worktree.
/// Set variables win over the worktree's `.env` (Laravel, dotenv, Next.js, Prisma).
/// `Err(reason)` when Anvil can't work out the worktree's own database: then nothing
/// of the worktree runs (`checked_env`, no PHP-FPM pool) unless it shares main's.
pub fn env(cfg: &Config, site: &Site) -> Result<Vec<(String, String)>, String> {
    if site.worktree_of.is_none() {
        return Ok(Vec::new());
    }
    let db = db_name(site);
    let mut env: Vec<(String, String)> = Vec::new();
    let choice = cfg
        .worktrees
        .get(&site.name)
        .map(|w| w.db)
        .unwrap_or_default();
    let share = choice == DbChoice::Share;
    if !share {
        let main = main_of(&site.path).ok_or("its main checkout wasn't found")?;
        let dotenv = crate::resources::dotenv(&main);
        let conn = dotenv.get("DB_CONNECTION").map(String::as_str);
        if conn == Some("sqlite") {
            if let Some(file) = dotenv
                .get("DB_DATABASE")
                .and_then(|f| sqlite_file(&main, &site.path, f))
            {
                env.push(("DB_DATABASE".into(), file));
            }
        } else if conn.is_some() || dotenv.contains_key("DB_DATABASE") {
            // Laravel falls back to `laravel` without DB_DATABASE: never main's.
            env.push(("DB_DATABASE".into(), db.clone()));
        }
        for key in ["DB_URL", "DATABASE_URL"] {
            if let Some(url) = dotenv.get(key).filter(|u| !u.is_empty()) {
                let url = with_db(url, &db, &main, &site.path)
                    .ok_or_else(|| format!("{key} uses a kind of database Anvil can't rewrite"))?;
                env.push((key.into(), url));
            }
        }
    }
    let prefix = format!("{db}_");
    env.push(("APP_URL".into(), crate::site_url(cfg, site)));
    env.push(("REDIS_PREFIX".into(), prefix.clone()));
    env.push(("CACHE_PREFIX".into(), prefix));
    if !share
        && site.php.is_some()
        && let Some((k, _)) = env.iter().find(|(k, v)| !fpm_safe(k, v))
    {
        return Err(format!("{k} holds characters PHP-FPM can't take"));
    }
    Ok(env)
}

/// Whether `env[k] = "v"` reads back as `v` in a PHP-FPM pool (checked with php-fpm -tt):
/// no line breaks, `;` or empty value (`check_ini`), no `"`, `\` or `$` (`${…}` expands).
pub(crate) fn fpm_safe(k: &str, v: &str) -> bool {
    crate::check_ini(k, v).is_ok() && !v.contains(['"', '$', '\\'])
}

/// `env`, or the error that refuses to start anything of a worktree it can't separate from main.
pub fn checked_env(cfg: &Config, site: &Site) -> anyhow::Result<Vec<(String, String)>> {
    env(cfg, site).map_err(|reason| {
        anyhow::anyhow!(
            "Anvil can't point {} at its own database ({reason}); choose Share main's to use main's database",
            site.name
        )
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct Worktree {
    pub path: PathBuf,
    /// Short branch name (`feature/login`), `None` when detached.
    pub branch: Option<String>,
    pub detached: bool,
    pub prunable: bool,
}

/// Linked worktrees from `git worktree list --porcelain`; the first entry (the main checkout) is left out.
pub fn parse_porcelain(text: &str) -> Vec<Worktree> {
    text.split("\n\n")
        .skip(1)
        .filter_map(|block| {
            let mut w = Worktree {
                path: PathBuf::new(),
                branch: None,
                detached: false,
                prunable: false,
            };
            for line in block.lines() {
                if let Some(p) = line.strip_prefix("worktree ") {
                    w.path = p.into();
                } else if let Some(b) = line.strip_prefix("branch ") {
                    w.branch = Some(b.strip_prefix("refs/heads/").unwrap_or(b).into());
                } else if line == "detached" {
                    w.detached = true;
                } else if line.starts_with("prunable") {
                    w.prunable = true;
                }
            }
            (!w.path.as_os_str().is_empty()).then_some(w)
        })
        .collect()
}

type Cached = (Vec<SystemTime>, Vec<Worktree>);
static CACHE: Mutex<Option<HashMap<PathBuf, Cached>>> = Mutex::new(None);

/// Modified times of `.git/worktrees` and each entry's HEAD (a branch switch rewrites HEAD).
/// `None` when the repository has no worktrees folder.
fn stamp(main: &Path) -> Option<Vec<SystemTime>> {
    let dir = main.join(".git/worktrees");
    let mtime = |p: &Path| fs::metadata(p).and_then(|m| m.modified()).ok();
    let mut out = vec![mtime(&dir)?];
    for e in fs::read_dir(&dir).ok()?.flatten() {
        out.extend(mtime(&e.path().join("HEAD")));
    }
    Some(out)
}

/// The linked worktrees of the repository at `main`. Git only runs when the worktrees folder changed.
pub fn list(main: &Path) -> Vec<Worktree> {
    let Some(stamp) = stamp(main) else {
        return Vec::new();
    };
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let cache = cache.get_or_insert_default();
    if let Some((s, list)) = cache.get(main)
        && *s == stamp
    {
        return list.clone();
    }
    let mut list = crate::git::git(main, &["worktree", "list", "--porcelain"])
        .map(|t| parse_porcelain(&t))
        .unwrap_or_default();
    for w in &mut list {
        w.path = fs::canonicalize(&w.path).unwrap_or_else(|_| w.path.clone());
    }
    cache.insert(main.to_path_buf(), (stamp, list.clone()));
    list
}

/// The main checkout when `dir` is a linked worktree (its `.git` file points into `<main>/.git/worktrees/<name>`).
pub fn main_of(dir: &Path) -> Option<PathBuf> {
    let git_dir = crate::git::git_dir(dir)?;
    let common = fs::read_to_string(git_dir.join("commondir")).ok()?;
    let common = fs::canonicalize(git_dir.join(common.trim())).ok()?;
    common.parent().map(Path::to_path_buf)
}

/// Create a worktree of `app` for `branch` (a new branch with `new_branch`) in
/// `<parent of the app>/<app>--<site name of the branch>`, and give it the app's `.env` when it
/// has none (git ignores it, so a fresh checkout lacks it). The app's own files are not touched.
pub fn add(_cfg: &Config, app: &Site, branch: &str, new_branch: bool) -> anyhow::Result<PathBuf> {
    let Some(parent) = app.path.parent() else {
        anyhow::bail!("{} has no parent folder", app.path.display());
    };
    let folder = parent.join(joined(&app.name, &crate::site_name(branch)));
    if folder.exists() {
        anyhow::bail!("{} already exists", folder.display());
    }
    let mut cmd = std::process::Command::new("git");
    cmd.arg("-C").arg(&app.path).args(["worktree", "add"]);
    if new_branch {
        cmd.args(["-b", branch]).arg(&folder);
    } else {
        cmd.arg(&folder).arg(branch);
    }
    let out = cmd.output()?;
    if !out.status.success() {
        anyhow::bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    let (from, to) = (app.path.join(".env"), folder.join(".env"));
    if from.is_file() && !to.exists() {
        fs::copy(from, to)?;
    }
    Ok(folder)
}

/// A database Anvil copied for a worktree that is gone. Never dropped on its own.
#[derive(Debug, Clone, PartialEq)]
pub struct Leftover {
    pub app: String,
    pub worktree: String,
    pub service: String,
    pub database: String,
}

/// Recorded copies whose worktree is no longer one of `sites`.
pub fn leftovers(cfg: &Config, sites: &[Site]) -> Vec<Leftover> {
    let mut out = Vec::new();
    for (name, w) in &cfg.worktrees {
        if sites.iter().any(|s| s.name == *name) {
            continue;
        }
        let (app, worktree) = name.split_once("--").unwrap_or((name, ""));
        for (service, database) in &w.copies {
            out.push(Leftover {
                app: app.into(),
                worktree: worktree.into(),
                service: service.clone(),
                database: database.clone(),
            });
        }
    }
    out
}

/// Remove a worktree: its folder (`git worktree remove`) and, with `drop_db`, the databases
/// recorded for it. Refuses uncommitted changes and unpushed commits unless `force`. Its
/// processes are stopped by the caller first. Changes `cfg` only: the caller saves it after Ok and after Err alike (a failed
/// drop has already removed the records of the databases that were dropped).
pub fn remove(cfg: &mut Config, site: &Site, drop_db: bool, force: bool) -> anyhow::Result<()> {
    remove_with(cfg, site, drop_db, force, &mut |c, id, db| {
        crate::databases::drop_database(c, id, db)
    })
}

fn remove_with(
    cfg: &mut Config,
    site: &Site,
    drop_db: bool,
    force: bool,
    drop: &mut dyn FnMut(&Config, &str, &str) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let Some(main) = main_of(&site.path) else {
        anyhow::bail!("{} isn't a git worktree", site.path.display());
    };
    if !force {
        let Some(info) = crate::git::info(&site.path) else {
            anyhow::bail!(
                "Anvil couldn't check {} for uncommitted changes or unpushed commits",
                site.name
            );
        };
        if info.changed > 0 {
            let n = info.changed;
            anyhow::bail!(
                "{} has {n} uncommitted {}",
                site.name,
                if n == 1 { "change" } else { "changes" }
            );
        }
        if info.ahead > 0 {
            anyhow::bail!("{} has {} unpushed commits", site.name, info.ahead);
        }
    }
    // Databases first: a failed drop leaves the folder, so nothing is half gone.
    if drop_db && let Some(w) = cfg.worktrees.get(&site.name).cloned() {
        let mut left = Vec::new();
        let mut failed = None;
        for copy in w.copies {
            if failed.is_none() {
                match drop(cfg, &copy.0, &copy.1) {
                    Ok(()) => continue,
                    Err(e) => failed = Some(e),
                }
            }
            left.push(copy);
        }
        if let Some(w) = cfg.worktrees.get_mut(&site.name) {
            w.copies = left;
        }
        if let Some(e) = failed {
            return Err(e);
        }
    }
    let mut cmd = std::process::Command::new("git");
    cmd.arg("-C").arg(&main).args(["worktree", "remove"]);
    if force {
        cmd.arg("--force");
    }
    let out = cmd.arg(&site.path).output()?;
    if !out.status.success() {
        anyhow::bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    if cfg
        .worktrees
        .get(&site.name)
        .is_some_and(|w| w.copies.is_empty())
    {
        cfg.worktrees.remove(&site.name);
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::process::Command;

    pub(crate) fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("anvil-wt-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        fs::canonicalize(&d).unwrap()
    }

    pub(crate) fn git(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t", "-C"])
            .arg(dir)
            .args(args)
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    }

    /// A repo `<tmp>/main` with one commit and a linked worktree `<tmp>/x` on branch `x`.
    pub(crate) fn repo(name: &str) -> (PathBuf, PathBuf) {
        let root = tmp(name);
        let main = root.join("main");
        fs::create_dir(&main).unwrap();
        git(&main, &["init", "-q", "-b", "main"]);
        git(&main, &["commit", "-q", "--allow-empty", "-m", "i"]);
        git(&main, &["worktree", "add", "-q", "../x", "-b", "x"]);
        (main, root.join("x"))
    }

    /// `shop--feature-x`, a worktree of `shop` whose main checkout has `dotenv` as its `.env`.
    pub(crate) fn worktree_site(name: &str, dotenv: &str) -> (Config, Site) {
        let (main, x) = repo(name);
        fs::write(main.join(".env"), dotenv).unwrap();
        let mut site = crate::tests::site("shop--feature-x", x.to_str().unwrap(), Some("8.4"));
        site.path = x;
        site.secure = false;
        site.worktree_of = Some("shop".into());
        let cfg = Config {
            http_port: 80,
            ..Config::default()
        };
        (cfg, site)
    }

    fn get<'a>(env: &'a [(String, String)], k: &str) -> Option<&'a str> {
        env.iter()
            .find(|(key, _)| key == k)
            .map(|(_, v)| v.as_str())
    }

    #[test]
    fn env_points_a_laravel_worktree_at_its_own_database() {
        let (cfg, site) = worktree_site("laravel", "DB_CONNECTION=mysql\nDB_DATABASE=shop\n");
        let env = env(&cfg, &site).unwrap();
        assert_eq!(get(&env, "DB_DATABASE"), Some("shop_feature_x"));
        assert_eq!(get(&env, "APP_URL"), Some("http://shop--feature-x.test"));
        assert_eq!(get(&env, "REDIS_PREFIX"), Some("shop_feature_x_"));
        assert_eq!(get(&env, "CACHE_PREFIX"), Some("shop_feature_x_"));
        assert_eq!(get(&env, "DATABASE_URL"), None);
        // The main app itself gets nothing.
        let main = crate::tests::site("shop", "/s/shop", None);
        assert!(super::env(&cfg, &main).unwrap().is_empty());
    }

    #[test]
    fn env_rewrites_database_url() {
        let url = "DATABASE_URL=\"postgres://postgres@127.0.0.1:5433/shop?schema=public\"\n";
        let (cfg, site) = worktree_site("url", url);
        let env = env(&cfg, &site).unwrap();
        assert_eq!(
            get(&env, "DATABASE_URL"),
            Some("postgres://postgres@127.0.0.1:5433/shop_feature_x?schema=public")
        );
        assert_eq!(get(&env, "DB_DATABASE"), None);
    }

    #[test]
    fn share_choice_leaves_the_database_alone() {
        let dotenv = "DB_DATABASE=shop\nDATABASE_URL=mysql://root@127.0.0.1/shop\n";
        let (mut cfg, site) = worktree_site("share", dotenv);
        cfg.worktrees.insert(
            site.name.clone(),
            WorktreeConfig {
                db: DbChoice::Share,
                ..Default::default()
            },
        );
        let env = env(&cfg, &site).unwrap();
        assert_eq!(get(&env, "DB_DATABASE"), None);
        assert_eq!(get(&env, "DATABASE_URL"), None);
        assert_eq!(get(&env, "REDIS_PREFIX"), Some("shop_feature_x_"));
        assert_eq!(get(&env, "CACHE_PREFIX"), Some("shop_feature_x_"));
    }

    #[test]
    fn no_database_settings_only_the_address_and_prefixes() {
        let (cfg, site) = worktree_site("nodb", "APP_NAME=Shop\n");
        let keys: Vec<_> = env(&cfg, &site)
            .unwrap()
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert_eq!(keys, ["APP_URL", "REDIS_PREFIX", "CACHE_PREFIX"]);
    }

    #[test]
    fn a_server_connection_without_db_database_still_gets_its_own() {
        // Laravel would fall back to `laravel`, which main uses too.
        let (cfg, site) = worktree_site("nodbname", "DB_CONNECTION=pgsql\n");
        let env = env(&cfg, &site).unwrap();
        assert_eq!(get(&env, "DB_DATABASE"), Some("shop_feature_x"));
    }

    #[test]
    fn db_url_and_other_schemes_are_rewritten() {
        let (cfg, site) = worktree_site("dburl", "DB_URL=mysql2://root@127.0.0.1:3306/shop\n");
        let env = env(&cfg, &site).unwrap();
        assert_eq!(
            get(&env, "DB_URL"),
            Some("mysql2://root@127.0.0.1:3306/shop_feature_x")
        );
        let (m, w) = (Path::new("/m"), Path::new("/w"));
        assert_eq!(
            with_db("sqlserver://h:1433;database=shop;user=sa", "d", m, w).as_deref(),
            Some("sqlserver://h:1433;database=d;user=sa")
        );
        assert_eq!(
            with_db("pgsql://u@h", "d", m, w).as_deref(),
            Some("pgsql://u@h/d")
        );
        assert_eq!(
            with_db("file:./dev.db", "d", m, w).as_deref(),
            Some("file:./dev.db")
        );
    }

    #[test]
    fn an_unknown_scheme_leaves_the_worktree_without_a_database() {
        let (mut cfg, site) = worktree_site("scheme", "DATABASE_URL=redis://h:6379/0\n");
        let err = checked_env(&cfg, &site).unwrap_err().to_string();
        assert_eq!(
            err,
            "Anvil can't point shop--feature-x at its own database (DATABASE_URL uses a kind of database Anvil can't rewrite); choose Share main's to use main's database"
        );
        // Share lifts it: main's database, on purpose.
        cfg.worktrees.insert(
            site.name.clone(),
            WorktreeConfig {
                db: DbChoice::Share,
                ..Default::default()
            },
        );
        let env = env(&cfg, &site).unwrap();
        assert_eq!(get(&env, "DATABASE_URL"), None);
    }

    #[test]
    fn a_value_php_fpm_cant_take_leaves_the_worktree_without_a_database() {
        let dotenv = "DB_URL=mysql://root:pa$$w0rd@127.0.0.1/shop\n";
        let (mut cfg, mut site) = worktree_site("fpmsafe", dotenv);
        assert_eq!(
            env(&cfg, &site).unwrap_err(),
            "DB_URL holds characters PHP-FPM can't take"
        );
        // Only PHP-FPM can't take it: a Node worktree gets it as it is.
        site.php = None;
        assert!(env(&cfg, &site).is_ok());
        site.php = Some("8.4".into());
        cfg.worktrees.entry(site.name.clone()).or_default().db = DbChoice::Share;
        assert!(env(&cfg, &site).is_ok());
    }

    #[test]
    fn sqlite_in_the_checkout_maps_into_the_worktree_through_symlinks() {
        let (_, site) = worktree_site("sqlite-in", "");
        let main = main_of(&site.path).unwrap();
        let link = main.parent().unwrap().join("link");
        std::os::unix::fs::symlink(&main, &link).unwrap();
        let file = link.join("database/app.sqlite");
        let dotenv = format!("DB_CONNECTION=sqlite\nDB_DATABASE={}\n", file.display());
        fs::write(main.join(".env"), dotenv).unwrap();
        let cfg = Config::default();
        let env = env(&cfg, &site).unwrap();
        let want = site.path.join("database/app.sqlite");
        assert_eq!(get(&env, "DB_DATABASE"), want.to_str());
    }

    #[test]
    fn sqlite_outside_the_checkout_gets_a_file_in_the_worktree() {
        let (_, site) = worktree_site("sqlite-out", "");
        let main = main_of(&site.path).unwrap();
        let dotenv = "DB_CONNECTION=sqlite\nDB_DATABASE=/var/data/shop.sqlite\n";
        fs::write(main.join(".env"), dotenv).unwrap();
        let env = env(&Config::default(), &site).unwrap();
        let want = site.path.join("database/shop.sqlite");
        assert_eq!(get(&env, "DB_DATABASE"), want.to_str());
    }

    /// A fake database server.
    #[derive(Default)]
    struct Server {
        exists: bool,
        stopped: bool,
    }

    /// `prepare_with` on `srv`: whether it made one, and what was said and asked in
    /// order (`Running` left out). The copy marker sits next to the worktree.
    fn prep(cfg: &mut Config, site: &Site, srv: &Server) -> anyhow::Result<(bool, Vec<String>)> {
        let log = std::cell::RefCell::new(Vec::new());
        let marker = marker_of(site);
        let made = prepare_with(
            cfg,
            site,
            &marker,
            &mut |line| log.borrow_mut().push(line.to_string()),
            &mut |_, id, op| {
                let (entry, answer) = match op {
                    Op::Running => return Ok(!srv.stopped),
                    Op::Exists(n) => (format!("{id} exists {n}"), srv.exists),
                    Op::Clone { from, to } => (format!("{id} clone {from} {to}"), true),
                    Op::Create(n) => (format!("{id} create {n}"), true),
                    Op::Drop(n) => (format!("{id} drop {n}"), true),
                };
                log.borrow_mut().push(entry);
                Ok(answer)
            },
        )?;
        Ok((made, log.into_inner()))
    }

    fn marker_of(site: &Site) -> PathBuf {
        site.path.with_file_name("copying")
    }

    fn with_postgres(mut cfg: Config) -> Config {
        cfg.services.insert(
            "postgres-x".into(),
            crate::services::ServiceConfig {
                kind: Some("postgres".into()),
                port: Some(5433),
                ..Default::default()
            },
        );
        cfg
    }

    #[test]
    fn prepare_is_idempotent_and_records_the_copy() {
        let dotenv = "DB_CONNECTION=pgsql\nDB_PORT=5433\nDB_DATABASE=shop\n";
        let (cfg, site) = worktree_site("prep", dotenv);
        let mut cfg = with_postgres(cfg);
        let (made, log) = prep(&mut cfg, &site, &Server::default()).unwrap();
        assert!(made);
        // The line comes before the work.
        assert_eq!(
            log,
            [
                "postgres-x exists shop_feature_x",
                "Anvil: preparing shop_feature_x (copy of shop)",
                "postgres-x clone shop shop_feature_x"
            ]
        );
        assert_eq!(
            cfg.worktrees[&site.name].copies,
            [("postgres-x".to_string(), "shop_feature_x".to_string())]
        );
        assert!(!marker_of(&site).exists(), "cleared after the copy");
        let (made, log) = prep(&mut cfg, &site, &Server::default()).unwrap();
        assert!(!made && log.is_empty());
        // The same through DATABASE_URL.
        let url = "DATABASE_URL=postgresql://postgres@127.0.0.1:5433/shop?schema=public\n";
        let (cfg, site) = worktree_site("prep-url", url);
        let mut cfg = with_postgres(cfg);
        let (made, log) = prep(&mut cfg, &site, &Server::default()).unwrap();
        assert!(made);
        assert_eq!(log[2], "postgres-x clone shop shop_feature_x");
    }

    #[test]
    fn an_existing_database_is_kept_and_not_recorded() {
        let (cfg, site) = worktree_site("prep-exists", "DB_CONNECTION=pgsql\nDB_PORT=5433\n");
        let mut cfg = with_postgres(cfg);
        let srv = Server {
            exists: true,
            ..Default::default()
        };
        let (made, log) = prep(&mut cfg, &site, &srv).unwrap();
        assert!(!made);
        assert_eq!(log, ["postgres-x exists shop_feature_x"]);
        assert!(!cfg.worktrees.contains_key(&site.name));
    }

    #[test]
    fn an_interrupted_copy_is_dropped_and_made_again() {
        let (cfg, site) = worktree_site("prep-cut", "DB_CONNECTION=pgsql\nDB_PORT=5433\n");
        let mut cfg = with_postgres(cfg);
        // A copy that failed half-way leaves its marker behind.
        let mut failing = |_: &Config, _: &str, op: Op| match op {
            Op::Clone { .. } => anyhow::bail!("cut off"),
            _ => Ok(matches!(op, Op::Running)),
        };
        let marker = marker_of(&site);
        assert!(prepare_with(&mut cfg, &site, &marker, &mut |_| {}, &mut failing).is_err());
        assert!(marker.exists());
        assert!(!cfg.worktrees.contains_key(&site.name));
        // Next time the half database is there: dropped and copied again.
        let srv = Server {
            exists: true,
            ..Default::default()
        };
        let (made, log) = prep(&mut cfg, &site, &srv).unwrap();
        assert!(made);
        assert_eq!(
            log,
            [
                "postgres-x exists shop_feature_x",
                "postgres-x drop shop_feature_x",
                "Anvil: preparing shop_feature_x (copy of laravel)",
                "postgres-x clone laravel shop_feature_x"
            ]
        );
        assert!(!marker.exists());
        assert_eq!(cfg.worktrees[&site.name].copies.len(), 1);
    }

    #[test]
    fn a_stopped_service_refuses_to_prepare() {
        let (cfg, site) = worktree_site("prep-stopped", "DB_CONNECTION=pgsql\nDB_PORT=5433\n");
        let mut cfg = with_postgres(cfg);
        let srv = Server {
            stopped: true,
            ..Default::default()
        };
        let e = prep(&mut cfg, &site, &srv).unwrap_err();
        assert_eq!(e.to_string(), "Start postgres-x to prepare shop_feature_x");
        assert!(!cfg.worktrees.contains_key(&site.name));
    }

    #[test]
    fn sqlite_copies_into_the_same_relative_path() {
        for (name, dotenv) in [
            (
                "sqlite-rel",
                "DB_CONNECTION=sqlite\nDB_DATABASE=database/database.sqlite\n",
            ),
            ("sqlite-default", "DB_CONNECTION=sqlite\n"),
        ] {
            let (mut cfg, site) = worktree_site(name, dotenv);
            let main = main_of(&site.path).unwrap();
            fs::create_dir_all(main.join("database")).unwrap();
            fs::write(main.join("database/database.sqlite"), "rows").unwrap();
            let (made, log) = prep(&mut cfg, &site, &Server::default()).unwrap();
            assert!(made);
            // Said, and nothing asked of a server.
            assert_eq!(
                log,
                ["Anvil: preparing database/database.sqlite (copy of shop's)"]
            );
            let copy = site.path.join("database/database.sqlite");
            assert_eq!(fs::read_to_string(&copy).unwrap(), "rows", "{name}");
            // It lives in the worktree folder: nothing to record.
            assert!(cfg.worktrees.is_empty());
            fs::write(&copy, "changed").unwrap();
            let (made, _) = prep(&mut cfg, &site, &Server::default()).unwrap();
            assert!(!made);
            assert_eq!(fs::read_to_string(&copy).unwrap(), "changed");
        }
    }

    #[test]
    fn empty_choice_creates_without_copying() {
        let (cfg, site) = worktree_site("prep-empty", "DB_CONNECTION=pgsql\nDB_PORT=5433\n");
        let mut cfg = with_postgres(cfg);
        cfg.worktrees.entry(site.name.clone()).or_default().db = DbChoice::Empty;
        let (made, log) = prep(&mut cfg, &site, &Server::default()).unwrap();
        assert!(made);
        assert_eq!(
            log,
            [
                "postgres-x exists shop_feature_x",
                "Anvil: preparing shop_feature_x (empty)",
                "postgres-x create shop_feature_x"
            ]
        );
        assert_eq!(cfg.worktrees[&site.name].copies.len(), 1);
    }

    #[test]
    fn share_choice_does_nothing() {
        let (cfg, site) = worktree_site("prep-share", "DB_CONNECTION=pgsql\nDB_PORT=5433\n");
        let mut cfg = with_postgres(cfg);
        cfg.worktrees.entry(site.name.clone()).or_default().db = DbChoice::Share;
        let (made, log) = prep(&mut cfg, &site, &Server::default()).unwrap();
        assert!(!made && log.is_empty());
        assert!(cfg.worktrees[&site.name].copies.is_empty());
    }

    #[test]
    fn an_external_database_is_not_copied() {
        for (name, dotenv) in [
            (
                "ext-host",
                "DB_CONNECTION=pgsql\nDB_HOST=db.example.com\nDB_PORT=5433\n",
            ),
            // Local, but no Anvil service on that port.
            (
                "ext-port",
                "DB_CONNECTION=mysql\nDB_PORT=3307\nDB_DATABASE=shop\n",
            ),
        ] {
            let (cfg, site) = worktree_site(name, dotenv);
            let mut cfg = with_postgres(cfg);
            let (made, log) = prep(&mut cfg, &site, &Server::default()).unwrap();
            assert!(!made && log.is_empty() && cfg.worktrees.is_empty());
            let db = main_db(&cfg, &site);
            assert_eq!(db, MainDb::External, "{name}");
            assert_eq!(db.choices(), [DbChoice::Share, DbChoice::Empty]);
        }
        // An Anvil one may be copied.
        let (cfg, site) = worktree_site("ext-not", "DB_CONNECTION=pgsql\nDB_PORT=5433\n");
        let db = main_db(&with_postgres(cfg), &site);
        assert_eq!(
            db,
            MainDb::Anvil {
                service: "postgres-x".into(),
                database: "laravel".into()
            }
        );
        assert_eq!(db.choices().len(), 3);
    }

    #[test]
    fn long_db_names_stay_unique() {
        let mut a = crate::tests::site("shop--x", "/w", None);
        a.worktree_of = Some("shop".into());
        let mut b = a.clone();
        let common = "a".repeat(70);
        a.name = format!("shop--{common}-one");
        b.name = format!("shop--{common}-two");
        let (na, nb) = (db_name(&a), db_name(&b));
        assert_ne!(na, nb);
        assert!(na.len() <= 63 && nb.len() <= 63);
        assert!(na.starts_with("shop_aaaa"));
        assert!(crate::databases::check_db(&na).is_ok());
        assert_eq!(db_name(&a), na, "stable");
    }

    #[test]
    fn db_names_are_slugged_and_short() {
        let mut site = crate::tests::site("shop--feature-x", "/w", None);
        site.worktree_of = Some("shop".into());
        assert_eq!(db_name(&site), "shop_feature_x");
        site.name = format!("shop--{}", "a".repeat(80));
        assert_eq!(db_name(&site).len(), 63);
    }

    #[test]
    fn parses_branches_detached_and_prunable() {
        let text = "worktree /m\nHEAD a\nbranch refs/heads/main\n\n\
            worktree /w/a\nHEAD b\nbranch refs/heads/feature/login\n\n\
            worktree /w/b\nHEAD c\ndetached\n\n\
            worktree /w/c\nHEAD d\nbranch refs/heads/c\nprunable gitdir file points to non-existent location\n";
        let l = parse_porcelain(text);
        assert_eq!(l.len(), 3);
        assert_eq!(l[0].path, PathBuf::from("/w/a"));
        assert_eq!(l[0].branch.as_deref(), Some("feature/login"));
        assert!(l[1].detached && l[1].branch.is_none());
        assert!(l[2].prunable && !l[0].prunable);
    }

    #[test]
    fn a_real_repo_lists_its_worktrees() {
        let (main, x) = repo("real");
        let l = list(&main);
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].path, fs::canonicalize(&x).unwrap());
        assert_eq!(l[0].branch.as_deref(), Some("x"));
        assert_eq!(main_of(&x), Some(main.clone()));
        assert_eq!(main_of(&main), None);
    }

    #[test]
    fn branch_switch_keeps_the_entry() {
        let (main, x) = repo("switch");
        let before = list(&main);
        git(&x, &["checkout", "-q", "-b", "y"]);
        let after = list(&main);
        assert_eq!(after[0].path, before[0].path);
        assert_eq!(after[0].branch.as_deref(), Some("y"));
    }

    #[test]
    fn no_repo_no_git_no_entries() {
        let d = tmp("plain");
        assert!(list(&d).is_empty());
        assert_eq!(main_of(&d), None);
        // Missing git binary: `git()` maps a failed spawn to None, which `list` turns into empty.
        assert!(crate::git::git(&d.join("nope"), &["status"]).is_none());
    }

    #[test]
    fn cache_skips_git_when_nothing_changed() {
        let (main, _) = repo("cache");
        let spawns = || crate::git::SPAWNS.with(|c| c.get());
        let first = list(&main);
        let n = spawns();
        assert_eq!(list(&main), first);
        assert_eq!(spawns(), n);
    }

    fn recorded(cfg: &mut Config, site: &Site) {
        cfg.worktrees.insert(
            site.name.clone(),
            WorktreeConfig {
                copies: vec![("postgres-x".into(), "shop_feature_x".into())],
                ..Default::default()
            },
        );
    }

    #[test]
    fn a_vanished_worktree_with_a_copy_is_a_leftover() {
        let (mut cfg, site) = worktree_site("left", "");
        assert!(leftovers(&cfg, &[]).is_empty());
        recorded(&mut cfg, &site);
        assert!(leftovers(&cfg, std::slice::from_ref(&site)).is_empty());
        assert_eq!(
            leftovers(&cfg, &[]),
            [Leftover {
                app: "shop".into(),
                worktree: "feature-x".into(),
                service: "postgres-x".into(),
                database: "shop_feature_x".into()
            }]
        );
    }

    #[test]
    fn remove_refuses_uncommitted_changes() {
        let (mut cfg, site) = worktree_site("remove", "");
        recorded(&mut cfg, &site);
        fs::write(site.path.join("a.txt"), "x").unwrap();
        let dropped = std::cell::RefCell::new(Vec::new());
        let mut drop = |_: &Config, id: &str, db: &str| {
            dropped.borrow_mut().push(format!("{id} {db}"));
            Ok(())
        };
        let e = remove_with(&mut cfg, &site, true, false, &mut drop).unwrap_err();
        assert!(e.to_string().contains("1 uncommitted change"), "{e}");
        assert!(site.path.exists() && cfg.worktrees.contains_key(&site.name));
        remove_with(&mut cfg, &site, true, true, &mut drop).unwrap();
        assert!(!site.path.exists());
        assert_eq!(*dropped.borrow(), ["postgres-x shop_feature_x"]);
        assert!(cfg.worktrees.is_empty());
    }

    #[test]
    fn remove_keeps_the_database_unless_asked() {
        let (mut cfg, site) = worktree_site("remove-keep", "");
        recorded(&mut cfg, &site);
        let mut never = |_: &Config, _: &str, _: &str| panic!("dropped");
        remove_with(&mut cfg, &site, false, false, &mut never).unwrap();
        assert!(!site.path.exists());
        // The copy stays on record, as a leftover.
        assert_eq!(leftovers(&cfg, &[]).len(), 1);
    }

    #[test]
    fn a_failing_drop_leaves_the_folder_and_the_other_records() {
        let (mut cfg, site) = worktree_site("remove-fail", "");
        recorded(&mut cfg, &site);
        cfg.worktrees
            .get_mut(&site.name)
            .unwrap()
            .copies
            .push(("postgres-x".into(), "other".into()));
        let mut drop = |_: &Config, _: &str, db: &str| {
            if db == "other" {
                anyhow::bail!("server down")
            }
            Ok(())
        };
        assert!(remove_with(&mut cfg, &site, true, false, &mut drop).is_err());
        assert!(site.path.exists());
        assert_eq!(cfg.worktrees[&site.name].copies.len(), 1);
        assert_eq!(cfg.worktrees[&site.name].copies[0].1, "other");
    }

    #[test]
    fn remove_fails_closed_when_git_cant_say() {
        let (mut cfg, site) = worktree_site("remove-closed", "");
        // main_of still works, but git status fails: break its HEAD.
        let git_dir = crate::git::git_dir(&site.path).unwrap();
        fs::write(git_dir.join("HEAD"), "garbage").unwrap();
        let mut never = |_: &Config, _: &str, _: &str| panic!("dropped");
        let e = remove_with(&mut cfg, &site, true, false, &mut never).unwrap_err();
        assert!(e.to_string().contains("couldn't check"), "{e}");
        assert!(site.path.exists());
    }

    #[test]
    fn add_creates_the_folder_next_to_the_app() {
        let (main, _) = repo("add");
        let root = main.parent().unwrap().join("apps");
        fs::create_dir(&root).unwrap();
        let shop = root.join("shop");
        git(
            &main,
            &[
                "worktree",
                "add",
                "-q",
                shop.to_str().unwrap(),
                "-b",
                "shop",
            ],
        );
        let mut app = crate::tests::site("shop", shop.to_str().unwrap(), None);
        app.path = shop.clone();
        fs::write(shop.join(".env"), "A=1\n").unwrap();
        let cfg = Config::default();
        let folder = add(&cfg, &app, "feature/login", true).unwrap();
        assert_eq!(folder, root.join("shop--feature-login"));
        assert_eq!(fs::read(folder.join(".env")).unwrap(), b"A=1\n");
        assert_eq!(fs::read(shop.join(".env")).unwrap(), b"A=1\n");
        // An existing branch, and a folder already there.
        git(&shop, &["branch", "old"]);
        assert!(
            add(&cfg, &app, "old", false)
                .unwrap()
                .ends_with("shop--old")
        );
        let e = add(&cfg, &app, "old", false).unwrap_err().to_string();
        assert!(e.contains("already exists"), "{e}");
    }

    #[test]
    fn add_refuses_an_existing_folder() {
        let (main, _) = repo("addx");
        let mut app = crate::tests::site("main", main.to_str().unwrap(), None);
        app.path = main.clone();
        fs::create_dir(main.parent().unwrap().join("main--b")).unwrap();
        let e = add(&Config::default(), &app, "b", true).unwrap_err();
        assert!(e.to_string().contains("already exists"));
    }
}
