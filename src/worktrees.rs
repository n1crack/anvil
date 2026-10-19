//! Parallel workspaces: git worktrees of an app, each served on its own address.

/// `<first>--<rest>`: the address label of an app's extra server or worktree.
pub fn joined(first: &str, rest: &str) -> String {
    format!("{first}--{rest}")
}

/// `joined(app, label)` as one DNS label (at most 63 bytes): a longer one keeps its
/// start and ends in a hash of the whole, as `db_name` does, so it stays the same.
pub fn capped(app: &str, label: &str) -> String {
    let name = joined(app, label);
    if name.len() <= 63 {
        return name;
    }
    let hash = crate::short_hash(&name);
    let mut end = 63 - 1 - hash.len();
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}-{hash}", name[..end].trim_end_matches(['-', '.']))
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
use std::fs;
use std::path::{Path, PathBuf};

/// Which database a worktree uses.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum DbChoice {
    /// Its own database, a copy of the main app's.
    #[default]
    Copy,
    /// Its own database, empty (filled by migrations). A `share` stored before Share
    /// main's was removed loads as this: a worktree never uses main's database.
    #[serde(alias = "share")]
    Empty,
}

impl std::str::FromStr for DbChoice {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "copy" => Ok(DbChoice::Copy),
            "empty" => Ok(DbChoice::Empty),
            "share" => Err("a worktree always has its own database: copy or empty".into()),
            _ => Err(format!("\"{s}\" isn't one of copy, empty")),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct WorktreeConfig {
    pub db: DbChoice,
    /// Copies Anvil made for this worktree: (service, database).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub copies: Vec<(String, String)>,
    /// Its database was there already and the user chose to use it ("Use existing").
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub adopted: bool,
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
    // Never main's own (main `shop` on `shop_dev`, worktree `dev`): a name of its own.
    let main_db = main_of(&site.path).and_then(|m| main_database(&m));
    if main_db.as_deref() == Some(name.as_str()) {
        name = format!("{name}_wt_{}", crate::short_hash(&name));
    }
    fit(name)
}

/// At most 63 bytes (Postgres' limit, MySQL's is 64): a longer name keeps its start and
/// ends in a hash of the whole.
fn fit(mut name: String) -> String {
    if name.len() > 63 {
        let hash = crate::short_hash(&name);
        name.truncate(63 - 1 - hash.len());
        name = format!("{name}_{hash}");
    }
    name
}

/// The name of main's database on a server, as main's `.env` gives it, by `env`'s rules:
/// a URL's path, else `DB_DATABASE` with a server connection or none named, else Laravel's
/// `laravel`; `None` for SQLite or no settings.
fn main_database(main: &Path) -> Option<String> {
    let env = crate::resources::dotenv(main);
    let get = |k: &str| env.get(k).map(String::as_str).filter(|v| !v.is_empty());
    let url_db = get("DB_URL").or(get("DATABASE_URL")).and_then(url_database);
    if url_db.is_some() {
        return url_db;
    }
    match get("DB_CONNECTION") {
        Some("sqlite") => None,
        None => get("DB_DATABASE").map(String::from),
        Some(_) => Some(get("DB_DATABASE").unwrap_or("laravel").into()),
    }
}

/// The database a `scheme://host/<db>?…` URL names; `None` without one (or a `file:` URL).
pub(crate) fn url_database(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let rest = &rest[..rest.find(['?', '#']).unwrap_or(rest.len())];
    rest.split_once('/')
        .map(|(_, db)| db.to_string())
        .filter(|db| !db.is_empty())
}

/// Whether `db` is the main app's own database: its name on a server, main's
/// `DB_DATABASE` as written, or an absolute SQLite path to main's file or into the main
/// checkout (not into the worktree, which may sit inside it).
pub fn is_mains_database(site: &Site, db: &str) -> bool {
    let Some(main) = main_of(&site.path) else {
        return false;
    };
    let mains = crate::resources::dotenv(&main).get("DB_DATABASE").cloned();
    if main_database(&main).as_deref() == Some(db) || mains.as_deref() == Some(db) {
        return true;
    }
    if !Path::new(db).is_absolute() {
        return false;
    }
    let file = real_file(Path::new(db));
    if file.starts_with(real_file(&site.path)) {
        return false;
    }
    file.starts_with(real_file(&main))
        || mains.is_some_and(|m| Path::new(&m).is_absolute() && real_file(Path::new(&m)) == file)
}

/// The worktree's record; for one Anvil has none of (opened outside Anvil, say), Settings'
/// default choice.
fn record(cfg: &Config, site: &Site) -> WorktreeConfig {
    cfg.worktrees
        .get(&site.name)
        .cloned()
        .unwrap_or_else(|| WorktreeConfig {
            db: cfg.worktree_db,
            ..Default::default()
        })
}

/// `record`, kept in `cfg`.
fn entry<'a>(cfg: &'a mut Config, site: &Site) -> &'a mut WorktreeConfig {
    let db = cfg.worktree_db;
    cfg.worktrees
        .entry(site.name.clone())
        .or_insert_with(|| WorktreeConfig {
            db,
            ..Default::default()
        })
}

/// The worktree's database choice (`record`'s).
pub fn choice(cfg: &Config, site: &Site) -> DbChoice {
    record(cfg, site).db
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
    /// What a worktree can choose: one on a server Anvil doesn't manage can only be empty.
    pub fn choices(&self) -> &'static [DbChoice] {
        match self {
            MainDb::External => &[DbChoice::Empty],
            _ => &[DbChoice::Copy, DbChoice::Empty],
        }
    }
}

/// Laravel's SQLite file when `DB_DATABASE` isn't set.
const DEFAULT_SQLITE: &str = "database/database.sqlite";

/// The main app's database for the worktree `site`, read from main's `.env`.
pub fn main_db(cfg: &Config, site: &Site) -> MainDb {
    match main_of(&site.path) {
        Some(main) => main_db_at(cfg, &main, &site.path),
        None => MainDb::None,
    }
}

/// `main_db` for the main checkout `main` and a worktree at `wt`.
fn main_db_at(cfg: &Config, main: &Path, wt: &Path) -> MainDb {
    use crate::resources::{is_local, url_host, url_port};
    let env = crate::resources::dotenv(main);
    let get = |k: &str| env.get(k).map(String::as_str).filter(|v| !v.is_empty());
    let conn = get("DB_CONNECTION");
    if conn == Some("sqlite") {
        let (from, to) = match get("DB_DATABASE") {
            Some(":memory:") => return MainDb::None,
            Some(f) => match sqlite_file(main, wt, f) {
                Some(to) => (PathBuf::from(f), PathBuf::from(to)),
                None => (main.join(f), wt.join(f)),
            },
            None => (main.join(DEFAULT_SQLITE), wt.join(DEFAULT_SQLITE)),
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
/// `None` when it has its own already, or there's nothing Anvil can make.
pub fn pending(cfg: &Config, site: &Site) -> Option<(DbChoice, MainDb)> {
    site.worktree_of.as_ref()?;
    let w = record(cfg, site);
    if !w.copies.is_empty() {
        return None;
    }
    match main_db(cfg, site) {
        MainDb::None | MainDb::External => None,
        MainDb::Sqlite { to, .. } if to.exists() => None,
        db => Some((w.db, db)),
    }
}

/// Its own database is made: a copy or empty one recorded, or its SQLite file there.
pub fn prepared(cfg: &Config, site: &Site) -> bool {
    cfg.worktrees
        .get(&site.name)
        .is_some_and(|w| !w.copies.is_empty())
        || matches!(main_db(cfg, site), MainDb::Sqlite { to, .. } if to.exists())
}

/// What the worktree may choose now: what main's database allows until its own is made,
/// then only that one.
pub fn choices(cfg: &Config, site: &Site) -> Vec<DbChoice> {
    if prepared(cfg, site) {
        return vec![choice(cfg, site)];
    }
    main_db(cfg, site).choices().to_vec()
}

/// The New Worktree dialog's Database choice for a new worktree of an app.
#[derive(Serialize, Debug, PartialEq)]
pub struct NewDb {
    /// What main's database allows (`choices`' rules).
    pub choices: Vec<DbChoice>,
    /// Settings' default when allowed, else the first choice.
    pub default: DbChoice,
    /// Main's database by name; `None` when main has no database settings (nothing to choose).
    pub main_database: Option<String>,
}

pub fn new_db(cfg: &Config, app: &Site) -> NewDb {
    let found = main_db_at(cfg, &app.path, &app.path);
    let choices = found.choices().to_vec();
    let default = if choices.contains(&cfg.worktree_db) {
        cfg.worktree_db
    } else {
        choices[0]
    };
    let main_database = match found {
        MainDb::None => None,
        MainDb::Anvil { database, .. } => Some(database),
        MainDb::Sqlite { .. } | MainDb::External => {
            Some(main_database(&app.path).unwrap_or_else(|| format!("{}'s database", app.name)))
        }
    };
    NewDb {
        choices,
        default,
        main_database,
    }
}

/// Record `choice` for the worktree, refusing one `choices` doesn't offer.
pub fn set_db(cfg: &mut Config, site: &Site, choice: DbChoice) -> anyhow::Result<()> {
    let allowed = choices(cfg, site);
    if !allowed.contains(&choice) {
        let why = if prepared(cfg, site) {
            "its own database is made already"
        } else {
            "main's database is on a server Anvil doesn't manage; choose Empty"
        };
        anyhow::bail!("{} can't use that database ({why})", site.name);
    }
    entry(cfg, site).db = choice;
    Ok(())
}

/// `set_db` for the worktree in `folder` (one `add` just made).
pub fn set_db_at(cfg: &mut Config, folder: &Path, choice: DbChoice) -> anyhow::Result<()> {
    let real = |p: &Path| fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let site = crate::sites(cfg)
        .into_iter()
        .find(|s| s.worktree_of.is_some() && real(&s.path) == real(folder))
        .ok_or_else(|| anyhow::anyhow!("Anvil doesn't see {} as a worktree", folder.display()))?;
    set_db(cfg, &site, choice)
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
            // It lives in the worktree folder and goes with it: only the choice is recorded,
            // so a later Settings default doesn't change what it uses.
            entry(cfg, site);
            Ok(true)
        }
        MainDb::Anvil { service, database } => {
            if !run(cfg, &service, Op::Running)? {
                anyhow::bail!("Start {service} to prepare {name}");
            }
            if run(cfg, &service, Op::Exists(&name))? {
                // The marker names the database whose copy was cut off.
                if fs::read_to_string(marker).ok().as_deref() != Some(name.as_str()) {
                    // Not Anvil's: maybe another app's. Used only when the user says so (`adopt`).
                    anyhow::bail!("{name} already exists and Anvil didn't make it");
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
            entry(cfg, site).copies.push((service, name));
            fs::remove_file(marker)?;
            Ok(true)
        }
        MainDb::None | MainDb::External => Ok(false),
    }
}

/// Use the existing database the worktree would get ("Use existing <db>"): recorded as
/// its own, so nothing copies over it. Like a copy, it is dropped only when the user asks.
pub fn adopt(cfg: &mut Config, site: &Site) -> anyhow::Result<()> {
    adopt_with(cfg, site, &mut on_server)
}

fn adopt_with(
    cfg: &mut Config,
    site: &Site,
    run: &mut dyn FnMut(&Config, &str, Op) -> anyhow::Result<bool>,
) -> anyhow::Result<()> {
    let name = db_name(site);
    let Some((_, MainDb::Anvil { service, .. })) = pending(cfg, site) else {
        anyhow::bail!("{} has no database to use", site.name);
    };
    if !run(cfg, &service, Op::Exists(&name))? {
        anyhow::bail!("{name} doesn't exist");
    }
    let w = entry(cfg, site);
    w.copies.push((service, name));
    w.adopted = true;
    Ok(())
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
    if made {
        let mine = record(cfg, site);
        let (saved, ()) = crate::update(|c| {
            let w = c.worktrees.entry(site.name.clone()).or_insert(mine.clone());
            for copy in &mine.copies {
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
pub(crate) fn with_db(url: &str, db: &str, main: &Path, wt: &Path) -> Option<String> {
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
    let real = real_file(file);
    let main = fs::canonicalize(main).unwrap_or_else(|_| main.to_path_buf());
    // Already in the worktree (its own `.env`; a worktree may sit inside main's folder).
    if real.starts_with(fs::canonicalize(wt).unwrap_or_else(|_| wt.to_path_buf())) {
        return None;
    }
    let to = match real.strip_prefix(&main) {
        Ok(rel) => wt.join(rel),
        Err(_) => wt.join("database").join(real.file_name()?),
    };
    Some(to.display().to_string())
}

/// `file` canonical, so a symlinked folder still matches; the file itself may not exist yet.
fn real_file(file: &Path) -> PathBuf {
    fs::canonicalize(file)
        .ok()
        .or_else(|| {
            Some(
                fs::canonicalize(file.parent()?)
                    .ok()?
                    .join(file.file_name()?),
            )
        })
        .unwrap_or_else(|| file.to_path_buf())
}

/// What a worktree's processes get on top of their usual environment, so they never
/// reach the main app's database or Redis keys. Empty for anything but a worktree.
/// Set variables win over the worktree's `.env` (Laravel, dotenv, Next.js, Prisma).
/// `Err(reason)` when Anvil can't work out the worktree's own database: then nothing
/// of the worktree runs (`checked_env`, no PHP-FPM pool).
pub fn env(cfg: &Config, site: &Site) -> Result<Vec<(String, String)>, String> {
    if site.worktree_of.is_none() {
        return Ok(Vec::new());
    }
    let mut env: Vec<(String, String)> = Vec::new();
    let db = db_name(site);
    let main = main_of(&site.path).ok_or("its main checkout wasn't found")?;
    // What its processes would load: main's `.env` (copied in), then its own files
    // over it, in Next.js' order (the last one wins).
    let mut files = vec![
        crate::resources::dotenv(&main),
        crate::resources::dotenv(&site.path),
    ];
    for f in [".env.development", ".env.local", ".env.development.local"] {
        files.push(crate::resources::dotenv_file(&site.path.join(f)));
    }
    let last = |k: &str| {
        files
            .iter()
            .rev()
            .find_map(|f| f.get(k).filter(|v| !v.is_empty()))
    };
    // Any file naming a server counts: Laravel reads only `.env`, Next.js all of them.
    let server = files.iter().any(|f| {
        f.get("DB_CONNECTION")
            .is_some_and(|c| !c.is_empty() && c != "sqlite")
    });
    let sqlite = last("DB_CONNECTION").map(String::as_str) == Some("sqlite");
    if server || (!sqlite && files.iter().any(|f| f.contains_key("DB_DATABASE"))) {
        // Laravel falls back to `laravel` without DB_DATABASE: never main's.
        env.push(("DB_DATABASE".into(), db.clone()));
    } else if sqlite
        && let Some(file) = last("DB_DATABASE").and_then(|f| sqlite_file(&main, &site.path, f))
    {
        env.push(("DB_DATABASE".into(), file));
    }
    for key in ["DB_URL", "DATABASE_URL"] {
        // Every one of them must be rewritable, whichever file wins.
        for url in files
            .iter()
            .filter_map(|f| f.get(key))
            .filter(|u| !u.is_empty())
        {
            with_db(url, &db, &main, &site.path)
                .ok_or_else(|| format!("{key} uses a kind of database Anvil can't rewrite"))?;
        }
        if let Some(url) = last(key).and_then(|u| with_db(u, &db, &main, &site.path)) {
            env.push((key.into(), url));
        }
    }
    let prefix = format!("{db}_");
    env.push(("APP_URL".into(), crate::site_url(cfg, site)));
    env.push(("REDIS_PREFIX".into(), prefix.clone()));
    env.push(("CACHE_PREFIX".into(), prefix));
    if site.php.is_some()
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
            "Anvil can't point {} at its own database ({reason}), so nothing of it runs",
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

/// The linked worktrees of the repository at `main`, read from its `.git/worktrees/<id>`
/// folders (`gitdir`, `HEAD`): no git process, so a missing or broken git (or one that
/// refuses the repository) never turns a worktree into a plain app that reaches main's data.
pub fn list(main: &Path) -> Vec<Worktree> {
    let Some(dir) = crate::git::git_dir(main).map(|d| d.join("worktrees")) else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<Worktree> = entries
        .flatten()
        .filter_map(|e| {
            let admin = e.path();
            // `<worktree>/.git`, absolute or (worktree.useRelativePaths) relative to `admin`.
            let dot_git = admin.join(fs::read_to_string(admin.join("gitdir")).ok()?.trim());
            let path = dot_git.parent()?;
            let head = fs::read_to_string(admin.join("HEAD")).unwrap_or_default();
            let branch = head.trim().strip_prefix("ref: ").map(|r| {
                r.trim()
                    .strip_prefix("refs/heads/")
                    .unwrap_or(r.trim())
                    .to_string()
            });
            Some(Worktree {
                path: fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()),
                detached: branch.is_none(),
                branch,
                prunable: !dot_git.exists(),
            })
        })
        .collect();
    // `read_dir` order isn't stable; the `-2` suffixes must not depend on it.
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
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
/// A `db` choice main's database doesn't allow is refused first; the caller records it
/// (`set_db_at`) once the worktree is there.
pub fn add(
    cfg: &Config,
    app: &Site,
    branch: &str,
    new_branch: bool,
    db: Option<DbChoice>,
) -> anyhow::Result<PathBuf> {
    if let Some(choice) = db
        && !new_db(cfg, app).choices.contains(&choice)
    {
        anyhow::bail!(
            "A worktree of {} can't use that database (main's database is on a server Anvil doesn't manage; choose Empty)",
            app.name
        );
    }
    let Some(parent) = app.path.parent() else {
        anyhow::bail!("{} has no parent folder", app.path.display());
    };
    // Never an option to git (`--force`, `--detach`), and a name git takes as a branch.
    if branch.is_empty() || branch.starts_with('-') {
        anyhow::bail!("\"{branch}\" isn't a branch name");
    }
    crate::git::run(&app.path, &["check-ref-format", "--branch", branch])
        .map_err(|_| anyhow::anyhow!("\"{branch}\" isn't a branch name"))?;
    let folder = parent.join(capped(&app.name, &crate::site_name(branch)));
    if folder.exists() {
        anyhow::bail!("{} already exists", folder.display());
    }
    let o = std::ffi::OsStr::new;
    let mut args = vec![o("worktree"), o("add")];
    if new_branch {
        args.extend([o("-b"), o(branch)]);
    }
    args.extend([o("--"), folder.as_os_str()]);
    if !new_branch {
        args.push(o(branch));
    }
    crate::git::run(&app.path, &args)?;
    let (from, to) = (app.path.join(".env"), folder.join(".env"));
    if from.is_file() && !to.exists() {
        fs::copy(from, to)?;
    }
    Ok(folder)
}

/// What a worktree (one made outside Anvil, say) lacks to run: main's `.env`, which git
/// ignores, and its packages: `(id, command)` for `install`.
#[derive(Serialize, Debug, Default, PartialEq)]
pub struct Missing {
    pub env: bool,
    pub installs: Vec<(String, String)>,
}

pub fn missing(site: &Site) -> Missing {
    let (dir, mut out) = (&site.path, Missing::default());
    out.env = !dir.join(".env").exists() && main_of(dir).is_some_and(|m| m.join(".env").is_file());
    if dir.join("composer.json").is_file() && !dir.join("vendor").exists() {
        out.installs
            .push(("composer".into(), "composer install".into()));
    }
    if dir.join("package.json").is_file() && !dir.join("node_modules").exists() {
        let manager = crate::apps::js_manager(dir);
        out.installs
            .push(("js".into(), format!("{manager} install")));
    }
    out
}

/// Give the worktree main's `.env`; never over one it has.
pub fn copy_env(site: &Site) -> anyhow::Result<()> {
    let main =
        main_of(&site.path).ok_or_else(|| anyhow::anyhow!("{} isn't a git worktree", site.name))?;
    let to = site.path.join(".env");
    if to.exists() {
        anyhow::bail!("{} has a .env already", site.name);
    }
    fs::copy(main.join(".env"), to)?;
    Ok(())
}

/// Install the worktree's packages (`missing`'s ids): `composer install` through Anvil's
/// composer shim, or node_modules with the lock file's manager, as a dev server would.
/// With the worktree's env: a composer script that runs artisan stays off main's database.
pub fn install(cfg: &Config, site: &Site, what: &str) -> anyhow::Result<()> {
    use crate::shims::resolve_with;
    let dir = &site.path;
    let (program, args, env) = match what {
        "composer" => {
            crate::create::composer()?;
            let (php, mut pre, env) = resolve_with("composer", dir, cfg, false)?;
            pre.extend(["install".into(), "--no-interaction".into()]);
            (php, pre, env)
        }
        "js" => {
            let install = crate::apps::js_install(crate::apps::js_manager(dir));
            let (_, _, env) = resolve_with("node", dir, cfg, false)?;
            ("/bin/sh".into(), vec!["-c".into(), install], env)
        }
        _ => anyhow::bail!("Anvil doesn't install {what}"),
    };
    let out = std::process::Command::new(program)
        .args(args)
        .envs(env)
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .output()?;
    if !out.status.success() {
        let text = String::from_utf8_lossy(&out.stderr);
        let tail: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
        anyhow::bail!("{}", tail[tail.len().saturating_sub(3)..].join("\n"));
    }
    Ok(())
}

/// A database Anvil copied for a worktree that is gone. Never dropped on its own.
#[derive(Debug, Clone, PartialEq, Serialize)]
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

/// The record a leftover of `app`'s worktree `worktree` comes from.
fn leftover_key(app: &str, worktree: &str) -> String {
    if worktree.is_empty() {
        app.into()
    } else {
        joined(app, worktree)
    }
}

/// Keep a leftover's databases: Anvil forgets they were the gone worktree's.
pub fn keep_leftover(cfg: &mut Config, sites: &[Site], app: &str, worktree: &str) {
    let key = leftover_key(app, worktree);
    if !sites.iter().any(|s| s.name == key) {
        cfg.worktrees.remove(&key);
    }
}

/// Drop a leftover database and forget it. Refused for one that isn't a leftover (its
/// worktree still exists, or Anvil never made it). Changes `cfg` only: the caller saves it.
pub fn drop_leftover(
    cfg: &mut Config,
    sites: &[Site],
    service: &str,
    database: &str,
) -> anyhow::Result<()> {
    drop_leftover_with(cfg, sites, service, database, &mut |c, id, db| {
        crate::databases::drop_database(c, id, db)
    })
}

fn drop_leftover_with(
    cfg: &mut Config,
    sites: &[Site],
    service: &str,
    database: &str,
    drop: &mut dyn FnMut(&Config, &str, &str) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let copy = (service.to_string(), database.to_string());
    let Some(l) = leftovers(cfg, sites)
        .into_iter()
        .find(|l| l.service == service && l.database == database)
    else {
        anyhow::bail!("{database} isn't a database left from a removed worktree");
    };
    let live = sites.iter().any(|s| {
        cfg.worktrees
            .get(&s.name)
            .is_some_and(|w| w.copies.contains(&copy))
    });
    if live {
        anyhow::bail!("A worktree still uses {database}");
    }
    drop(cfg, service, database)?;
    let key = leftover_key(&l.app, &l.worktree);
    if let Some(w) = cfg.worktrees.get_mut(&key) {
        w.copies.retain(|c| *c != copy);
        if w.copies.is_empty() {
            cfg.worktrees.remove(&key);
        }
    }
    Ok(())
}

/// Refuses a worktree with uncommitted changes or unpushed commits, or one git can't tell about.
fn check_clean(site: &Site) -> anyhow::Result<()> {
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
    // Detached (mid-rebase, a checked-out commit): commits made there are on no branch,
    // and removing the worktree drops the only HEAD that reaches them.
    let args = [
        "rev-list",
        "-n1",
        "HEAD",
        "--not",
        "--branches",
        "--tags",
        "--remotes",
    ];
    if info.detached && crate::git::run(&site.path, &args).map_or(true, |o| !o.trim().is_empty()) {
        anyhow::bail!(
            "{} has commits on no branch, lost with the worktree",
            site.name
        );
    }
    Ok(())
}

/// `stop` (the worktree's processes) once `remove`'s safety check passes, unless `force`:
/// a refused remove leaves everything running.
pub fn stop_if_clean<T>(site: &Site, force: bool, stop: impl FnOnce() -> T) -> anyhow::Result<T> {
    if !force {
        check_clean(site)?;
    }
    Ok(stop())
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
        check_clean(site)?;
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
    let o = std::ffi::OsStr::new;
    let mut args = vec![o("worktree"), o("remove")];
    if force {
        args.push(o("--force"));
    }
    args.extend([o("--"), site.path.as_os_str()]);
    crate::git::run(&main, &args)?;
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
    fn a_stored_share_loads_as_empty() {
        let cfg: Config =
            toml::from_str("worktree_db = \"share\"\n[worktrees.\"shop--x\"]\ndb = \"share\"\n")
                .unwrap();
        assert_eq!(cfg.worktree_db, DbChoice::Empty);
        assert_eq!(cfg.worktrees["shop--x"].db, DbChoice::Empty);
        let e = "share".parse::<DbChoice>().unwrap_err();
        assert_eq!(e, "a worktree always has its own database: copy or empty");
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
            "Anvil can't point shop--feature-x at its own database (DATABASE_URL uses a kind of database Anvil can't rewrite), so nothing of it runs"
        );
        // Empty doesn't lift it: that needs the URL rewritten too.
        cfg.worktrees.entry(site.name.clone()).or_default().db = DbChoice::Empty;
        assert!(env(&cfg, &site).is_err());
    }

    #[test]
    fn a_value_php_fpm_cant_take_leaves_the_worktree_without_a_database() {
        let dotenv = "DB_URL=mysql://root:pa$$w0rd@127.0.0.1/shop\n";
        let (cfg, mut site) = worktree_site("fpmsafe", dotenv);
        assert_eq!(
            env(&cfg, &site).unwrap_err(),
            "DB_URL holds characters PHP-FPM can't take"
        );
        // Only PHP-FPM can't take it: a Node worktree gets it as it is.
        site.php = None;
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
    fn an_existing_database_anvil_didnt_make_is_used_only_when_asked() {
        let (cfg, site) = worktree_site("prep-exists", "DB_CONNECTION=pgsql\nDB_PORT=5433\n");
        let mut cfg = with_postgres(cfg);
        let srv = Server {
            exists: true,
            ..Default::default()
        };
        // Maybe another app's (`blog_api` of the app `blog-api`): refused, nothing recorded.
        let e = prep(&mut cfg, &site, &srv).unwrap_err();
        assert_eq!(
            e.to_string(),
            "shop_feature_x already exists and Anvil didn't make it"
        );
        assert!(!cfg.worktrees.contains_key(&site.name));
        // "Use existing shop_feature_x": recorded as its own, and nothing is copied over it.
        let mut answer = |_: &Config, _: &str, op: Op| Ok(matches!(op, Op::Exists(_)));
        adopt_with(&mut cfg, &site, &mut answer).unwrap();
        let w = &cfg.worktrees[&site.name];
        assert!(w.adopted);
        assert_eq!(
            w.copies,
            [("postgres-x".to_string(), "shop_feature_x".to_string())]
        );
        let (made, log) = prep(&mut cfg, &site, &srv).unwrap();
        assert!(!made && log.is_empty());
        // Not there: nothing to use.
        let (cfg2, site2) = worktree_site("adopt-none", "DB_CONNECTION=pgsql\nDB_PORT=5433\n");
        let mut cfg2 = with_postgres(cfg2);
        let mut none = |_: &Config, _: &str, _: Op| Ok(false);
        assert!(adopt_with(&mut cfg2, &site2, &mut none).is_err());
        assert!(cfg2.worktrees.is_empty());
    }

    #[test]
    fn a_worktree_never_gets_mains_own_database_name() {
        for dotenv in [
            "DB_CONNECTION=pgsql\nDB_DATABASE=shop_dev\n",
            "DATABASE_URL=postgres://u@127.0.0.1:5433/shop_dev?schema=public\n",
        ] {
            let (cfg, mut site) = worktree_site("main-name", dotenv);
            site.name = "shop--dev".into();
            let db = db_name(&site);
            assert_ne!(db, "shop_dev");
            assert!(db.starts_with("shop_dev_wt_"), "{db}");
            assert_eq!(db_name(&site), db, "stable");
            let env = env(&cfg, &site).unwrap();
            assert!(
                env.iter()
                    .all(|(_, v)| !v.ends_with("/shop_dev?schema=public"))
            );
            assert!(env.iter().all(|(k, v)| k != "DB_DATABASE" || *v == db));
        }
    }

    #[test]
    fn env_reads_the_worktrees_own_env_files_and_export_lines() {
        let (cfg, site) = worktree_site("own-files", "APP_NAME=Shop\n");
        fs::write(site.path.join(".env"), "export DB_CONNECTION=mysql\n").unwrap();
        fs::write(
            site.path.join(".env.local"),
            "DATABASE_URL=postgres://u@127.0.0.1:5433/shop\n",
        )
        .unwrap();
        let env = env(&cfg, &site).unwrap();
        assert_eq!(get(&env, "DB_DATABASE"), Some("shop_feature_x"));
        assert_eq!(
            get(&env, "DATABASE_URL"),
            Some("postgres://u@127.0.0.1:5433/shop_feature_x")
        );
        // One that can't be rewritten in any of them refuses, whichever file wins.
        fs::write(site.path.join(".env"), "export DB_URL=redis://h/0\n").unwrap();
        fs::write(
            site.path.join(".env.local"),
            "DB_URL=mysql://r@127.0.0.1/shop\n",
        )
        .unwrap();
        let e = env_err(&cfg, &site);
        assert_eq!(e, "DB_URL uses a kind of database Anvil can't rewrite");
    }

    fn env_err(cfg: &Config, site: &Site) -> String {
        env(cfg, site).unwrap_err()
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
            // It lives in the worktree folder: only the choice is recorded.
            assert!(cfg.worktrees[&site.name].copies.is_empty());
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
            assert_eq!(db.choices(), [DbChoice::Empty]);
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
        assert_eq!(db.choices(), [DbChoice::Copy, DbChoice::Empty]);
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
    fn a_leftover_is_dropped_or_kept_and_then_forgotten() {
        let (mut cfg, site) = worktree_site("left-drop", "");
        recorded(&mut cfg, &site);
        let dropped = std::cell::RefCell::new(Vec::new());
        let mut drop = |_: &Config, id: &str, db: &str| {
            dropped.borrow_mut().push(format!("{id} {db}"));
            Ok(())
        };
        let live = std::slice::from_ref(&site);
        // Its worktree is still there: not a leftover, nothing dropped or forgotten.
        let e = drop_leftover_with(&mut cfg, live, "postgres-x", "shop_feature_x", &mut drop);
        assert!(e.is_err());
        keep_leftover(&mut cfg, live, "shop", "feature-x");
        assert!(dropped.borrow().is_empty() && cfg.worktrees.contains_key(&site.name));
        // Gone: Delete drops and forgets it.
        drop_leftover_with(&mut cfg, &[], "postgres-x", "shop_feature_x", &mut drop).unwrap();
        assert_eq!(*dropped.borrow(), ["postgres-x shop_feature_x"]);
        assert!(cfg.worktrees.is_empty());
        // Keep only forgets.
        recorded(&mut cfg, &site);
        keep_leftover(&mut cfg, &[], "shop", "feature-x");
        assert!(leftovers(&cfg, &[]).is_empty());
        assert_eq!(dropped.borrow().len(), 1);
    }

    #[test]
    fn a_refused_remove_stops_nothing() {
        let (_, site) = worktree_site("stop-clean", "");
        fs::write(site.path.join("a.txt"), "x").unwrap();
        let mut stopped = false;
        let e = stop_if_clean(&site, false, || stopped = true).unwrap_err();
        assert!(e.to_string().contains("1 uncommitted change"), "{e}");
        assert!(!stopped);
        stop_if_clean(&site, true, || stopped = true).unwrap();
        assert!(stopped);
    }

    #[test]
    fn the_database_choice_follows_main_and_what_is_made() {
        use DbChoice::*;
        // Main on a server Anvil doesn't manage: Empty only.
        let (mut cfg, site) = worktree_site(
            "choice-ext",
            "DB_CONNECTION=mysql\nDB_HOST=db.example.com\n",
        );
        assert_eq!(choices(&cfg, &site), [Empty]);
        let e = set_db(&mut cfg, &site, Copy).unwrap_err();
        assert_eq!(
            e.to_string(),
            "shop--feature-x can't use that database (main's database is on a server Anvil doesn't manage; choose Empty)"
        );
        set_db(&mut cfg, &site, Empty).unwrap();
        // Made already: only that one.
        let (mut cfg, site) = worktree_site("choice-made", "DB_CONNECTION=sqlite\n");
        assert_eq!(choices(&cfg, &site), [Copy, Empty]);
        recorded(&mut cfg, &site);
        assert_eq!(choices(&cfg, &site), [Copy]);
        let e = set_db(&mut cfg, &site, Empty).unwrap_err();
        assert!(e.to_string().contains("made already"), "{e}");
        set_db(&mut cfg, &site, Copy).unwrap();
        assert!(pending(&cfg, &site).is_none());
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
        let folder = add(&cfg, &app, "feature/login", true, None).unwrap();
        assert_eq!(folder, root.join("shop--feature-login"));
        assert_eq!(fs::read(folder.join(".env")).unwrap(), b"A=1\n");
        assert_eq!(fs::read(shop.join(".env")).unwrap(), b"A=1\n");
        // An existing branch, and a folder already there.
        git(&shop, &["branch", "old"]);
        assert!(
            add(&cfg, &app, "old", false, None)
                .unwrap()
                .ends_with("shop--old")
        );
        let e = add(&cfg, &app, "old", false, None).unwrap_err().to_string();
        assert!(e.contains("already exists"), "{e}");
    }

    #[test]
    fn add_refuses_a_branch_git_would_take_as_an_option() {
        let (main, _) = repo("add-opt");
        let mut app = crate::tests::site("main", main.to_str().unwrap(), None);
        app.path = main.clone();
        let cfg = Config::default();
        for bad in ["", "-f", "--detach", "a..b", "x y", "-"] {
            let e = add(&cfg, &app, bad, true, None).unwrap_err().to_string();
            assert!(e.contains("isn't a branch name"), "{bad}: {e}");
        }
        let folders = fs::read_dir(main.parent().unwrap()).unwrap().count();
        assert_eq!(folders, 2, "main and x only");
        // A name that is fine still works, and an existing branch too.
        assert!(add(&cfg, &app, "feat/ok", true, None).is_ok());
        assert!(add(&cfg, &app, "x2", true, None).is_ok());
    }

    #[test]
    fn remove_refuses_commits_on_no_branch() {
        let (mut cfg, site) = worktree_site("detached", "");
        // Detached at a commit a branch has: nothing to lose.
        git(&site.path, &["checkout", "-q", "--detach"]);
        assert!(check_clean(&site).is_ok());
        git(&site.path, &["commit", "-q", "--allow-empty", "-m", "lost"]);
        let mut never = |_: &Config, _: &str, _: &str| panic!("dropped");
        let e = remove_with(&mut cfg, &site, false, false, &mut never).unwrap_err();
        assert_eq!(
            e.to_string(),
            "shop--feature-x has commits on no branch, lost with the worktree"
        );
        assert!(site.path.exists());
        remove_with(&mut cfg, &site, false, true, &mut never).unwrap();
        assert!(!site.path.exists());
    }

    #[test]
    fn long_worktree_names_stay_one_dns_label() {
        let long = "a".repeat(80);
        let name = capped("shop", &long);
        assert_eq!(name.len(), 63);
        assert!(name.starts_with("shop--aaa"));
        assert_eq!(capped("shop", &long), name, "stable");
        assert_ne!(capped("shop", &format!("{long}-2")), name);
        assert_eq!(capped("shop", "x"), "shop--x");
    }

    #[test]
    fn a_worktree_says_what_it_lacks_and_gets_mains_env() {
        let (_, site) = worktree_site("missing", "A=1\n");
        assert_eq!(
            missing(&site),
            Missing {
                env: true,
                installs: vec![]
            }
        );
        fs::write(site.path.join("composer.json"), "{}").unwrap();
        fs::write(site.path.join("package.json"), "{}").unwrap();
        fs::write(site.path.join("pnpm-lock.yaml"), "").unwrap();
        let ids: Vec<_> = missing(&site)
            .installs
            .into_iter()
            .map(|(_, c)| c)
            .collect();
        assert_eq!(ids, ["composer install", "pnpm install"]);
        copy_env(&site).unwrap();
        assert_eq!(fs::read_to_string(site.path.join(".env")).unwrap(), "A=1\n");
        assert!(!missing(&site).env);
        assert!(copy_env(&site).is_err(), "never over its own");
        fs::create_dir(site.path.join("vendor")).unwrap();
        fs::create_dir(site.path.join("node_modules")).unwrap();
        assert_eq!(missing(&site), Missing::default());
    }

    #[test]
    fn any_file_naming_a_server_keeps_the_worktree_off_mains_database() {
        // N1: Laravel reads `.env` (pgsql, shop), not `.env.local`'s sqlite.
        let (cfg, site) = worktree_site("n1", "DB_CONNECTION=pgsql\nDB_DATABASE=shop\n");
        fs::write(site.path.join(".env.local"), "DB_CONNECTION=sqlite\n").unwrap();
        let env = env(&cfg, &site).unwrap();
        assert_eq!(get(&env, "DB_DATABASE"), Some("shop_feature_x"));
    }

    #[test]
    fn env_development_files_are_read_too() {
        let (cfg, site) = worktree_site("env-dev", "APP_NAME=Shop\n");
        fs::write(
            site.path.join(".env.development"),
            "DATABASE_URL=postgres://u@127.0.0.1:5433/shop\n",
        )
        .unwrap();
        let env = env(&cfg, &site).unwrap();
        assert_eq!(
            get(&env, "DATABASE_URL"),
            Some("postgres://u@127.0.0.1:5433/shop_feature_x")
        );
        fs::write(
            site.path.join(".env.development.local"),
            "DB_URL=redis://h/0\n",
        )
        .unwrap();
        assert_eq!(
            env_err(&cfg, &site),
            "DB_URL uses a kind of database Anvil can't rewrite"
        );
    }

    #[test]
    fn mains_database_name_follows_envs_rules() {
        // M1: a URL with no database path plus DB_DATABASE, and DB_DATABASE alone.
        for dotenv in [
            "DATABASE_URL=postgres://u@127.0.0.1:5433\nDB_DATABASE=shop_dev\n",
            "DB_DATABASE=shop_dev\n",
        ] {
            let (cfg, mut site) = worktree_site("m1", dotenv);
            site.name = "shop--dev".into();
            let db = db_name(&site);
            assert!(db.starts_with("shop_dev_wt_"), "{dotenv}: {db}");
            let env = env(&cfg, &site).unwrap();
            assert!(env.iter().all(|(k, v)| k != "DB_DATABASE" || *v == db));
        }
    }

    #[test]
    fn settings_default_applies_to_a_worktree_opened_outside_anvil() {
        let dotenv = "DB_CONNECTION=pgsql\nDB_PORT=5433\nDB_DATABASE=shop\n";
        let (cfg, site) = worktree_site("default-empty", dotenv);
        let mut cfg = with_postgres(cfg);
        cfg.worktree_db = DbChoice::Empty;
        assert_eq!(pending(&cfg, &site).map(|p| p.0), Some(DbChoice::Empty));
        // Made empty, and the choice stays with it.
        let (made, log) = prep(&mut cfg, &site, &Server::default()).unwrap();
        assert!(made);
        assert_eq!(log[1], "Anvil: preparing shop_feature_x (empty)");
        cfg.worktree_db = DbChoice::Copy;
        assert_eq!(cfg.worktrees[&site.name].db, DbChoice::Empty);
        assert_eq!(
            get(&env(&cfg, &site).unwrap(), "DB_DATABASE"),
            Some("shop_feature_x")
        );
    }

    #[test]
    fn mains_own_database_is_known_by_name() {
        let (_, site) = worktree_site("is-mains", "DB_CONNECTION=pgsql\nDB_DATABASE=shop\n");
        assert!(is_mains_database(&site, "shop"));
        assert!(!is_mains_database(&site, "shop_testing"));
        assert!(!is_mains_database(&site, ":memory:"));
    }

    /// `root/shop`, a git repo parked in `root`, as an app of `cfg`.
    fn parked_app(name: &str, dotenv: &str) -> (Config, Site) {
        let root = tmp(name);
        let shop = root.join("shop");
        fs::create_dir(&shop).unwrap();
        git(&shop, &["init", "-q", "-b", "main"]);
        git(&shop, &["commit", "-q", "--allow-empty", "-m", "i"]);
        fs::write(shop.join(".env"), dotenv).unwrap();
        let mut cfg = with_postgres(Config::default());
        cfg.paths.push(root);
        let app = crate::sites(&cfg)
            .into_iter()
            .find(|s| s.name == "shop")
            .unwrap();
        (cfg, app)
    }

    #[test]
    fn a_new_worktree_gets_the_database_chosen_for_it() {
        let (mut cfg, app) = parked_app("add-db", "DB_CONNECTION=pgsql\nDB_PORT=5433\n");
        cfg.worktree_db = DbChoice::Empty;
        let new = new_db(&cfg, &app);
        assert_eq!(new.choices, [DbChoice::Copy, DbChoice::Empty]);
        assert_eq!(new.default, DbChoice::Empty);
        assert_eq!(new.main_database.as_deref(), Some("laravel"));
        let folder = add(&cfg, &app, "feat", true, Some(DbChoice::Copy)).unwrap();
        set_db_at(&mut cfg, &folder, DbChoice::Copy).unwrap();
        assert_eq!(cfg.worktrees["shop--feat"].db, DbChoice::Copy);
    }

    #[test]
    fn a_new_worktree_refuses_a_choice_main_doesnt_allow() {
        let (cfg, app) = parked_app("add-ext", "DB_CONNECTION=mysql\nDB_HOST=db.example.com\n");
        let new = new_db(&cfg, &app);
        assert_eq!(new.choices, [DbChoice::Empty]);
        // Copy, Settings' default, isn't offered: the first one is.
        assert_eq!(new.default, DbChoice::Empty);
        let e = add(&cfg, &app, "feat", true, Some(DbChoice::Copy)).unwrap_err();
        assert_eq!(
            e.to_string(),
            "A worktree of shop can't use that database (main's database is on a server Anvil doesn't manage; choose Empty)"
        );
        assert!(!app.path.with_file_name("shop--feat").exists());
        // No database settings: nothing to choose.
        let (cfg, app) = parked_app("add-none", "APP_NAME=Shop\n");
        assert!(new_db(&cfg, &app).main_database.is_none());
    }

    #[test]
    fn add_refuses_an_existing_folder() {
        let (main, _) = repo("addx");
        let mut app = crate::tests::site("main", main.to_str().unwrap(), None);
        app.path = main.clone();
        fs::create_dir(main.parent().unwrap().join("main--b")).unwrap();
        let e = add(&Config::default(), &app, "b", true, None).unwrap_err();
        assert!(e.to_string().contains("already exists"));
    }
}
