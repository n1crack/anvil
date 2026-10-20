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
use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

/// One layer of an app's worktree hooks (`resolve`): each field set, or left to the next layer.
/// Read leniently (see `Deserialize`): it sits inside config.toml, which a typo here must not break.
#[derive(Serialize, Debug, Clone, Default, PartialEq)]
pub struct Hooks {
    /// Files or folders copied from the main checkout into a new worktree (git leaves out
    /// ignored ones); a `*` in the last part matches any run of characters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub copy: Option<Vec<String>>,
    /// New Worktree's "Copy the database too": on, a copy of main's; off, an empty one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub copy_database: Option<bool>,
    /// Env keys set per worktree, `{db}` `{url}` `{prefix}` `{port}` `{name}` filled in. A
    /// key that is just `{db}` points at the worktree's database: a URL's database part
    /// is replaced, a SQLite file maps into the worktree.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keys: Option<BTreeMap<String, String>>,
    /// Keys whose value in the worktree's env files must differ from main's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub check: Option<Vec<String>>,
    /// Commands for a new worktree, run when the user says so; `{install}` is the JS
    /// install its lock file asks for (left out without package.json).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after_create: Option<Vec<String>>,
    /// What isn't one of these, or not of its kind, kept as written (`check_hooks` says so).
    #[serde(flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub unknown: BTreeMap<String, serde_json::Value>,
}

impl Hooks {
    pub fn is_empty(&self) -> bool {
        *self == Hooks::default()
    }
}

impl<'de> Deserialize<'de> for Hooks {
    /// Each field that reads as its kind; anything else goes to `unknown`.
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut map = BTreeMap::<String, serde_json::Value>::deserialize(d)?;
        fn take<T: serde::de::DeserializeOwned>(
            map: &mut BTreeMap<String, serde_json::Value>,
            k: &str,
        ) -> Option<T> {
            let t = serde_json::from_value(map.get(k)?.clone()).ok()?;
            map.remove(k);
            Some(t)
        }
        Ok(Hooks {
            copy: take(&mut map, "copy"),
            copy_database: take(&mut map, "copy_database"),
            keys: take(&mut map, "keys"),
            check: take(&mut map, "check"),
            after_create: take(&mut map, "after_create"),
            unknown: map,
        })
    }
}

/// The framework presets: id, name, the kinds (`apps::KINDS`) that use it. Any other kind uses `other`.
pub const PRESETS: [(&str, &str, &[&str]); 8] = [
    ("laravel", "Laravel", &["laravel"]),
    ("statamic", "Statamic", &["statamic"]),
    ("symfony", "Symfony", &["symfony"]),
    ("next", "Next.js", &["next"]),
    ("nuxt", "Nuxt", &["nuxt"]),
    ("node", "Node.js", &["node", "vite"]),
    (
        "python",
        "Django, FastAPI, Flask",
        &["django", "fastapi", "flask"],
    ),
    ("other", "Other", &[]),
];

/// The presets as Anvil ships them. Each sets the database keys, which do nothing where
/// no env file names a database: no preset lets a worktree reach main's database, only
/// the user taking them out does. `copy_database` is left to Settings.
const SHIPPED: &str = r#"
[laravel]
copy = [".env"]
check = ["DB_DATABASE", "DB_URL", "DATABASE_URL", "REDIS_PREFIX", "CACHE_PREFIX"]
after_create = ["composer install", "{install}", "php artisan migrate"]
keys = { DB_DATABASE = "{db}", DB_URL = "{db}", DATABASE_URL = "{db}", APP_URL = "{url}", REDIS_PREFIX = "{prefix}", CACHE_PREFIX = "{prefix}" }

[statamic]
copy = [".env"]
check = ["DB_DATABASE", "DB_URL", "DATABASE_URL", "REDIS_PREFIX", "CACHE_PREFIX"]
after_create = ["composer install", "{install}", "php artisan migrate"]
keys = { DB_DATABASE = "{db}", DB_URL = "{db}", DATABASE_URL = "{db}", APP_URL = "{url}", REDIS_PREFIX = "{prefix}", CACHE_PREFIX = "{prefix}" }

[symfony]
copy = [".env.local"]
check = ["DATABASE_URL"]
after_create = ["composer install", "php bin/console doctrine:migrations:migrate --no-interaction"]
keys = { DB_DATABASE = "{db}", DB_URL = "{db}", DATABASE_URL = "{db}", DEFAULT_URI = "{url}" }

[next]
copy = [".env", ".env.local"]
check = ["DATABASE_URL"]
after_create = ["{install}"]
keys = { DB_DATABASE = "{db}", DB_URL = "{db}", DATABASE_URL = "{db}" }

[nuxt]
copy = [".env", ".env.local"]
check = ["DATABASE_URL"]
after_create = ["{install}"]
keys = { DB_DATABASE = "{db}", DB_URL = "{db}", DATABASE_URL = "{db}" }

[node]
copy = [".env", ".env.local"]
check = ["DATABASE_URL"]
after_create = ["{install}"]
keys = { DB_DATABASE = "{db}", DB_URL = "{db}", DATABASE_URL = "{db}" }

[python]
copy = [".env"]
check = ["DATABASE_URL"]
after_create = []
keys = { DB_DATABASE = "{db}", DB_URL = "{db}", DATABASE_URL = "{db}" }

[other]
copy = [".env"]
check = ["DB_DATABASE", "DB_URL", "DATABASE_URL"]
after_create = []
keys = { DB_DATABASE = "{db}", DB_URL = "{db}", DATABASE_URL = "{db}" }
"#;

/// The keys pointing at a database. Test runs always get them (`run_env`).
const DB_KEYS: [&str; 3] = ["DB_DATABASE", "DB_URL", "DATABASE_URL"];

/// Whether the env key `k` names a database: `DB_KEYS`, and names like `PGDATABASE`,
/// `MONGO_DB`, `DB_NAME` (a value of main's there is refused, a missing one stops the worktree).
fn names_db(k: &str) -> bool {
    DB_KEYS.contains(&k) || k.contains("DATABASE") || k.ends_with("_DB") || k.ends_with("DB_NAME")
}

/// Anvil's own preset `id`.
fn shipped(id: &str) -> Hooks {
    static ALL: std::sync::OnceLock<BTreeMap<String, Hooks>> = std::sync::OnceLock::new();
    ALL.get_or_init(|| toml::from_str(SHIPPED).expect("the shipped presets parse"))
        .get(id)
        .cloned()
        .unwrap_or_default()
}

/// The preset a kind of app uses.
pub fn preset_of(kind: &str) -> &'static str {
    PRESETS
        .iter()
        .find(|(_, _, kinds)| kinds.contains(&kind))
        .map_or("other", |p| p.0)
}

/// A preset as it applies: the user's edits (Settings › Worktrees) over Anvil's, field by field.
pub fn preset(cfg: &Config, id: &str) -> Hooks {
    let edited = cfg.worktree_presets.get(id).cloned().unwrap_or_default();
    let shipped = shipped(id);
    Hooks {
        copy: edited.copy.or(shipped.copy),
        copy_database: edited.copy_database.or(shipped.copy_database),
        keys: edited.keys.or(shipped.keys),
        check: edited.check.or(shipped.check),
        after_create: edited.after_create.or(shipped.after_create),
        unknown: edited.unknown,
    }
}

/// An app's worktree hooks, every field filled in.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Resolved {
    pub copy: Vec<String>,
    pub copy_database: bool,
    pub keys: BTreeMap<String, String>,
    pub check: Vec<String>,
    pub after_create: Vec<String>,
    /// Where each field came from: "this app", ".anvil.toml", ".worktreeinclude",
    /// "<preset> preset", or "Settings" (`copy_database`).
    pub from: BTreeMap<&'static str, String>,
    /// The name of the preset its kind uses.
    pub preset: &'static str,
    /// What is wrong with a layer, one line each.
    pub problems: Vec<String>,
}

/// The hooks of the app `app` (main checkout `main`, of kind `kind`), field by field, the
/// first layer that sets it: the app's own settings, `.anvil.toml`'s `[worktree]`,
/// `.worktreeinclude` (`copy` only), the preset of its kind, Settings (`copy_database`).
pub fn resolve(cfg: &Config, app: &str, main: &Path, kind: &str) -> Resolved {
    let id = preset_of(kind);
    let name = PRESETS.iter().find(|p| p.0 == id).map_or("Other", |p| p.1);
    let mut settings = shipped("other");
    settings.copy_database = Some(cfg.worktree_db == DbChoice::Copy);
    let layers = [
        (
            "this app".to_string(),
            cfg.sites
                .get(app)
                .map(|s| s.worktree.clone())
                .unwrap_or_default(),
        ),
        (
            ".anvil.toml".into(),
            crate::project::load(main)
                .and_then(|p| p.worktree)
                .unwrap_or_default(),
        ),
        (
            ".worktreeinclude".into(),
            Hooks {
                copy: worktreeinclude(main),
                ..Hooks::default()
            },
        ),
        (format!("{name} preset"), preset(cfg, id)),
        ("Settings".into(), settings),
    ];
    // A layer with a mistake (a typo in config.toml, say): said, its good fields still used.
    let mut problems: Vec<String> = layers
        .iter()
        .flat_map(|(layer, h)| {
            let mut out: Vec<String> = h
                .unknown
                .keys()
                .map(|k| {
                    format!("{layer}: {k} isn't a hooks field, or its value isn't the right kind")
                })
                .collect();
            let rest = Hooks {
                unknown: BTreeMap::new(),
                ..h.clone()
            };
            out.extend(check_hooks(&rest).err().map(|e| format!("{layer}: {e}")));
            out
        })
        .collect();
    let mut from = BTreeMap::new();
    let mut pick = |field: &'static str, has: fn(&Hooks) -> bool| {
        let (layer, hooks) = layers
            .iter()
            .find(|(_, h)| has(h))
            .expect("the last layer sets every field");
        from.insert(field, layer.clone());
        hooks.clone()
    };
    let copy = pick("copy", |h| h.copy.is_some()).copy;
    let copy_database = pick("copy_database", |h| h.copy_database.is_some()).copy_database;
    let mut keys = pick("keys", |h| h.keys.is_some()).keys;
    // Only the user's own layer takes database keys out: one a team file or a preset leaves
    // out comes from the next layer down that sets it (Settings sets them all).
    if !cfg
        .sites
        .get(app)
        .is_some_and(|s| s.worktree.keys.is_some())
        && let Some(keys) = keys.as_mut()
    {
        let mut back = Vec::new();
        for k in DB_KEYS {
            let below = layers.iter().skip(1).filter_map(|(_, h)| h.keys.as_ref());
            if let Some(v) = below.filter_map(|l| l.get(k)).next()
                && !keys.contains_key(k)
            {
                keys.insert(k.into(), v.clone());
                back.push(k);
            }
        }
        if !back.is_empty() {
            problems.push(format!(
                "{}: keys leave out {}, so the layers below give them; only the app's own worktree keys can take them out",
                layers.iter().find(|(_, h)| h.keys.is_some()).map_or("", |l| l.0.as_str()),
                back.join(", ")
            ));
        }
    }
    let check = pick("check", |h| h.check.is_some()).check;
    let after_create = pick("after_create", |h| h.after_create.is_some()).after_create;
    Resolved {
        copy: copy.unwrap_or_default(),
        copy_database: copy_database.unwrap_or_default(),
        keys: keys.unwrap_or_default(),
        check: check.unwrap_or_default(),
        after_create: after_create.unwrap_or_default(),
        from,
        preset: name,
        problems,
    }
}

/// `resolve` for `site`: a worktree's are its app's, read from the main checkout.
pub fn hooks(cfg: &Config, site: &Site) -> Resolved {
    match &site.worktree_of {
        Some(app) => {
            let main = main_of(&site.path).unwrap_or_else(|| site.path.clone());
            resolve(cfg, app, &main, &site.kind)
        }
        None => resolve(cfg, &site.name, &site.path, &site.kind),
    }
}

/// The paths a project's `.worktreeinclude` lists (gitignore-like; `!` lines are left out).
fn worktreeinclude(main: &Path) -> Option<Vec<String>> {
    let text = fs::read_to_string(main.join(".worktreeinclude")).ok()?;
    Some(
        text.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with(['#', '!']))
            .map(String::from)
            .collect(),
    )
}

const PLACEHOLDERS: [&str; 5] = ["db", "url", "prefix", "port", "name"];

/// The `{…}` names in `s`.
fn placeholders(s: &str) -> impl Iterator<Item = &str> {
    s.split('{')
        .skip(1)
        .filter_map(|p| p.split_once('}').map(|(n, _)| n))
}

/// What's wrong with a hooks layer, if anything: for the forms and `.anvil.toml`.
pub fn check_hooks(h: &Hooks) -> Result<(), String> {
    if let Some(k) = h.unknown.keys().next() {
        return Err(format!(
            "{k} isn't a hooks field, or its value isn't the right kind"
        ));
    }
    let key = |k: &str| {
        !k.is_empty()
            && !k.starts_with(|c: char| c.is_ascii_digit())
            && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    };
    for (k, v) in h.keys.iter().flatten() {
        if !key(k) {
            return Err(format!("\"{k}\" isn't an env key name"));
        }
        if v.trim().is_empty() {
            return Err(format!(
                "{k} has no value: give it one, or take the key out (taking a database key out shares main's database)"
            ));
        }
        if let Some(p) = placeholders(v).find(|p| !PLACEHOLDERS.contains(p)) {
            return Err(format!(
                "{k}: {{{p}}} isn't one of {{db}}, {{url}}, {{prefix}}, {{port}}, {{name}}"
            ));
        }
    }
    if let Some(k) = h.check.iter().flatten().find(|k| !key(k)) {
        return Err(format!("\"{k}\" isn't an env key name"));
    }
    if let Some(f) = h.copy.iter().flatten().find(|f| inside(f).is_none()) {
        return Err(format!("\"{f}\" isn't a path inside the project"));
    }
    Ok(())
}

/// `entry` as a relative path inside the project; `None` for an absolute one or one with `..`.
fn inside(entry: &str) -> Option<PathBuf> {
    let p = Path::new(entry.trim().trim_start_matches('/').trim_end_matches('/'));
    let ok = p.components().next().is_some()
        && p.components().all(|c| matches!(c, Component::Normal(_)));
    ok.then(|| p.to_path_buf())
}

/// Set one field of a hooks layer from a form (`None`: back to the next layer), checked.
pub fn set_field(
    h: &mut Hooks,
    field: &str,
    value: Option<serde_json::Value>,
) -> anyhow::Result<()> {
    let mut v = serde_json::to_value(&*h)?;
    let map = v.as_object_mut().expect("Hooks is a map");
    match value {
        Some(x) => map.insert(field.into(), x),
        None => map.remove(field),
    };
    let new: Hooks = serde_json::from_value(v).map_err(|e| anyhow::anyhow!("{e}"))?;
    check_hooks(&new).map_err(anyhow::Error::msg)?;
    *h = new;
    Ok(())
}

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

impl DbChoice {
    pub fn is_empty(&self) -> bool {
        *self == DbChoice::Empty
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
    // Read back to its names only when the app's has no `-` or `.` (where it ends is clear)
    // and its own part no `.` (it reads as a `-`): else a hash of the whole, so app shop-a's
    // worktree x and app shop's worktree a-x don't share shop_a_x.
    let (raw, clear) = match rest {
        Some((main, rest)) => (
            format!("{main}_{rest}"),
            !main.contains(['-', '.']) && !rest.contains('.'),
        ),
        None => (site.name.clone(), false),
    };
    let mut name: String = raw
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    if !clear {
        name = format!("{name}_{}", crate::short_hash(&site.name));
    }
    // Postgres' limit is 63 bytes (MySQL's 64): a longer one keeps its start and gets a
    // hash of the whole, so two long names that start alike still differ.
    // Never main's own (main `shop` on `shop_dev`, worktree `dev`): a name of its own.
    let mains = main_of(&site.path)
        .map(|m| main_databases(&m))
        .unwrap_or_default();
    if mains.iter().any(|m| m.trim().eq_ignore_ascii_case(&name)) {
        name = format!("{name}_wt_{}", crate::short_hash(&name));
    }
    fit(name)
}

/// The database the worktree uses: the one Anvil recorded for it, else `db_name`'s.
pub fn own_db(cfg: &Config, site: &Site) -> String {
    cfg.worktrees
        .get(&site.name)
        .and_then(|w| w.copies.first())
        .map_or_else(|| db_name(site), |(_, db)| db.clone())
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

/// Every database main's `.env` could have it use: `DB_DATABASE` (not a relative SQLite
/// path, which resolves inside the worktree), each URL's database, and Laravel's `laravel`
/// for a server connection without `DB_DATABASE`.
fn main_databases(main: &Path) -> Vec<String> {
    let env = crate::resources::dotenv_file(&main.join(base_env(main, true)));
    let get = |k: &str| env.get(k).map(String::as_str).filter(|v| !v.is_empty());
    let sqlite = get("DB_CONNECTION") == Some("sqlite");
    let mut out: Vec<String> = ["DB_URL", "DATABASE_URL"]
        .into_iter()
        .flat_map(|k| get(k).map(url_databases).unwrap_or_default())
        .collect();
    match get("DB_DATABASE") {
        Some(db) if !(sqlite && !Path::new(db).is_absolute()) => out.push(db.into()),
        None if get("DB_CONNECTION").is_some() && !sqlite => out.push("laravel".into()),
        _ => {}
    }
    // Every other env file at its root (`.env.prod`, `.env.test`, …): one some run of it
    // loads. Not the examples (`.env.example`, `.env.dist`, `*.dist`).
    let others: Vec<String> = fs::read_dir(main)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|f| f.starts_with(".env.") && f != ".env.example" && !f.ends_with(".dist"))
        .collect();
    for f in &others {
        let env = crate::resources::dotenv_file(&main.join(f));
        let get = |k: &str| env.get(k).map(String::as_str).filter(|v| !v.is_empty());
        out.extend(
            ["DB_URL", "DATABASE_URL"]
                .into_iter()
                .flat_map(|k| get(k).map(url_databases).unwrap_or_default()),
        );
        let sqlite = sqlite || get("DB_CONNECTION") == Some("sqlite");
        if let Some(db) = get("DB_DATABASE").filter(|db| !sqlite || Path::new(db).is_absolute()) {
            out.push(db.into());
        }
    }
    out.sort();
    out.dedup();
    out
}

/// The database a `scheme://host/<db>?…` URL names; `None` without one (or a `file:` URL).
/// A `database=` / `dbname=` query parameter wins over the path, as in Laravel and Doctrine.
pub(crate) fn url_database(url: &str) -> Option<String> {
    url_databases(url).pop()
}

/// A query parameter naming the database (Laravel's `database`, Doctrine's `dbname`,
/// mysqlclient's `db`: Django through dj-database-url, SQLAlchemy).
fn is_db_param(key: &str) -> bool {
    ["database", "dbname", "db"]
        .iter()
        .any(|p| key.eq_ignore_ascii_case(p))
}

/// `s` percent-decoded, as Laravel and Doctrine read a URL; `+` is a space in a query.
fn decoded(s: &str, query: bool) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = s
            .get(i + 1..i + 3)
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (b[i], hex) {
            (b'%', Some(c)) => {
                out.push(c);
                i += 3;
                continue;
            }
            (b'+', _) if query => out.push(b' '),
            (c, _) => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `s` percent-encoded for a URL: all but letters, digits and `-._~`.
pub(crate) fn encoded(s: &str) -> String {
    s.bytes()
        .map(|c| match c {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (c as char).to_string()
            }
            c => format!("%{c:02X}"),
        })
        .collect()
}

/// Every database a URL names: its path's, then its query's (the one that is used last),
/// percent-decoded.
fn url_databases(url: &str) -> Vec<String> {
    let Some((_, rest)) = url.split_once("://") else {
        return Vec::new();
    };
    let rest = &rest[..rest.find('#').unwrap_or(rest.len())];
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    let mut out: Vec<String> = path
        .split_once('/')
        .map(|(_, db)| decoded(db, false))
        .into_iter()
        .collect();
    // Laravel and Doctrine `rawurldecode` the query, then `parse_str` it: decoded twice.
    let query = decoded(query, false);
    out.extend(query.split('&').filter_map(|p| {
        let (k, v) = p.split_once('=')?;
        is_db_param(&decoded(k, true)).then(|| decoded(v, true))
    }));
    out.retain(|db| !db.is_empty());
    out
}

/// Whether `db` is the main app's own database: its name on a server, or an absolute SQLite path to main's file or into the main
/// checkout (not into the worktree, which may sit inside it).
pub fn is_mains_database(site: &Site, db: &str) -> bool {
    let Some(main) = main_of(&site.path) else {
        return false;
    };
    let mains = main_databases(&main);
    let db = db.trim();
    // A relative SQLite path resolves inside the worktree: only a server name counts, in any
    // case (MySQL on macOS takes `SHOP` for `shop`).
    if mains.iter().any(|m| m.trim().eq_ignore_ascii_case(db)) {
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
        || mains
            .iter()
            .any(|m| Path::new(m).is_absolute() && real_file(Path::new(m)) == file)
}

/// The worktree's record; for one Anvil has none of (opened outside Anvil, say), the
/// choice its hooks' `copy_database` makes.
fn record(cfg: &Config, site: &Site) -> WorktreeConfig {
    cfg.worktrees
        .get(&site.name)
        .cloned()
        .unwrap_or_else(|| WorktreeConfig {
            db: hooks_choice(cfg, site),
            ..Default::default()
        })
}

/// `copy_database` as a choice.
fn hooks_choice(cfg: &Config, site: &Site) -> DbChoice {
    if hooks(cfg, site).copy_database {
        DbChoice::Copy
    } else {
        DbChoice::Empty
    }
}

/// `record`, kept in `cfg`.
fn entry<'a>(cfg: &'a mut Config, site: &Site) -> &'a mut WorktreeConfig {
    let db = hooks_choice(cfg, site);
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
pub(crate) fn main_db_at(cfg: &Config, main: &Path, wt: &Path) -> MainDb {
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
            let Some((scheme, _)) = url.split_once("://") else {
                return MainDb::None; // `file:`, SQLite through a URL
            };
            let Some((kind, default)) = kind(scheme) else {
                return MainDb::External;
            };
            let port = url_port(url).unwrap_or(default);
            // The one the app uses: a query parameter's over the path's, decoded.
            (kind, url_host(url), port, url_database(url))
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

/// What New Worktree offers for a new worktree of an app: "Copy the database too". Its
/// `after_create` is worked out from the worktree once it is made (`after_create`).
#[derive(Serialize, Debug, PartialEq)]
pub struct NewDb {
    /// Main's database can be copied (it isn't on a server Anvil doesn't manage).
    pub can_copy: bool,
    /// The checkbox as the hooks' `copy_database` sets it; off when it can't be copied.
    pub copy: bool,
    /// Main's database by name; `None` when main has no database settings (nothing to choose).
    pub main_database: Option<String>,
}

pub fn new_db(cfg: &Config, app: &Site) -> NewDb {
    let found = main_db_at(cfg, &app.path, &app.path);
    let can_copy = found.choices().contains(&DbChoice::Copy);
    let hooks = hooks(cfg, app);
    let main_database = match found {
        MainDb::None => None,
        MainDb::Anvil { database, .. } => Some(database),
        MainDb::Sqlite { .. } | MainDb::External => {
            Some(main_database(&app.path).unwrap_or_else(|| format!("{}'s database", app.name)))
        }
    };
    NewDb {
        can_copy,
        copy: can_copy && hooks.copy_database,
        main_database,
    }
}

/// Record `choice` for the worktree, refusing one `choices` doesn't offer.
fn set_db(cfg: &mut Config, site: &Site, choice: DbChoice) -> anyhow::Result<()> {
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
    let site = site_at(cfg, folder)
        .ok_or_else(|| anyhow::anyhow!("Anvil doesn't see {} as a worktree", folder.display()))?;
    set_db(cfg, &site, choice)
}

/// The worktree Anvil serves from `folder` (one `add` just made, say).
pub fn site_at(cfg: &Config, folder: &Path) -> Option<Site> {
    let real = |p: &Path| crate::platform::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    crate::sites(cfg)
        .into_iter()
        .find(|s| s.worktree_of.is_some() && real(&s.path) == real(folder))
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
    let db = encoded(db);
    // A database in the query wins over the path: it names the worktree's too.
    let (query, fragment) =
        rest[tail..].split_at(rest[tail..].find('#').unwrap_or(rest.len() - tail));
    let query = query
        .split('&')
        .map(|p| match p.trim_start_matches('?').split_once('=') {
            Some((k, _)) if is_db_param(&decoded(&decoded(k, false), true)) => {
                format!("{}{k}={db}", if p.starts_with('?') { "?" } else { "" })
            }
            _ => p.to_string(),
        })
        .collect::<Vec<_>>()
        .join("&");
    let url = format!("{scheme}://{}/{db}{query}{fragment}", &rest[..host_end]);
    // A database it still names that isn't the worktree's (`?x=1%26database=shop`, a `&`
    // encoded): it can't be rewritten.
    url_databases(&url)
        .iter()
        .all(|d| d == &decoded(&db, false))
        .then_some(url)
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
    let main = crate::platform::canonicalize(main).unwrap_or_else(|_| main.to_path_buf());
    // Already in the worktree (its own `.env`; a worktree may sit inside main's folder).
    if real.starts_with(crate::platform::canonicalize(wt).unwrap_or_else(|_| wt.to_path_buf())) {
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
    crate::platform::canonicalize(file)
        .ok()
        .or_else(|| {
            Some(
                crate::platform::canonicalize(file.parent()?)
                    .ok()?
                    .join(file.file_name()?),
            )
        })
        .unwrap_or_else(|| file.to_path_buf())
}

/// What a worktree's processes get on top of their usual environment: its hooks' `keys`,
/// so they never reach the main app's database or Redis keys. Empty for anything but a
/// worktree. Set variables win over the worktree's `.env` (Laravel, dotenv, Next.js, Prisma).
/// `Err(reason)` when Anvil can't work out the worktree's own database: then nothing
/// of the worktree runs (`checked_env`, no PHP-FPM pool).
pub fn env(cfg: &Config, site: &Site) -> Result<Vec<(String, String)>, String> {
    env_with(cfg, site, &hooks(cfg, site).keys, false)
}

/// `template` with each `{name}` of `vars` filled in; `None` when one has no value.
fn fill(template: &str, vars: &[(&str, Option<&str>)]) -> Option<String> {
    let mut out = template.to_string();
    for (name, value) in vars {
        let p = format!("{{{name}}}");
        if out.contains(&p) {
            out = out.replace(&p, (*value)?);
        }
    }
    Some(out)
}

/// `env` with the keys `keys`, for a test run when `test` (its env files differ).
fn env_with(
    cfg: &Config,
    site: &Site,
    keys: &BTreeMap<String, String>,
    test: bool,
) -> Result<Vec<(String, String)>, String> {
    if site.worktree_of.is_none() {
        return Ok(Vec::new());
    }
    // Only the app's own layer takes database keys out (`resolve`): the messages say so.
    let app = site.worktree_of.as_deref().unwrap_or("main");
    // Symfony Runtime's `dotenv_overload`: its env files win over the process env, so
    // Anvil's keys can't keep it off main's database.
    let composer = fs::read_to_string(site.path.join("composer.json")).unwrap_or_default();
    let composer = serde_json::from_str::<serde_json::Value>(&composer).unwrap_or_default();
    // Set as PHP reads it: anything but false, null, 0, "", "0" (and an empty array) is on.
    let set = |option: &str| {
        use serde_json::Value;
        composer
            .pointer(&format!("/extra/runtime/{option}"))
            .is_some_and(|v| match v {
                Value::Null | Value::Bool(false) => false,
                Value::Number(n) => n.as_f64() != Some(0.0),
                Value::String(s) => !s.is_empty() && s != "0",
                Value::Array(a) => !a.is_empty(),
                _ => true,
            })
    };
    if set("dotenv_overload") {
        return Err("its composer.json sets extra.runtime.dotenv_overload, so its env files win over the worktree's own settings and could reach main's database: turn it off to run it".into());
    }
    // Its env files elsewhere: Anvil reads the ones at the root, so it can't tell their database.
    if set("dotenv_path") {
        return Err("its composer.json sets extra.runtime.dotenv_path, so its env files aren't the ones Anvil reads and could reach main's database: take it out to run it".into());
    }
    let mut env: Vec<(String, String)> = Vec::new();
    let db = own_db(cfg, site);
    let main = main_of(&site.path).ok_or("its main checkout wasn't found")?;
    // What its processes would load: main's `.env` (copied in), then its own files
    // over it, in the order its framework loads them (the last one wins).
    let symfony = site.kind == "symfony";
    let mut names = vec![base_env(&main, symfony).to_string()];
    names.extend(runtime_env(&site.path, symfony, test));
    let files: Vec<_> = names
        .iter()
        .enumerate()
        .map(|(i, f)| {
            crate::resources::dotenv_file(&if i == 0 { &main } else { &site.path }.join(f))
        })
        .collect();
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
    let (url, prefix) = (crate::site_url(cfg, site), format!("{db}_"));
    let port = site.port.map(|p| p.to_string());
    let vars = [
        ("db", Some(db.as_str())),
        ("url", Some(url.as_str())),
        ("prefix", Some(prefix.as_str())),
        ("port", port.as_deref()),
        ("name", Some(site.name.as_str())),
    ];
    for (key, template) in keys {
        if template != "{db}" {
            match fill(template, &vars).filter(|v| !v.is_empty()) {
                Some(v) => env.push((key.clone(), v)),
                // Left out, the worktree's `.env` (main's) would win: nothing of it runs.
                None if DB_KEYS.contains(&key.as_str()) => {
                    return Err(format!(
                        "{key} has no value from the worktree keys; give it one, or take it out of {app}'s own worktree keys, in its Settings tab under Worktrees, to share {app}'s database"
                    ));
                }
                // Named like a database (a Redis index, say): its own reason.
                None if names_db(key) => {
                    let empty = vars
                        .iter()
                        .find(|(n, v)| v.is_none() && template.contains(&format!("{{{n}}}")));
                    return Err(match empty {
                        Some((n, _)) => {
                            format!("{key} has no value: {{{n}}} has none for this worktree")
                        }
                        None => format!("{key} is empty in the worktree keys"),
                    });
                }
                // A placeholder without a value (`{port}` of an app Anvil serves) leaves it out.
                None => {}
            }
            continue;
        }
        // The worktree's own database, as `key` names it: a URL's database part, a SQLite
        // file in the worktree, or its name.
        let values: Vec<&String> = files
            .iter()
            .filter_map(|f| f.get(key))
            .filter(|v| !v.is_empty())
            .collect();
        if key.ends_with("URL")
            || values
                .iter()
                .any(|v| v.contains("://") || v.starts_with("file:"))
        {
            // Every one of them must be rewritable, whichever file wins.
            for v in values {
                with_db(v, &db, &main, &site.path)
                    .ok_or_else(|| format!("{key} uses a kind of database Anvil can't rewrite"))?;
            }
            if let Some(v) = last(key).and_then(|v| with_db(v, &db, &main, &site.path)) {
                env.push((key.clone(), v));
            }
        } else if server || (!sqlite && files.iter().any(|f| f.contains_key(key))) {
            // Laravel falls back to `laravel` without DB_DATABASE: never main's.
            env.push((key.clone(), db.clone()));
        } else if sqlite
            && let Some(file) = last(key).and_then(|f| sqlite_file(&main, &site.path, f))
        {
            env.push((key.clone(), file));
        }
    }
    // A database key the user set by hand to main's database: sharing it is taking the key out.
    let is_db = |k: &str| names_db(k) || keys.get(k).is_some_and(|t| t == "{db}");
    // A URL's every name (path and query), else the value itself.
    let mains = |v: &str| {
        let names = url_databases(v);
        if names.is_empty() {
            is_mains_database(site, v)
        } else {
            names.iter().any(|d| is_mains_database(site, d))
        }
    };
    // A database key Anvil doesn't set (Django's POSTGRES_DB, a second connection's
    // DB_LEGACY_DATABASE) left at main's value, in main's file of that name or as main's
    // database. Taking a DB_KEYS key out shares; a server's host, user or a number (a Redis
    // index, a port) isn't a database.
    let other = |k: &str| {
        names_db(k)
            && !DB_KEYS.contains(&k)
            && !keys.contains_key(k)
            && !["_HOST", "_USER", "_USERNAME", "_PASSWORD", "_ENGINE"]
                .iter()
                .any(|s| k.ends_with(s))
    };
    let named: std::collections::BTreeSet<&String> = files
        .iter()
        .flat_map(|f| f.keys())
        .filter(|k| other(k))
        .collect();
    for k in named {
        // The value its processes get, and the file it comes from.
        let Some((name, v)) = names
            .iter()
            .zip(&files)
            .rev()
            .find_map(|(n, f)| Some((n, f.get(k).filter(|v| !v.is_empty())?)))
        else {
            continue;
        };
        let theirs = crate::resources::dotenv_file(&main.join(name));
        let number = v.chars().all(|c| c.is_ascii_digit());
        if !number && !in_worktree(v) && (theirs.get(k) == Some(v) || mains(v)) {
            return Err(format!(
                "{k} in {name} is {app}'s value; give it one of its own in {app}'s worktree keys, in its Settings tab under Worktrees ({{db}} for the worktree's own)"
            ));
        }
    }
    if let Some((k, _)) = env.iter().find(|(k, v)| is_db(k) && mains(v)) {
        return Err(format!(
            "{k} names {app}'s database; take it out of {app}'s own worktree keys, in its Settings tab under Worktrees, to share that database"
        ));
    }
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

/// What `test_config` reads: the database keys and the connection (never main's database).
const TEST_KEYS: [&str; 4] = ["DB_CONNECTION", "DB_DATABASE", "DB_URL", "DATABASE_URL"];

/// A database setting in the project's test config.
#[derive(Debug)]
pub(crate) struct TestVar {
    key: &'static str,
    value: String,
    /// It wins over a variable that is set already: `<env force="true">`, `<server>`.
    forced: bool,
    from: &'static str,
}

/// The database settings the project's test config names: `<env>` and `<server>` (not
/// commented out) in the PHPUnit config PHPUnit picks (the first of phpunit.xml,
/// phpunit.dist.xml, phpunit.xml.dist), and `.env.testing`. Empty values are left out.
pub(crate) fn test_config(dir: &Path) -> Vec<TestVar> {
    let mut out = Vec::new();
    let xml_file = ["phpunit.xml", "phpunit.dist.xml", "phpunit.xml.dist"]
        .into_iter()
        .find(|f| dir.join(f).is_file());
    if let Some(from) = xml_file {
        let mut xml = fs::read_to_string(dir.join(from)).unwrap_or_default();
        while let Some(a) = xml.find("<!--") {
            let b = xml[a..].find("-->").map_or(xml.len(), |b| a + b + 3);
            xml.replace_range(a..b, "");
        }
        for part in xml.split('<').skip(1) {
            let server = part.starts_with("server");
            if !server && !part.starts_with("env") {
                continue;
            }
            let tag = &part[..part.find('>').unwrap_or(part.len())];
            let attr = |name: &str| {
                ['"', '\''].iter().find_map(|q| {
                    let at = tag.find(&format!(" {name}={q}"))? + name.len() + 3;
                    Some(tag[at..][..tag[at..].find(*q)?].to_string())
                })
            };
            let Some(key) = attr("name").and_then(|n| TEST_KEYS.into_iter().find(|k| *k == n))
            else {
                continue;
            };
            let Some(value) = attr("value").filter(|v| !v.is_empty()) else {
                continue;
            };
            out.push(TestVar {
                key,
                value,
                forced: server || attr("force").as_deref() == Some("true"),
                from,
            });
        }
    }
    let testing = crate::resources::dotenv_file(&dir.join(".env.testing"));
    for key in TEST_KEYS {
        if let Some(v) = testing.get(key).filter(|v| !v.is_empty()) {
            out.push(TestVar {
                key,
                value: v.clone(),
                forced: false,
                from: ".env.testing",
            });
        }
    }
    out
}

/// `env` checked against the project's test config, for every Anvil start path (`test`: a
/// test run). Anvil's database keys are never removed by it: a removed one lets the
/// worktree's `.env`, a copy of main's, win. A test run gets the test config's database when
/// none of it is main's (`:memory:` as SQLite), other keys keeping the worktree's own;
/// anything else gets the worktree's own. A test run gets the database keys even when the
/// user took them out of the hooks to share main's database. `Err` when a test config
/// setting naming main's would win over Anvil's (forced, or a key Anvil doesn't set).
pub fn run_env(cfg: &Config, site: &Site, test: bool) -> Result<Vec<(String, String)>, String> {
    if site.worktree_of.is_none() {
        return Ok(Vec::new());
    }
    let mut keys = hooks(cfg, site).keys;
    if test {
        // Whatever is stored (taken out, empty, another value): the worktree's own.
        for k in DB_KEYS {
            keys.insert(k.into(), "{db}".into());
        }
    }
    let mut env = env_with(cfg, site, &keys, test)?;
    let vars = test_config(&site.path);
    // Main's, or a URL Anvil can't read the database of.
    let mains = |v: &TestVar| match v.key {
        "DB_CONNECTION" => false,
        "DB_DATABASE" => is_mains_database(site, &v.value),
        _ => {
            let names = url_databases(&v.value);
            names.is_empty() || names.iter().any(|db| is_mains_database(site, db))
        }
    };
    let has = |env: &[(String, String)], key: &str| env.iter().any(|(k, _)| k == key);
    // Only test runs read `.env.testing`, ones Anvil doesn't tell apart too (`composer test`):
    // its URL Anvil doesn't set gets the worktree's own database, which wins over it there.
    // Not one the user took out to share main's database: that stays refused.
    for v in vars.iter().filter(|v| {
        v.from == ".env.testing" && v.key.ends_with("URL") && keys.contains_key(v.key) && mains(v)
    }) {
        if !has(&env, v.key) {
            let main = main_of(&site.path).ok_or("its main checkout wasn't found")?;
            let url =
                with_db(&v.value, &own_db(cfg, site), &main, &site.path).ok_or_else(|| {
                    format!(
                        "{}'s {} uses a kind of database Anvil can't rewrite",
                        v.from, v.key
                    )
                })?;
            env.push((v.key.into(), url));
        }
    }
    if let Some(v) = vars
        .iter()
        .find(|v| mains(v) && (v.forced || !has(&env, v.key)))
    {
        return Err(format!(
            "{}'s {} names {}'s database and wins over Anvil's: name a test database of its own there",
            v.from,
            v.key,
            site.worktree_of.as_deref().unwrap_or("main")
        ));
    }
    if !test || vars.iter().all(|v| v.key == "DB_CONNECTION") || vars.iter().any(mains) {
        return Ok(env);
    }
    let main = main_of(&site.path).ok_or("its main checkout wasn't found")?;
    let set = |env: &mut Vec<(String, String)>, key: &str, value: String| match env
        .iter_mut()
        .find(|(k, _)| k == key)
    {
        Some((_, v)) => *v = value,
        None => env.push((key.into(), value)),
    };
    let own_connection = vars.iter().any(|v| v.key == "DB_CONNECTION");
    for v in &vars {
        if v.key == "DB_DATABASE" && v.value == ":memory:" && !own_connection {
            set(&mut env, "DB_CONNECTION", "sqlite".into());
        }
        set(&mut env, v.key, v.value.clone());
    }
    // A URL's database wins over DB_DATABASE: Anvil's URLs point at the test database too.
    let db = vars
        .iter()
        .find(|v| v.key == "DB_DATABASE")
        .map(|v| v.value.clone());
    if let Some(db) = db.filter(|d| d != ":memory:")
        && vars
            .iter()
            .all(|v| v.key == "DB_DATABASE" || v.key == "DB_CONNECTION")
    {
        for key in ["DB_URL", "DATABASE_URL"] {
            if let Some(url) = env.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()) {
                let url = with_db(&url, &db, &main, &site.path)
                    .ok_or_else(|| format!("{key} uses a kind of database Anvil can't rewrite"))?;
                set(&mut env, key, url);
            }
        }
    }
    Ok(env)
}

/// `run_env`, or the error that refuses to start anything of a worktree it can't separate from main.
/// `run_env` for its web requests (PHP-FPM; Caddy shows the reason). Nothing until its
/// own database on an Anvil server is made: one of that name may be there and not be
/// Anvil's, which only `prepare` refuses.
pub fn web_env(cfg: &Config, site: &Site) -> Result<Vec<(String, String)>, String> {
    let env = run_env(cfg, site, false)?;
    if let Some((_, MainDb::Anvil { .. })) = pending(cfg, site) {
        return Err(format!(
            "{} isn't made yet; start {} in Anvil to make it",
            db_name(site),
            site.name
        ));
    }
    Ok(env)
}

pub fn checked_env(cfg: &Config, site: &Site, test: bool) -> anyhow::Result<Vec<(String, String)>> {
    run_env(cfg, site, test).map_err(|reason| {
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
                path: crate::platform::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()),
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
    let common = crate::platform::canonicalize(git_dir.join(common.trim())).ok()?;
    common.parent().map(Path::to_path_buf)
}

/// Create a worktree of `app` for `branch` (a new branch with `new_branch`) in
/// `<parent of the app>/<app>--<site name of the branch>`, and give it the files of its hooks'
/// `copy` (git ignores them, so a fresh checkout lacks them), with the hooks' `keys` written
/// into the env files among them. The app's own files are not touched. `copy_db` (unset: the
/// hooks' `copy_database`) is refused first when main's database can't be copied; the caller
/// records the choice returned (`set_db_at`). `after_create` isn't run: the caller asks first.
pub fn add(
    cfg: &Config,
    app: &Site,
    branch: &str,
    new_branch: bool,
    copy_db: Option<bool>,
) -> anyhow::Result<(PathBuf, DbChoice)> {
    // A linked app may have worktrees; an alias (a link to another app's folder) is that app.
    let real = |p: &Path| crate::platform::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let alias = app.linked
        && crate::sites(cfg)
            .iter()
            .any(|o| o.name != app.name && !o.linked && real(&o.path) == real(&app.path));
    if app.worktree_of.is_some() || alias {
        anyhow::bail!("{} isn't an app's own folder", app.name);
    }
    let offer = new_db(cfg, app);
    let copy = copy_db.unwrap_or(offer.copy);
    if copy && !offer.can_copy {
        anyhow::bail!(
            "A worktree of {} can't copy its database (main's database is on a server Anvil doesn't manage)",
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
    let db = if copy {
        DbChoice::Copy
    } else {
        DbChoice::Empty
    };
    let leave = left_out(cfg, &app.path, &folder, db);
    let copied = copy_in(&app.path, &folder, &hooks(cfg, app).copy, leave.as_deref())?;
    // Not worked out (a URL Anvil can't rewrite): the copy stays as made, and nothing of the
    // worktree runs until it is (`checked_env`).
    if let Some(site) = site_at(cfg, &folder) {
        let _ = write_keys(cfg, &site, &copied);
    }
    Ok((folder, db))
}

/// `list`'s entries that `main` has, relative: a `*` in the last part matches any run of
/// characters there; an entry outside the project is left out.
// ponytail: `*` only, in the last part; full gitignore globs if `.worktreeinclude`s need them.
fn expand(main: &Path, list: &[String]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for rel in list.iter().filter_map(|e| inside(e)) {
        let name = rel.file_name().unwrap_or_default().to_string_lossy();
        let found = if name.contains('*') {
            let dir = rel.parent().unwrap_or(Path::new(""));
            let mut names: Vec<PathBuf> = fs::read_dir(main.join(dir))
                .into_iter()
                .flatten()
                .flatten()
                .filter(|e| star(&name, &e.file_name().to_string_lossy()))
                .map(|e| dir.join(e.file_name()))
                .collect();
            names.sort();
            names
        } else {
            vec![rel.clone()]
        };
        for f in found {
            if main.join(&f).exists() && !out.contains(&f) {
                out.push(f);
            }
        }
    }
    out
}

/// Whether `name` matches `pattern`, whose `*`s match any run of characters.
fn star(pattern: &str, name: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if name.len() < first.len() + last.len() || !name.starts_with(first) || !name.ends_with(last) {
        return false;
    }
    let mut rest = &name[first.len()..name.len() - last.len()];
    for mid in &parts[1..parts.len() - 1] {
        match rest.find(mid) {
            Some(i) => rest = &rest[i + mid.len()..],
            None => return false,
        }
    }
    true
}

/// Copy `list`'s entries (`expand`) from `main` into `wt`, folders whole, never over what is
/// there: what was copied, relative. `leave`, a file of main's, is never copied (its SQLite
/// database when the worktree's is to be empty).
fn copy_in(
    main: &Path,
    wt: &Path,
    list: &[String],
    leave: Option<&Path>,
) -> anyhow::Result<Vec<PathBuf>> {
    fn copy_all(from: &Path, to: &Path, leave: Option<&PathBuf>) -> std::io::Result<()> {
        if leave == Some(&real_file(from)) {
            return Ok(());
        }
        if from.is_dir() {
            fs::create_dir_all(to)?;
            for e in fs::read_dir(from)?.flatten() {
                copy_all(&e.path(), &to.join(e.file_name()), leave)?;
            }
        } else if !to.exists() {
            if let Some(dir) = to.parent() {
                fs::create_dir_all(dir)?;
            }
            fs::copy(from, to)?;
        }
        Ok(())
    }
    let leave = leave.map(real_file);
    let todo: Vec<PathBuf> = expand(main, list)
        .into_iter()
        .filter(|f| !wt.join(f).exists() && leave != Some(real_file(&main.join(f))))
        .collect();
    for f in &todo {
        copy_all(&main.join(f), &wt.join(f), leave.as_ref())?;
    }
    Ok(todo)
}

/// Main's SQLite file when the worktree's database is to be `choice` Empty: `copy_in` leaves it out.
fn left_out(cfg: &Config, main: &Path, wt: &Path, choice: DbChoice) -> Option<PathBuf> {
    match main_db_at(cfg, main, wt) {
        MainDb::Sqlite { from, .. } if choice == DbChoice::Empty => Some(from),
        _ => None,
    }
}

/// The env files at the project's root that `env` reads as the processes' env, in Next.js'
/// order (the last one wins). Only these get keys written: never `.env.example`, `.env.testing`
/// or anything else a `.env*` matched.
const RUNTIME_ENV: [&str; 4] = [
    ".env",
    ".env.development",
    ".env.local",
    ".env.development.local",
];

/// `dir`'s first env file: `.env`, or for Symfony `.env.dist` when there's no `.env` (Dotenv
/// loads it in its place).
fn base_env(dir: &Path, symfony: bool) -> &'static str {
    if symfony && !dir.join(".env").is_file() && dir.join(".env.dist").is_file() {
        ".env.dist"
    } else {
        ".env"
    }
}

/// The env files at `dir`'s root a process loads, in its order (the last one wins): `RUNTIME_ENV`,
/// or Symfony's `.env`, `.env.local` (not for tests), `.env.<APP_ENV>`, `.env.<APP_ENV>.local`.
/// APP_ENV is `test` for a test run, else as `symfony_env_files` works it out. A test run
/// of anything else reads `.env.testing` last too: Laravel loads it with APP_ENV=testing.
fn runtime_env(dir: &Path, symfony: bool, test: bool) -> Vec<String> {
    if !symfony {
        let mut files = RUNTIME_ENV.map(String::from).to_vec();
        if test {
            files.push(".env.testing".into());
        }
        return files;
    }
    // ponytail: APP_ENV from Anvil's own env only; `bin/console --env=`, an inline
    // `APP_ENV=prod cmd` and per-process env aren't read. Read them if apps rely on them.
    symfony_env_files(dir, test, std::env::var("APP_ENV").ok())
}

/// Symfony's `Dotenv::loadEnv` order with APP_ENV `process` (the process env's, which wins),
/// else `.env`'s, then `.env.local`'s unless that says `test` (it skips `.env.local`), else `dev`.
fn symfony_env_files(dir: &Path, test: bool, process: Option<String>) -> Vec<String> {
    // Only a plain name: it becomes part of a file name.
    let plain = |v: String| {
        (!v.is_empty()
            && v.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        .then_some(v)
    };
    let from = |f: &str| {
        crate::resources::dotenv_file(&dir.join(f))
            .remove("APP_ENV")
            .and_then(plain)
    };
    let base = base_env(dir, true);
    let process = process.and_then(plain);
    let app_env = match (test, process) {
        (true, _) => "test".to_string(),
        (false, Some(p)) => p,
        (false, None) => match from(base).unwrap_or_else(|| "dev".into()) {
            e if e == "test" => e,
            e => from(".env.local").unwrap_or(e),
        },
    };
    let mut out = vec![base.to_string()];
    if app_env != "test" {
        out.push(".env.local".into());
    }
    if app_env != "local" {
        out.push(format!(".env.{app_env}"));
        out.push(format!(".env.{app_env}.local"));
    }
    out
}

/// Whether `f` (relative) is one of `site`'s env files its processes read outside test runs:
/// the ones Rewrite writes. Never a test env file.
fn is_env_file(site: &Site, f: &Path) -> bool {
    runtime_env(&site.path, site.kind == "symfony", false)
        .iter()
        .any(|r| f == Path::new(r) && !r.starts_with(".env.test") && !r.ends_with(".dist"))
}

/// `text` with each line setting one of `values`' keys (`KEY=…`, `export KEY=…`) set to its
/// value, its quotes kept, every other byte as it was; and the keys no line sets.
fn set_keys(text: &str, values: &[(String, String)]) -> (String, Vec<String>) {
    let mut seen = std::collections::BTreeSet::new();
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        let hit = body
            .split_once('=')
            .filter(|_| !body.trim_start().starts_with('#'))
            .and_then(|(k, old)| {
                let key = k.trim();
                let key = key.strip_prefix("export ").unwrap_or(key).trim();
                let (name, v) = values.iter().find(|(name, _)| name == key)?;
                Some((k, old, name, v))
            });
        let Some((k, old, name, v)) = hit else {
            out.push_str(line);
            continue;
        };
        seen.insert(name.clone());
        // The spacing after `=` and a ` # comment` after the value stay.
        let rest = old.trim_start();
        let lead = &old[..old.len() - rest.len()];
        let (quote, tail) = match rest.chars().next() {
            Some(q @ ('"' | '\'')) => (Some(q), rest[1..].find(q).map_or("", |i| &rest[i + 2..])),
            _ => {
                let value = rest[..rest.find(" #").unwrap_or(rest.len())].trim_end();
                (None, &rest[value.len()..])
            }
        };
        let v = match quote {
            Some(q) => format!("{q}{v}{q}"),
            None => quoted(v),
        };
        out.push_str(&format!("{k}={lead}{v}{tail}{}", &line[body.len()..]));
    }
    let missing = values
        .iter()
        .filter(|(k, _)| !seen.contains(k))
        .map(|(k, _)| k.clone())
        .collect();
    (out, missing)
}

/// `v` in double quotes when an env file would read it otherwise (spaces, `#`).
fn quoted(v: &str) -> String {
    if v.contains([' ', '\t', '#']) {
        format!("\"{v}\"")
    } else {
        v.into()
    }
}

/// `values` added at the end of `text`, with its line ending.
fn append_keys(text: &mut String, values: &[(String, String)]) {
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    if !text.is_empty() && !text.ends_with('\n') {
        text.push_str(eol);
    }
    for (k, v) in values {
        text.push_str(&format!("{k}={}{eol}", quoted(v)));
    }
}

/// Write the worktree's `keys` (as `env` works them out) into its env files among `files`
/// (relative): a line setting one changes, a key none sets is added to the first; nothing
/// else changes.
fn write_keys(cfg: &Config, site: &Site, files: &[PathBuf]) -> anyhow::Result<()> {
    let values = env(cfg, site).map_err(anyhow::Error::msg)?;
    let keys = hooks(cfg, site).keys;
    let (db, main) = (db_name(site), main_of(&site.path).unwrap_or_default());
    for (i, f) in files.iter().filter(|f| is_env_file(site, f)).enumerate() {
        let file = site.path.join(f);
        let text = fs::read_to_string(&file)?;
        // A `{db}` key gets this file's own value pointed at the worktree's database (its
        // host and options kept); the rest what the processes get.
        let own = crate::resources::dotenv_file(&file);
        let mine: Vec<(String, String)> = values
            .iter()
            .map(|(k, v)| {
                let from_file = own
                    .get(k)
                    .filter(|o| !o.is_empty() && keys.get(k).is_some_and(|t| t == "{db}"));
                let v = from_file
                    .and_then(|o| {
                        with_db(o, &db, &main, &site.path)
                            .filter(|_| o.contains("://") || o.starts_with("file:"))
                    })
                    .unwrap_or_else(|| v.clone());
                (k.clone(), v)
            })
            .collect();
        let (mut text, missing) = set_keys(&text, &mine);
        if i == 0 && !missing.is_empty() {
            let add: Vec<_> = mine
                .into_iter()
                .filter(|(k, _)| missing.contains(k))
                .collect();
            append_keys(&mut text, &add);
        }
        fs::write(&file, text)?;
    }
    Ok(())
}

/// The worktree's env files from its hooks' `copy`, relative: the ones main and it both have.
fn env_files(cfg: &Config, site: &Site) -> Vec<PathBuf> {
    let Some(main) = main_of(&site.path) else {
        return Vec::new();
    };
    expand(&main, &hooks(cfg, site).copy)
        .into_iter()
        .filter(|f| is_env_file(site, f) && site.path.join(f).is_file())
        .collect()
}

/// "Rewrite with the hooks": the hooks' `keys` into the worktree's env files (`write_keys`).
pub fn rewrite(cfg: &Config, site: &Site) -> anyhow::Result<()> {
    let files = env_files(cfg, site);
    if files.is_empty() {
        anyhow::bail!("{} has none of the env files its hooks copy", site.name);
    }
    write_keys(cfg, site, &files)
}

/// The hooks' `check` keys whose value in one of the worktree's env files is main's in
/// the same file. Unset or empty in either isn't the same.
pub fn same_as_main(cfg: &Config, site: &Site) -> Vec<String> {
    let Some(main) = main_of(&site.path) else {
        return Vec::new();
    };
    let check = hooks(cfg, site).check;
    let mut out: Vec<String> = Vec::new();
    for f in env_files(cfg, site) {
        let mine = crate::resources::dotenv_file(&site.path.join(&f));
        let theirs = crate::resources::dotenv_file(&main.join(&f));
        for k in &check {
            let same = mine
                .get(k)
                .is_some_and(|v| !v.is_empty() && theirs.get(k) == Some(v) && !in_worktree(v));
            if same && !out.contains(k) {
                out.push(k.clone());
            }
        }
    }
    out
}

/// A value that names a file by a relative path (`database/database.sqlite`, Prisma's
/// `file:./dev.db`): read from the worktree, it is the worktree's own file already.
fn in_worktree(v: &str) -> bool {
    let path = v.strip_prefix("file:").unwrap_or(v);
    let file = v.starts_with("file:")
        || path.contains('/')
        || path.ends_with(".sqlite")
        || path.ends_with(".db");
    file && !path.contains("://") && !Path::new(path).is_absolute()
}

/// `list` (`after_create`) for a worktree in `dir`: `{install}` is the JS install its lock
/// file asks for (`npm ci` with package-lock.json), a command with it left out without package.json.
fn commands(list: &[String], dir: &Path) -> Vec<String> {
    // `js_manager` says npm without a lock file too: then there's nothing to install from.
    let install = match crate::apps::js_manager(dir) {
        _ if !dir.join("package.json").is_file() => None,
        "npm" => dir
            .join("package-lock.json")
            .is_file()
            .then(|| "npm ci".to_string()),
        // pnpm 11 fails an install whose dependencies' build scripts it skipped (`js_install`).
        "pnpm" => Some("pnpm install --config.strict-dep-builds=false".into()),
        m => Some(format!("{m} install")),
    };
    list.iter()
        .filter_map(|c| match &install {
            _ if !c.contains("{install}") => Some(c.clone()),
            Some(i) => Some(c.replace("{install}", i)),
            None => None,
        })
        .collect()
}

/// The worktree's `after_create` commands, ready to run (`commands`).
pub fn after_create(cfg: &Config, site: &Site) -> Vec<String> {
    commands(&hooks(cfg, site).after_create, &site.path)
}

/// Run one of the worktree's `after_create` commands in it, with what its processes get
/// (Anvil's PHP and Node.js, its own database) and Anvil's shims first on PATH, so
/// `composer` is Anvil's. Only when the user said so; its database is prepared first by the
/// caller. `quiet`: the output is kept, its last lines in the error; else it shows.
pub fn run_after(cfg: &Config, site: &Site, command: &str, quiet: bool) -> anyhow::Result<()> {
    use std::process::Stdio;
    let mut env = crate::processes::env(cfg, site, false)?;
    let shims = crate::home().join("shims");
    if shims.is_dir()
        && let Some((_, path)) = env.iter_mut().find(|(k, _)| k == "PATH")
    {
        *path = crate::prepend_path(&shims, path);
    }
    let mut cmd = crate::platform::shell(command, None);
    cmd.envs(env).current_dir(&site.path).stdin(Stdio::null());
    if !quiet {
        if !cmd.status()?.success() {
            anyhow::bail!("{command} failed");
        }
        return Ok(());
    }
    let out = cmd.output()?;
    if !out.status.success() {
        let text = String::from_utf8_lossy(&out.stderr);
        let tail: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
        anyhow::bail!(
            "{command} failed: {}",
            tail[tail.len().saturating_sub(3)..].join("\n")
        );
    }
    Ok(())
}

/// What a worktree (one made outside Anvil, say) lacks to run: the files of its hooks'
/// `copy` that main has (git ignores them), and its packages: `(id, command)` for `install`.
#[derive(Serialize, Debug, Default, PartialEq)]
pub struct Missing {
    pub copy: Vec<String>,
    pub installs: Vec<(String, String)>,
}

pub fn missing(cfg: &Config, site: &Site) -> Missing {
    let (dir, mut out) = (&site.path, Missing::default());
    if let Some(main) = main_of(dir) {
        out.copy = expand(&main, &hooks(cfg, site).copy)
            .into_iter()
            .filter(|f| !dir.join(f).exists())
            .map(|f| f.display().to_string())
            .collect();
    }
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

/// Give the worktree the files of its hooks' `copy` it lacks (`missing`), with the hooks'
/// `keys` written into them, as a new worktree gets them; never over what it has.
pub fn copy_files(cfg: &Config, site: &Site) -> anyhow::Result<()> {
    let main =
        main_of(&site.path).ok_or_else(|| anyhow::anyhow!("{} isn't a git worktree", site.name))?;
    let leave = left_out(cfg, &main, &site.path, choice(cfg, site));
    let copied = copy_in(&main, &site.path, &hooks(cfg, site).copy, leave.as_deref())?;
    if copied.is_empty() {
        anyhow::bail!("{} has every file its hooks copy", site.name);
    }
    write_keys(cfg, site, &copied)
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
            let (sh, args) = crate::platform::shell_args(&install);
            (sh, args, env)
        }
        _ => anyhow::bail!("Anvil doesn't install {what}"),
    };
    let out = crate::platform::command(program)
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

/// What removing the worktree deletes that git can't bring back: files it ignores or
/// doesn't track that hold data, relative to the worktree. Its SQLite files (the one its
/// `.env` uses, any `*.sqlite`, `*.sqlite3`, `*.db` under `database/`) and its `.env` files
/// that main has no copy of or that set other values than main's, the hooks' keys aside
/// (Anvil writes those). What a build makes again (`vendor`, `node_modules`) isn't listed.
pub fn removal_losses(cfg: &Config, site: &Site) -> Vec<String> {
    let wt = &site.path;
    let args = [
        "status",
        "--porcelain",
        "-z",
        "--ignored=matching",
        "--untracked-files=all",
    ];
    let Ok(out) = crate::git::run(wt, &args) else {
        return Vec::new();
    };
    // `?? path` and `!! path` (a folder ends in `/` and stands for all in it).
    let loose: Vec<&str> = out
        .split('\0')
        .filter_map(|e| e.strip_prefix("?? ").or(e.strip_prefix("!! ")))
        .collect();
    let untracked = |rel: &str| {
        loose
            .iter()
            .any(|l| *l == rel || (l.ends_with('/') && rel.starts_with(l)))
    };
    let mut files: Vec<String> = Vec::new();
    if let MainDb::Sqlite { to, .. } = main_db(cfg, site)
        && let Ok(rel) = to.strip_prefix(wt)
    {
        files.push(rel.display().to_string());
    }
    let mut dirs = vec![wt.join("database")];
    while let Some(dir) = dirs.pop() {
        for e in fs::read_dir(&dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                dirs.push(p);
            } else if matches!(
                p.extension().and_then(|x| x.to_str()),
                Some("sqlite" | "sqlite3" | "db")
            ) && let Ok(rel) = p.strip_prefix(wt)
            {
                files.push(rel.display().to_string());
            }
        }
    }
    let main = main_of(wt);
    let keys = hooks(cfg, site).keys;
    let values = |f: &Path| {
        let mut v = crate::resources::dotenv_file(f);
        v.retain(|k, _| !keys.contains_key(k));
        v
    };
    let mut envs: Vec<String> = fs::read_dir(wt)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n == ".env" || n.starts_with(".env."))
        .filter(|n| {
            let theirs = main.as_ref().map(|m| m.join(n)).filter(|f| f.is_file());
            theirs.is_none_or(|t| values(&wt.join(n)) != values(&t))
        })
        .collect();
    envs.sort();
    files.extend(envs);
    let mut out: Vec<String> = Vec::new();
    for f in files {
        if wt.join(&f).is_file() && untracked(&f) && !out.contains(&f) {
            out.push(f);
        }
    }
    out
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

    pub(crate) fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("anvil-wt-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        crate::platform::canonicalize(&d).unwrap()
    }

    pub(crate) fn git(dir: &Path, args: &[&str]) {
        let ok = crate::platform::command("git")
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

    /// `shop--feature-x`, a worktree of the Laravel app `shop` whose main checkout has
    /// `dotenv` as its `.env`: its keys are the Laravel preset's.
    pub(crate) fn worktree_site(name: &str, dotenv: &str) -> (Config, Site) {
        let (main, x) = repo(name);
        fs::write(main.join(".env"), dotenv).unwrap();
        let mut site = crate::tests::site("shop--feature-x", x.to_str().unwrap(), Some("8.4"));
        site.kind = "laravel".into();
        site.path = x;
        site.secure = false;
        site.worktree_of = Some("shop".into());
        let cfg = Config {
            http_port: 80,
            // "Copy the database too" on: most tests here are about the copy.
            worktree_db: DbChoice::Copy,
            ..Config::default()
        };
        (cfg, site)
    }

    pub(crate) fn get<'a>(env: &'a [(String, String)], k: &str) -> Option<&'a str> {
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
    fn an_unfillable_key_says_which_placeholder_has_no_value() {
        let (cfg, mut site) =
            worktree_site("unfillable", "DB_CONNECTION=mysql\nDB_DATABASE=shop\n");
        site.port = None;
        let mut keys = shipped("laravel").keys.unwrap();
        // A Redis index named like a database: not a database key, no "share" offered.
        keys.insert("REDIS_DB".into(), "{port}".into());
        let err = env_with(&cfg, &site, &keys, false).unwrap_err();
        assert_eq!(
            err,
            "REDIS_DB has no value: {port} has none for this worktree"
        );
        keys.insert("REDIS_DB".into(), "".into());
        let err = env_with(&cfg, &site, &keys, false).unwrap_err();
        assert_eq!(err, "REDIS_DB is empty in the worktree keys");
        keys.remove("REDIS_DB");
        keys.insert("DB_DATABASE".into(), "{port}".into());
        let err = env_with(&cfg, &site, &keys, false).unwrap_err();
        assert!(
            err == "DB_DATABASE has no value from the worktree keys; give it one, or take it out of shop's own worktree keys, in its Settings tab under Worktrees, to share shop's database",
            "{err}"
        );
    }

    #[test]
    fn only_the_apps_own_keys_can_leave_the_database_keys_out() {
        let (mut cfg, site) = worktree_site("team-keys", "DB_CONNECTION=mysql\nDB_DATABASE=shop\n");
        let main = main_of(&site.path).unwrap();
        let url_only = BTreeMap::from([("APP_URL".to_string(), "{url}".to_string())]);
        let db_keys_are_the_worktrees = |cfg: &Config, layer: &str| {
            let resolved = hooks(cfg, &site);
            // Said, in the app's Settings tab and `anvil worktree hooks`.
            assert!(
                resolved.problems.contains(&format!(
                    "{layer}: keys leave out DB_DATABASE, DB_URL, DATABASE_URL, so the layers below give them; only the app's own worktree keys can take them out"
                )),
                "{:?}",
                resolved.problems
            );
            let keys = resolved.keys;
            assert_eq!(keys.get("APP_URL").map(String::as_str), Some("{url}"));
            for k in DB_KEYS {
                assert_eq!(keys.get(k).map(String::as_str), Some("{db}"), "{k}");
            }
            // Only the database keys come back.
            assert!(!keys.contains_key("REDIS_PREFIX"));
            let env = env(cfg, &site).unwrap();
            assert_eq!(get(&env, "DB_DATABASE"), Some("shop_feature_x"));
        };
        // A team's `.anvil.toml` leaving them out.
        fs::write(
            main.join(".anvil.toml"),
            "[worktree]\nkeys = { APP_URL = \"{url}\" }\n",
        )
        .unwrap();
        db_keys_are_the_worktrees(&cfg, ".anvil.toml");
        // A preset edited that way.
        fs::remove_file(main.join(".anvil.toml")).unwrap();
        cfg.worktree_presets.insert(
            "laravel".into(),
            Hooks {
                keys: Some(url_only.clone()),
                ..Hooks::default()
            },
        );
        db_keys_are_the_worktrees(&cfg, "Laravel preset");
        // The user's own app settings can: the worktree shares main's database.
        cfg.sites.entry("shop".into()).or_default().worktree.keys = Some(url_only);
        let resolved = hooks(&cfg, &site);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        let keys = resolved.keys;
        assert!(DB_KEYS.iter().all(|k| !keys.contains_key(*k)), "{keys:?}");
        assert_eq!(get(&env(&cfg, &site).unwrap(), "DB_DATABASE"), None);
    }

    #[test]
    fn a_url_query_is_decoded_twice_as_laravel_and_doctrine_do() {
        // `rawurldecode` of the query, then `parse_str`.
        for url in [
            "mysql://r@h/app?data%2562ase=shop",
            "mysql://r@h/app?x=1%26database=shop",
            "mysql://r@h/app?database%3Dshop",
            "mysql://r@h/app?database=sh%256Fp",
        ] {
            assert_eq!(url_database(url).as_deref(), Some("shop"), "{url}");
        }
        // Main's `.env`: rewritten where Anvil can, else nothing runs.
        let (cfg, site) = worktree_site(
            "url-twice",
            "DB_CONNECTION=mysql\nDB_URL=mysql://root@127.0.0.1/app?data%2562ase=shop\n",
        );
        assert_eq!(
            get(&env(&cfg, &site).unwrap(), "DB_URL"),
            Some("mysql://root@127.0.0.1/shop_feature_x?data%2562ase=shop_feature_x")
        );
        let main = main_of(&site.path).unwrap();
        fs::write(
            main.join(".env"),
            "DB_CONNECTION=mysql\nDB_URL=mysql://root@127.0.0.1/app?x=1%26database=shop\n",
        )
        .unwrap();
        assert!(env(&cfg, &site).is_err());
        // A team value naming main's twice-encoded: refused.
        fs::write(main.join(".env"), "DB_CONNECTION=mysql\nDB_DATABASE=shop\n").unwrap();
        fs::write(
            main.join(".anvil.toml"),
            "[worktree]\nkeys = { DB_URL = \"mysql://root@127.0.0.1/x?database=sh%256Fp\" }\n",
        )
        .unwrap();
        let err = env(&cfg, &site).unwrap_err();
        assert!(err.contains("names shop's database"), "{err}");
    }

    #[test]
    fn a_percent_encoded_database_is_read_decoded() {
        assert_eq!(url_database("mysql://r@h/sh%6Fp").as_deref(), Some("shop"));
        assert_eq!(
            url_database("mysql://r@h/?data%62ase=sh%6Fp").as_deref(),
            Some("shop")
        );
        // Main's `.env`: the encoded parameter is rewritten too.
        let (cfg, site) = worktree_site(
            "url-encoded",
            "DB_CONNECTION=mysql\nDB_URL=mysql://root@127.0.0.1/?data%62ase=shop\n",
        );
        assert_eq!(
            get(&env(&cfg, &site).unwrap(), "DB_URL"),
            Some("mysql://root@127.0.0.1/shop_feature_x?data%62ase=shop_feature_x")
        );
        assert!(is_mains_database(&site, "shop"));
        // A team `.anvil.toml` naming main's encoded: refused.
        let main = main_of(&site.path).unwrap();
        fs::write(
            main.join(".anvil.toml"),
            "[worktree]\nkeys = { DB_URL = \"mysql://root@127.0.0.1/sh%6Fp\" }\n",
        )
        .unwrap();
        let err = env(&cfg, &site).unwrap_err();
        assert!(err.contains("names shop's database"), "{err}");
        // What is written is encoded: a name with a space (a hand-picked one) stays one value.
        assert_eq!(
            with_db("mysql://h/x?db=y", "a b", &main, &site.path).as_deref(),
            Some("mysql://h/a%20b?db=a%20b")
        );
    }

    #[test]
    fn a_database_in_the_url_query_is_the_worktrees_too() {
        // Laravel's `?database=` and Doctrine's `?dbname=` win over the path.
        assert_eq!(
            url_database("mysql://r@h/?database=shop").as_deref(),
            Some("shop")
        );
        assert_eq!(
            url_database("mysql://r@h/a?x=1&DBName=shop").as_deref(),
            Some("shop")
        );
        // mysqlclient's `db` (Django through dj-database-url, SQLAlchemy).
        assert_eq!(
            url_database("mysql://r@h:3306/app?db=shop").as_deref(),
            Some("shop")
        );
        assert_eq!(
            with_db(
                "mysql://r@h:3306/app?db=shop",
                "w",
                Path::new("/m"),
                Path::new("/w")
            )
            .as_deref(),
            Some("mysql://r@h:3306/w?db=w")
        );
        let (cfg, site) = worktree_site(
            "url-query",
            "DB_CONNECTION=mysql\nDB_URL=mysql://root@127.0.0.1/?database=shop\nDATABASE_URL=mysql://r@h:3306/x?charset=utf8&dbname=shop\n",
        );
        let env = env(&cfg, &site).unwrap();
        assert_eq!(
            get(&env, "DB_URL"),
            Some("mysql://root@127.0.0.1/shop_feature_x?database=shop_feature_x")
        );
        assert_eq!(
            get(&env, "DATABASE_URL"),
            Some("mysql://r@h:3306/shop_feature_x?charset=utf8&dbname=shop_feature_x")
        );
        // Main's named in either place: refused.
        for url in [
            "mysql://r@h/shop_feature_x?database=shop",
            "mysql://r@h/shop?database=shop_feature_x",
        ] {
            let mut keys = shipped("laravel").keys.unwrap();
            keys.insert("DB_URL".into(), url.into());
            let err = env_with(&cfg, &site, &keys, false).unwrap_err();
            assert!(err.contains("names shop's database"), "{url}: {err}");
        }
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
        // "Copy the database too" is off unless the user turned it on.
        let unset: Config = toml::from_str("tld = \"test\"\n").unwrap();
        assert_eq!(unset.worktree_db, DbChoice::Empty);
        // `worktree_db = "copy"` was written to every config, chosen or not: read as unset,
        // and gone from the file at its next save.
        let old: Config = toml::from_str("worktree_db = \"copy\"\n").unwrap();
        assert_eq!(old.worktree_db, DbChoice::Empty);
        assert!(!toml::to_string(&old).unwrap().contains("worktree_d"));
        // Turned on from now on: stored, and kept.
        let cfg = Config {
            worktree_db: DbChoice::Copy,
            ..Config::default()
        };
        let saved = toml::to_string(&cfg).unwrap();
        assert!(saved.contains("worktree_database = \"copy\""), "{saved}");
        let back: Config = toml::from_str(&saved).unwrap();
        assert_eq!(back.worktree_db, DbChoice::Copy);
    }

    #[test]
    fn no_database_settings_only_the_address_and_prefixes() {
        let (cfg, site) = worktree_site("nodb", "APP_NAME=Shop\n");
        let keys: Vec<_> = env(&cfg, &site)
            .unwrap()
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert_eq!(keys, ["APP_URL", "CACHE_PREFIX", "REDIS_PREFIX"]);
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
        let err = checked_env(&cfg, &site, false).unwrap_err().to_string();
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

    pub(crate) fn with_postgres(mut cfg: Config) -> Config {
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
        // The copy is of the database the app uses: a URL's query one wins over its path.
        let (cfg, site) = worktree_site(
            "copy-query",
            "DATABASE_URL=postgresql://u@127.0.0.1:5433/other?dbname=sh%6Fp\n",
        );
        assert_eq!(
            main_db(&with_postgres(cfg), &site),
            MainDb::Anvil {
                service: "postgres-x".into(),
                database: "shop".into()
            }
        );
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
    fn db_names_of_two_apps_never_clash() {
        // App shop-a's worktree x and app shop's worktree a-x; a `.` reads as a `-`.
        let names = [
            ("shop-a", "x"),
            ("shop", "a-x"),
            ("shop", "v1.2"),
            ("shop", "v1-2"),
        ];
        let dbs: Vec<String> = names
            .iter()
            .map(|(app, rest)| {
                let mut s = crate::tests::site(&joined(app, rest), "/w", None);
                s.worktree_of = Some(app.to_string());
                db_name(&s)
            })
            .collect();
        for (i, a) in dbs.iter().enumerate() {
            assert!(crate::databases::check_db(a).is_ok(), "{a}");
            assert!(!dbs[i + 1..].contains(a), "{dbs:?}");
        }
        assert_eq!(dbs[1], "shop_a_x", "the usual name stays as it was");
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
        assert_eq!(l[0].path, crate::platform::canonicalize(&x).unwrap());
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
    fn removal_losses_names_ignored_data_not_what_a_build_makes() {
        let main_env = "APP_URL=http://shop.test\nDB_CONNECTION=sqlite\nMAIL_HOST=main\n";
        let (cfg, site) = worktree_site("losses", main_env);
        let x = &site.path;
        fs::write(x.join(".gitignore"), ".env\n/vendor\n/node_modules\n").unwrap();
        fs::create_dir_all(x.join("database")).unwrap();
        fs::write(x.join("database/.gitignore"), "*.sqlite*\n").unwrap();
        fs::write(x.join(".env.example"), "edited=1\n").unwrap();
        git(x, &["add", "."]);
        git(x, &["commit", "-q", "-m", "ignores"]);
        fs::write(x.join("database/database.sqlite"), "data").unwrap();
        for f in ["vendor/autoload.php", "node_modules/x/index.js"] {
            fs::create_dir_all(x.join(f).parent().unwrap()).unwrap();
            fs::write(x.join(f), "").unwrap();
        }
        // Only the keys Anvil writes differ: nothing of the user's is in it.
        let anvils = "APP_URL=http://shop--feature-x.test\nDB_CONNECTION=sqlite\nMAIL_HOST=main\nDB_DATABASE=x\n";
        fs::write(x.join(".env"), anvils).unwrap();
        assert_eq!(removal_losses(&cfg, &site), ["database/database.sqlite"]);
        fs::write(x.join(".env"), anvils.replace("=main", "=edited")).unwrap();
        assert_eq!(
            removal_losses(&cfg, &site),
            ["database/database.sqlite", ".env"]
        );
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
        let (folder, _) = add(&cfg, &app, "feature/login", true, None).unwrap();
        assert_eq!(folder, root.join("shop--feature-login"));
        assert_eq!(fs::read(folder.join(".env")).unwrap(), b"A=1\n");
        assert_eq!(fs::read(shop.join(".env")).unwrap(), b"A=1\n");
        // An existing branch, and a folder already there.
        git(&shop, &["branch", "old"]);
        assert!(
            add(&cfg, &app, "old", false, None)
                .unwrap()
                .0
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
        let (cfg, site) = worktree_site("missing", "A=1\nREDIS_PREFIX=shop_\n");
        assert_eq!(
            missing(&cfg, &site),
            Missing {
                copy: vec![".env".into()],
                installs: vec![]
            }
        );
        fs::write(site.path.join("composer.json"), "{}").unwrap();
        fs::write(site.path.join("package.json"), "{}").unwrap();
        fs::write(site.path.join("pnpm-lock.yaml"), "").unwrap();
        let ids: Vec<_> = missing(&cfg, &site)
            .installs
            .into_iter()
            .map(|(_, c)| c)
            .collect();
        assert_eq!(ids, ["composer install", "pnpm install"]);
        // Anvil made the copy: the hooks' keys go into it, the rest as main has it.
        copy_files(&cfg, &site).unwrap();
        assert_eq!(
            fs::read_to_string(site.path.join(".env")).unwrap(),
            "A=1\nREDIS_PREFIX=shop_feature_x_\nAPP_URL=http://shop--feature-x.test\nCACHE_PREFIX=shop_feature_x_\n"
        );
        assert!(missing(&cfg, &site).copy.is_empty());
        assert!(copy_files(&cfg, &site).is_err(), "never over its own");
        fs::create_dir(site.path.join("vendor")).unwrap();
        fs::create_dir(site.path.join("node_modules")).unwrap();
        assert_eq!(missing(&cfg, &site), Missing::default());
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
    fn a_test_run_takes_the_test_configs_own_database_by_replacing_anvils() {
        let (cfg, site) = worktree_site("t-own", "DB_CONNECTION=pgsql\nDB_DATABASE=shop\n");
        let get = |env: &[(String, String)], k: &str| super::tests::get(env, k).map(String::from);
        // Laravel's default phpunit.xml on a server-database app.
        fs::write(
            site.path.join("phpunit.xml"),
            r#"<php><env name="DB_CONNECTION" value="sqlite"/><env name="DB_DATABASE" value=":memory:"/></php>"#,
        )
        .unwrap();
        let test = checked_env(&cfg, &site, true).unwrap();
        assert_eq!(get(&test, "DB_CONNECTION").as_deref(), Some("sqlite"));
        assert_eq!(get(&test, "DB_DATABASE").as_deref(), Some(":memory:"));
        let other = checked_env(&cfg, &site, false).unwrap();
        assert_eq!(
            get(&other, "DB_DATABASE").as_deref(),
            Some("shop_feature_x")
        );
        // A connection of its own for `:memory:` is passed through, not made `sqlite`.
        fs::write(
            site.path.join("phpunit.xml"),
            r#"<php><env name="DB_CONNECTION" value="testing"/><env name="DB_DATABASE" value=":memory:"/></php>"#,
        )
        .unwrap();
        let test = checked_env(&cfg, &site, true).unwrap();
        assert_eq!(get(&test, "DB_CONNECTION").as_deref(), Some("testing"));
        assert_eq!(get(&test, "DB_DATABASE").as_deref(), Some(":memory:"));
        fs::remove_file(site.path.join("phpunit.xml")).unwrap();
        // Its own name, and `.env`'s URL pointed at it rather than dropped.
        fs::write(
            site.path.join(".env"),
            "DB_CONNECTION=pgsql\nDB_URL=pgsql://u@127.0.0.1:5433/shop\n",
        )
        .unwrap();
        fs::write(site.path.join(".env.testing"), "DB_DATABASE=shop_testing\n").unwrap();
        let test = checked_env(&cfg, &site, true).unwrap();
        assert_eq!(get(&test, "DB_DATABASE").as_deref(), Some("shop_testing"));
        assert_eq!(
            get(&test, "DB_URL").as_deref(),
            Some("pgsql://u@127.0.0.1:5433/shop_testing")
        );
        // Main's in it: the worktree's own, keys all set.
        fs::write(site.path.join(".env.testing"), "DB_DATABASE=shop\n").unwrap();
        let test = checked_env(&cfg, &site, true).unwrap();
        assert_eq!(get(&test, "DB_DATABASE").as_deref(), Some("shop_feature_x"));
        assert_eq!(
            get(&test, "DB_URL").as_deref(),
            Some("pgsql://u@127.0.0.1:5433/shop_feature_x")
        );
    }

    #[test]
    fn the_phpunit_config_phpunit_uses_is_read_and_a_winning_mains_refuses_every_run() {
        let (cfg, site) = worktree_site("t-dist", "DB_CONNECTION=pgsql\nDB_DATABASE=shop\n");
        fs::write(
            site.path.join("phpunit.dist.xml"),
            r#"<php><server name="DB_DATABASE" value="shop"/></php>"#,
        )
        .unwrap();
        let e = checked_env(&cfg, &site, false).unwrap_err().to_string();
        assert_eq!(
            e,
            "Anvil can't point shop--feature-x at its own database (phpunit.dist.xml's DB_DATABASE names shop's database and wins over Anvil's: name a test database of its own there), so nothing of it runs"
        );
        // A phpunit.xml there is the one PHPUnit uses.
        fs::write(site.path.join("phpunit.xml"), "<php></php>").unwrap();
        assert!(checked_env(&cfg, &site, false).is_ok());
        // Empty values aren't settings.
        fs::write(
            site.path.join("phpunit.xml"),
            r#"<php><server name="DB_URL" value=""/></php>"#,
        )
        .unwrap();
        assert!(checked_env(&cfg, &site, true).is_ok());
    }

    #[test]
    fn whatever_the_config_anvils_database_is_never_mains() {
        // `{MAIN}`: the main checkout's folder. Each with every database main could use,
        // written out here rather than asked of `is_mains_database`.
        let mains: [(&str, &[&str]); 17] = [
            // Encoded twice, which the query is decoded.
            (
                "DB_CONNECTION=mysql\nDB_URL=mysql://root@127.0.0.1/app?data%2562ase=shop\n",
                &["shop", "app", "laravel"],
            ),
            // Percent-encoded, as Laravel and Doctrine decode it.
            (
                "DB_CONNECTION=mysql\nDB_URL=mysql://root@127.0.0.1/sh%6Fp?data%62ase=shop\n",
                &["shop", "laravel"],
            ),
            // mysqlclient's `?db=` (the Python preset's stacks).
            (
                "DATABASE_URL=mysql://root@127.0.0.1:3306/app?db=shop\n",
                &["shop", "app"],
            ),
            ("DB_CONNECTION=pgsql\nDB_DATABASE=shop\n", &["shop"]),
            // The database in the query (Laravel's, Doctrine's), which wins over the path.
            (
                "DB_CONNECTION=mysql\nDB_URL=mysql://root@127.0.0.1/?database=shop\n",
                &["shop", "laravel"],
            ),
            (
                "DATABASE_URL=mysql://root@127.0.0.1:3306/other?dbname=shop\n",
                &["shop", "other"],
            ),
            (
                "DB_CONNECTION=mysql\nDB_URL=mysql://r@127.0.0.1/shop\n",
                &["shop", "laravel"],
            ),
            ("DATABASE_URL=postgres://u@127.0.0.1:5433/shop\n", &["shop"]),
            ("DB_DATABASE=shop\n", &["shop"]),
            (
                "DB_CONNECTION=pgsql\nDB_DATABASE=shop\nDB_URL=pgsql://u@h/shop\n",
                &["shop"],
            ),
            // A URL naming another database than DB_DATABASE, in both orders.
            (
                "DB_CONNECTION=pgsql\nDB_DATABASE=shop\nDATABASE_URL=postgres://h/other\n",
                &["shop", "other"],
            ),
            (
                "DATABASE_URL=postgres://h/other\nDB_CONNECTION=pgsql\nDB_DATABASE=shop\n",
                &["shop", "other"],
            ),
            (
                "DB_DATABASE=shop\nDB_URL=mysql://r@h/other\nDB_CONNECTION=mysql\n",
                &["shop", "other"],
            ),
            // Main's DB_DATABASE is the worktree's own name (`shop_feature_x`).
            (
                "DB_CONNECTION=pgsql\nDATABASE_URL=postgres://h/other\nDB_DATABASE=shop_feature_x\n",
                &["shop_feature_x", "other"],
            ),
            ("DB_CONNECTION=sqlite\n", &[]),
            (
                "DB_CONNECTION=sqlite\nDB_DATABASE={MAIN}/database/database.sqlite\n",
                &["{MAIN}/database/database.sqlite"],
            ),
            (
                "DB_DATABASE={MAIN}/database/database.sqlite\nDB_CONNECTION=sqlite\n",
                &["{MAIN}/database/database.sqlite"],
            ),
        ];
        let configs = [
            ("", ""),
            (".env.testing", "DB_DATABASE=shop\n"),
            (".env.testing", "DB_DATABASE=other\n"),
            (".env.testing", "DB_DATABASE=shop_feature_x\n"),
            (".env.testing", "DB_DATABASE=shop_testing\n"),
            (
                ".env.testing",
                "DB_DATABASE={MAIN}/database/database.sqlite\n",
            ),
            (
                ".env.testing",
                "DATABASE_URL=postgres://u@127.0.0.1:5433/shop_t\n",
            ),
            (".env.testing", "DB_URL=mysql://r@127.0.0.1/shop\n"),
            (
                "phpunit.xml",
                r#"<env name="DB_CONNECTION" value="sqlite"/><env name="DB_DATABASE" value=":memory:"/>"#,
            ),
            (
                "phpunit.xml",
                r#"<env name="DB_URL" value="mysql://r@127.0.0.1/shop"/>"#,
            ),
            (
                "phpunit.xml",
                r#"<env name="DB_DATABASE" value="shop" force="true"/>"#,
            ),
            (
                "phpunit.xml",
                r#"<env name="DB_DATABASE" value="other" force="true"/>"#,
            ),
            (
                "phpunit.xml",
                r#"<env name="DB_CONNECTION" value="testing"/><env name="DB_DATABASE" value=":memory:"/>"#,
            ),
            (
                "phpunit.dist.xml",
                r#"<server name="DATABASE_URL" value="postgres://h/shop"/>"#,
            ),
            (
                "phpunit.xml.dist",
                r#"<env name="DB_DATABASE" value="shop_t"/>"#,
            ),
        ];
        // Every database a value names (a URL's path and query).
        let db_of = |k: &str, v: &str| match k {
            "DB_DATABASE" => Some(vec![v.to_string()]),
            "DB_URL" | "DATABASE_URL" => Some(url_databases(v)),
            _ => None,
        };
        let cases = mains.iter().enumerate().flat_map(|(n, m)| {
            [
                "laravel",
                "symfony",
                "symfony-test",
                "symfony-overload",
                "symfony-dist",
            ]
            .map(|k| (n, m, k))
        });
        for (n, (dotenv, names), symfony) in cases {
            let (mut cfg, mut site) = worktree_site(&format!("prop-{n}-{symfony}"), "");
            let main = main_of(&site.path).unwrap();
            let fill = |t: &str| t.replace("{MAIN}", &main.display().to_string());
            let both = |f: &str, text: &str| {
                fs::write(main.join(f), text).unwrap();
                fs::write(site.path.join(f), text).unwrap();
            };
            match symfony {
                // Symfony: main's settings in the committed `.env.dev`, and `.env.test` for tests.
                "symfony" => {
                    site.kind = "symfony".into();
                    both(".env.dev", &fill(dotenv));
                    both(".env.test", &fill(dotenv));
                }
                // `.env` says test, so Symfony skips `.env.local` (which says dev): only in `.env.test`.
                "symfony-test" => {
                    site.kind = "symfony".into();
                    both(".env", "APP_ENV=test\n");
                    fs::write(site.path.join(".env.local"), "APP_ENV=dev\n").unwrap();
                    both(".env.test", &fill(dotenv));
                }
                // Its env files win over the process env: nothing of it runs.
                "symfony-overload" => {
                    site.kind = "symfony".into();
                    both(".env.dev", &fill(dotenv));
                    both(
                        "composer.json",
                        r#"{"extra": {"runtime": {"dotenv_overload": true}}}"#,
                    );
                }
                // Only a committed `.env.dist`, which Symfony loads without a `.env`.
                "symfony-dist" => {
                    site.kind = "symfony".into();
                    fs::remove_file(main.join(".env")).unwrap();
                    both(".env.dist", &fill(dotenv));
                }
                _ => fs::write(main.join(".env"), fill(dotenv)).unwrap(),
            }
            // Main's: one of its names, its SQLite file by canonical path, or a file in its
            // checkout (the worktree is next to it, not inside).
            let canon = |p: &str| {
                let p = Path::new(p);
                let dir = crate::platform::canonicalize(p.parent().unwrap())
                    .unwrap_or(p.parent().unwrap().into());
                dir.join(p.file_name().unwrap_or_default())
            };
            let is_mains = |db: &str| {
                names.iter().any(|m| fill(m) == db)
                    || (Path::new(db).is_absolute()
                        && (canon(db).starts_with(&main)
                            || names.iter().any(|m| {
                                Path::new(&fill(m)).is_absolute() && canon(&fill(m)) == canon(db)
                            })))
            };
            // What it gets with no test config and the preset's keys: its own.
            let own = checked_env(&cfg, &site, false).unwrap_or_else(|e| {
                assert_eq!(symfony, "symfony-overload", "{e}");
                Vec::new()
            });
            // The worktree keys as the user may leave them: the preset's, DB_DATABASE empty, one
            // whose placeholder has no value, main's name typed in, or taken out (sharing).
            for variant in ["preset", "", "{port}", "shop", "removed"] {
                let mut keys = shipped("laravel").keys.unwrap();
                match variant {
                    "preset" => {}
                    "removed" => {
                        for k in DB_KEYS {
                            keys.remove(k);
                        }
                    }
                    v => {
                        keys.insert("DB_DATABASE".into(), v.into());
                    }
                }
                cfg.sites.entry("shop".into()).or_default().worktree.keys = Some(keys);
                for (file, text) in configs {
                    for f in [
                        ".env.testing",
                        "phpunit.xml",
                        "phpunit.dist.xml",
                        "phpunit.xml.dist",
                    ] {
                        let _ = fs::remove_file(site.path.join(f));
                    }
                    if !file.is_empty() {
                        fs::write(site.path.join(file), fill(text)).unwrap();
                    }
                    for test in [false, true] {
                        let case = format!(
                            "{dotenv:?} symfony={symfony} {file} {text:?} keys={variant:?} test={test}"
                        );
                        let got = checked_env(&cfg, &site, test);
                        if symfony == "symfony-overload" {
                            assert!(got.is_err(), "{case}");
                        }
                        // A DB key with no value never lets `.env` (main's) win: nothing runs.
                        if !test && ["", "{port}"].contains(&variant) {
                            assert!(got.is_err(), "{case}");
                        }
                        let Ok(env) = got else {
                            continue; // refused: nothing runs
                        };
                        // Anvil's database keys are never removed, except by taking them out, and a
                        // test run gets them even then.
                        if test || variant != "removed" {
                            for (k, _) in own.iter().filter(|(k, v)| db_of(k, v).is_some()) {
                                assert!(env.iter().any(|(key, _)| key == k), "{k} unset: {case}");
                            }
                            // Main names a database: a database key is set, so no file's wins.
                            if !names.is_empty() {
                                assert!(
                                    env.iter().any(|(k, _)| DB_KEYS.contains(&k.as_str())),
                                    "no database key set: {case}"
                                );
                            }
                        } else {
                            assert!(
                                env.iter().all(|(k, _)| !DB_KEYS.contains(&k.as_str())),
                                "{case}"
                            );
                        }
                        for (k, v) in &env {
                            let Some(dbs) = db_of(k, v) else { continue };
                            // A relative SQLite name resolves inside the worktree.
                            assert!(!dbs.iter().any(|db| is_mains(db)), "{k}={v}: {case}");
                            // Not a test run (or one misdetected): only ever its own; a URL
                            // only `.env.testing` names, set to the worktree's own database.
                            if !test && variant == "preset" {
                                match get(&own, k) {
                                    Some(o) => assert_eq!(v, o, "{case}"),
                                    None => assert!(
                                        file == ".env.testing" && dbs == ["shop_feature_x"],
                                        "{k}={v}: {case}"
                                    ),
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn mains_own_database_is_known_by_name() {
        let (_, site) = worktree_site("is-mains", "DB_CONNECTION=pgsql\nDB_DATABASE=shop\n");
        assert!(is_mains_database(&site, "shop"));
        // MySQL on macOS takes names in any case.
        assert!(is_mains_database(&site, "SHOP"));
        assert!(is_mains_database(&site, " Shop "));
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
    fn the_checkbox_off_makes_an_empty_database_on_a_copy() {
        let (mut cfg, app) = parked_app("add-db", "DB_CONNECTION=pgsql\nDB_PORT=5433\n");
        // Anvil's default: off.
        let new = new_db(&cfg, &app);
        assert!(new.can_copy && !new.copy);
        assert_eq!(new.main_database.as_deref(), Some("laravel"));
        let srv = Server::default();
        for (branch, copy_db, want) in [
            ("off", None, "create shop_off"),
            ("on", Some(true), "clone laravel shop_on"),
        ] {
            let (folder, choice) = add(&cfg, &app, branch, true, copy_db).unwrap();
            set_db_at(&mut cfg, &folder, choice).unwrap();
            let site = site_at(&cfg, &folder).unwrap();
            let (made, log) = prep(&mut cfg, &site, &srv).unwrap();
            assert!(made);
            assert_eq!(log.last().unwrap(), &format!("postgres-x {want}"));
        }
        // Settings (or a preset, or the app) turned on: the checkbox starts on.
        cfg.worktree_db = DbChoice::Copy;
        assert!(new_db(&cfg, &app).copy);
        cfg.sites
            .entry("shop".into())
            .or_default()
            .worktree
            .copy_database = Some(false);
        assert!(!new_db(&cfg, &app).copy);
    }

    #[test]
    fn a_new_worktree_refuses_a_choice_main_doesnt_allow() {
        let (mut cfg, app) = parked_app("add-ext", "DB_CONNECTION=mysql\nDB_HOST=db.example.com\n");
        cfg.worktree_db = DbChoice::Copy;
        let new = new_db(&cfg, &app);
        // Copy, Settings' default, isn't offered.
        assert!(!new.can_copy && !new.copy);
        let e = add(&cfg, &app, "feat", true, Some(true)).unwrap_err();
        assert_eq!(
            e.to_string(),
            "A worktree of shop can't copy its database (main's database is on a server Anvil doesn't manage)"
        );
        assert!(!app.path.with_file_name("shop--feat").exists());
        // No database settings: nothing to choose.
        let (cfg, app) = parked_app("add-none", "APP_NAME=Shop\n");
        assert!(new_db(&cfg, &app).main_database.is_none());
    }

    #[test]
    fn an_empty_choice_leaves_main_s_sqlite_file_out_of_the_copy() {
        let (mut cfg, app) = parked_app("add-sqlite", "DB_CONNECTION=sqlite\n");
        fs::create_dir(app.path.join("database")).unwrap();
        fs::write(app.path.join("database/database.sqlite"), "main's data").unwrap();
        fs::write(app.path.join("database/seed.sqlite"), "seed").unwrap();
        cfg.sites.entry("shop".into()).or_default().worktree.copy =
            Some(vec![".env".into(), "database/*.sqlite".into()]);
        let (folder, choice) = add(&cfg, &app, "empty", true, Some(false)).unwrap();
        assert!(!folder.join("database/database.sqlite").exists());
        assert!(folder.join("database/seed.sqlite").exists());
        set_db_at(&mut cfg, &folder, choice).unwrap();
        let site = site_at(&cfg, &folder).unwrap();
        assert!(
            pending(&cfg, &site).is_some(),
            "an empty one is still to make"
        );
        // The same for the files a worktree gets later.
        fs::remove_dir_all(folder.join("database")).unwrap();
        copy_files(&cfg, &site).unwrap();
        assert!(!folder.join("database/database.sqlite").exists());
        // Copy chosen: the hooks may bring it.
        let (folder, _) = add(&cfg, &app, "copy", true, Some(true)).unwrap();
        assert!(folder.join("database/database.sqlite").exists());
    }

    #[test]
    fn add_refuses_an_alias_and_a_worktree() {
        let (mut cfg, app) = parked_app("add-alias", "APP_NAME=Shop\n");
        cfg.links.insert("admin".into(), app.path.clone());
        let all = crate::sites(&cfg);
        let alias = all.iter().find(|s| s.name == "admin").unwrap();
        let e = add(&cfg, alias, "b", true, None).unwrap_err();
        assert_eq!(e.to_string(), "admin isn't an app's own folder");
        let mut wt = app.clone();
        wt.worktree_of = Some("shop".into());
        assert!(add(&cfg, &wt, "b", true, None).is_err());
        assert!(!app.path.with_file_name("admin--b").exists());
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

    #[test]
    fn the_shipped_presets_parse_and_cover_every_kind() {
        for (id, _, _) in PRESETS {
            let h = shipped(id);
            assert!(
                h.copy.is_some()
                    && h.keys.is_some()
                    && h.check.is_some()
                    && h.after_create.is_some(),
                "{id}"
            );
            assert_eq!(h.copy_database, None, "{id}: left to Settings");
            assert_eq!(check_hooks(&h), Ok(()), "{id}");
            // No preset shares main's database: only the user taking the keys out does.
            for k in DB_KEYS {
                assert_eq!(
                    h.keys.as_ref().unwrap().get(k).map(String::as_str),
                    Some("{db}"),
                    "{id} {k}"
                );
            }
        }
        assert_eq!(preset_of("vite"), "node");
        assert_eq!(preset_of("flask"), "python");
        assert_eq!(preset_of("go"), "other");
        let laravel = shipped("laravel");
        assert_eq!(laravel.keys.unwrap()["REDIS_PREFIX"], "{prefix}");
        assert_eq!(shipped("symfony").copy.unwrap(), [".env.local"]);
    }

    #[test]
    fn each_field_comes_from_the_first_layer_that_sets_it() {
        let (main, _) = repo("layers");
        let r = |cfg: &Config| resolve(cfg, "shop", &main, "laravel");
        let mut cfg = Config::default();
        let all = r(&cfg);
        assert_eq!(all.preset, "Laravel");
        for f in ["copy", "keys", "check", "after_create"] {
            assert_eq!(all.from[f], "Laravel preset", "{f}");
        }
        assert_eq!(
            (all.copy_database, all.from["copy_database"].as_str()),
            (false, "Settings")
        );
        // Settings › Worktrees: the preset edited, and the database checkbox.
        cfg.worktree_db = DbChoice::Copy;
        cfg.worktree_presets.insert(
            "laravel".into(),
            Hooks {
                check: Some(vec!["APP_KEY".into()]),
                ..Hooks::default()
            },
        );
        assert!(r(&cfg).copy_database);
        assert_eq!(r(&cfg).check, ["APP_KEY"]);
        assert_eq!(
            r(&cfg).after_create,
            shipped("laravel").after_create.unwrap()
        );
        // `.worktreeinclude` beats the preset's copy; `.anvil.toml` beats both.
        fs::write(
            main.join(".worktreeinclude"),
            "# secrets\n.env\nconfig/local.php\n!x\n",
        )
        .unwrap();
        assert_eq!(r(&cfg).copy, [".env", "config/local.php"]);
        assert_eq!(r(&cfg).from["copy"], ".worktreeinclude");
        fs::write(
            main.join(".anvil.toml"),
            "[worktree]\ncopy = [\".env.team\"]\ncopy_database = false\n",
        )
        .unwrap();
        assert_eq!(r(&cfg).copy, [".env.team"]);
        assert_eq!(
            (
                r(&cfg).copy_database,
                r(&cfg).from["copy_database"].as_str()
            ),
            (false, ".anvil.toml")
        );
        // The app's own settings beat everything, field by field.
        cfg.sites.entry("shop".into()).or_default().worktree.copy = Some(vec![]);
        assert!(r(&cfg).copy.is_empty());
        assert_eq!(r(&cfg).from["copy"], "this app");
        assert_eq!(r(&cfg).from["keys"], "Laravel preset");
        // A `[worktree]` with a mistake is the file's error, not silently ignored.
        fs::write(main.join(".anvil.toml"), "[worktree]\ncopy = [\"../x\"]\n").unwrap();
        assert_eq!(
            crate::project::error(&main).unwrap(),
            ".anvil.toml: [worktree] \"../x\" isn't a path inside the project"
        );
    }

    #[test]
    fn a_hooks_typo_is_that_apps_error_not_the_configs() {
        let text = "[sites.shop.worktree]\ncopyy = [\".env\"]\ncheck = 5\nkeys = { APP_URL = \"{url}\" }\n";
        let cfg: Config = toml::from_str(text).expect("the rest of the config still loads");
        let own = &cfg.sites["shop"].worktree;
        assert_eq!(own.keys.as_ref().unwrap()["APP_URL"], "{url}");
        assert_eq!(own.check, None, "a value of the wrong kind isn't used");
        // Kept as written, so a save doesn't lose it.
        let saved = toml::to_string(&cfg).unwrap();
        assert!(
            saved.contains("copyy") && saved.contains("check = 5"),
            "{saved}"
        );
        let (main, _) = repo("typo");
        let r = resolve(&cfg, "shop", &main, "laravel");
        assert_eq!(
            r.problems,
            [
                "this app: check isn't a hooks field, or its value isn't the right kind",
                "this app: copyy isn't a hooks field, or its value isn't the right kind"
            ]
        );
        assert_eq!(r.from["check"], "Laravel preset");
    }

    #[test]
    fn a_form_sets_one_field_checked() {
        let mut h = Hooks::default();
        set_field(
            &mut h,
            "keys",
            Some(serde_json::json!({ "APP_URL": "{url}/x" })),
        )
        .unwrap();
        assert_eq!(h.keys.as_ref().unwrap()["APP_URL"], "{url}/x");
        let e = set_field(&mut h, "keys", Some(serde_json::json!({ "A": "{nope}" })));
        assert_eq!(
            e.unwrap_err().to_string(),
            "A: {nope} isn't one of {db}, {url}, {prefix}, {port}, {name}"
        );
        assert!(set_field(&mut h, "keys", Some(serde_json::json!({ "A B": "x" }))).is_err());
        // A key left empty (a form line with no `=`) isn't taken out: refused, saying how.
        let e = set_field(
            &mut h,
            "keys",
            Some(serde_json::json!({ "DB_DATABASE": "" })),
        );
        assert_eq!(
            e.unwrap_err().to_string(),
            "DB_DATABASE has no value: give it one, or take the key out (taking a database key out shares main's database)"
        );
        assert!(set_field(&mut h, "colour", Some(serde_json::json!(1))).is_err());
        assert!(h.keys.is_some(), "a refused change changes nothing");
        set_field(&mut h, "keys", None).unwrap();
        assert!(h.is_empty());
    }

    #[test]
    fn keys_fill_their_placeholders_and_point_urls_at_the_worktrees_database() {
        let dotenv =
            "DB_CONNECTION=pgsql\nDB_DATABASE=shop\nPRISMA_URL=postgres://u@h:5433/shop?x=1\n";
        let (mut cfg, mut site) = worktree_site("fill", dotenv);
        site.port = Some(5173);
        let keys = [
            ("APP_NAME", "Shop {name}"),
            ("VITE_PORT", "{port}"),
            ("SESSION_PREFIX", "{prefix}s"),
            ("LITERAL", "x"),
            ("PRISMA_URL", "{db}"),
            ("NAME", "{db}"),
        ];
        cfg.sites.entry("shop".into()).or_default().worktree.keys = Some(
            keys.iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        );
        let env = env(&cfg, &site).unwrap();
        assert_eq!(get(&env, "APP_NAME"), Some("Shop shop--feature-x"));
        assert_eq!(get(&env, "VITE_PORT"), Some("5173"));
        assert_eq!(get(&env, "SESSION_PREFIX"), Some("shop_feature_x_s"));
        assert_eq!(get(&env, "LITERAL"), Some("x"));
        assert_eq!(
            get(&env, "PRISMA_URL"),
            Some("postgres://u@h:5433/shop_feature_x?x=1")
        );
        // A server connection: a database name key gets the worktree's own name.
        assert_eq!(get(&env, "NAME"), Some("shop_feature_x"));
        // Only the listed keys: DB_DATABASE was taken out.
        assert_eq!(get(&env, "DB_DATABASE"), None);
        // No port to give: the key is left out.
        site.port = None;
        assert_eq!(get(&super::env(&cfg, &site).unwrap(), "VITE_PORT"), None);
    }

    #[test]
    fn a_key_naming_mains_database_refuses_but_taking_the_keys_out_shares() {
        let dotenv = "DB_CONNECTION=pgsql\nDB_DATABASE=shop\n";
        let (mut cfg, site) = worktree_site("share", dotenv);
        fs::write(site.path.join(".env"), dotenv).unwrap();
        let mut keys = shipped("laravel").keys.unwrap();
        keys.insert("DB_DATABASE".into(), "shop".into());
        cfg.sites.entry("shop".into()).or_default().worktree.keys = Some(keys.clone());
        assert_eq!(
            env_err(&cfg, &site),
            "DB_DATABASE names shop's database; take it out of shop's own worktree keys, in its Settings tab under Worktrees, to share that database"
        );
        // Any key naming a database, not only Laravel's.
        let mut other = shipped("laravel").keys.unwrap();
        other.insert("PGDATABASE".into(), "shop".into());
        cfg.sites.entry("shop".into()).or_default().worktree.keys = Some(other);
        assert!(env_err(&cfg, &site).starts_with("PGDATABASE names shop's database"));
        // Taken out: main's database by the user's choice, and `check` says so.
        for k in DB_KEYS {
            keys.remove(k);
        }
        cfg.sites.entry("shop".into()).or_default().worktree.keys = Some(keys);
        let env = checked_env(&cfg, &site, false).unwrap();
        assert_eq!(get(&env, "DB_DATABASE"), None);
        assert_eq!(same_as_main(&cfg, &site), ["DB_DATABASE"]);
        // A test run still never gets main's: its own database.
        let test = checked_env(&cfg, &site, true).unwrap();
        assert_eq!(get(&test, "DB_DATABASE"), Some("shop_feature_x"));
    }

    #[test]
    fn a_database_key_anvil_doesnt_set_refuses_main_s_value() {
        // Django's names, a second Laravel connection: the copied `.env` gives main's.
        let dotenv = "DB_CONNECTION=pgsql\nDB_DATABASE=shop\nPOSTGRES_DB=shop\nREDIS_DB=0\nDATABASE_HOST=127.0.0.1\nDATABASE_USER=shop\n";
        let (mut cfg, site) = worktree_site("other-db-keys", dotenv);
        fs::write(site.path.join(".env"), dotenv).unwrap();
        assert_eq!(
            env_err(&cfg, &site),
            "POSTGRES_DB in .env is shop's value; give it one of its own in shop's worktree keys, in its Settings tab under Worktrees ({db} for the worktree's own)"
        );
        // A value of its own in the worktree's file passes; main's in another file refuses.
        let mine = dotenv.replace("POSTGRES_DB=shop", "POSTGRES_DB=shop_x");
        fs::write(site.path.join(".env"), &mine).unwrap();
        assert!(env(&cfg, &site).is_ok());
        let main = main_of(&site.path).unwrap();
        fs::write(main.join(".env.local"), "DB_LEGACY_DATABASE=shop_old\n").unwrap();
        fs::write(
            site.path.join(".env.local"),
            "DB_LEGACY_DATABASE=shop_old\n",
        )
        .unwrap();
        assert!(
            env_err(&cfg, &site).starts_with("DB_LEGACY_DATABASE in .env.local is shop's value")
        );
        // Given a value in the keys: the worktree's own.
        let mut keys = shipped("laravel").keys.unwrap();
        keys.insert("DB_LEGACY_DATABASE".into(), "{db}".into());
        cfg.sites.entry("shop".into()).or_default().worktree.keys = Some(keys);
        let env = env(&cfg, &site).unwrap();
        assert_eq!(get(&env, "DB_LEGACY_DATABASE"), Some("shop_feature_x"));
    }

    #[test]
    fn copy_takes_worktreeinclude_and_stars_and_never_overwrites() {
        let (main, wt) = repo("copy");
        fs::write(main.join(".env"), "A=1\n").unwrap();
        fs::write(main.join(".env.local"), "B=1\n").unwrap();
        fs::write(main.join(".envrc"), "x").unwrap();
        fs::create_dir_all(main.join("config/local")).unwrap();
        fs::write(main.join("config/local/keys.json"), "{}").unwrap();
        fs::write(wt.join(".env.local"), "mine").unwrap();
        fs::write(
            main.join(".worktreeinclude"),
            ".env*\nconfig/local/\n../outside\nnot-there\n",
        )
        .unwrap();
        let list = worktreeinclude(&main).unwrap();
        let copied = copy_in(&main, &wt, &list, None).unwrap();
        let shown: Vec<_> = copied.iter().map(|p| p.display().to_string()).collect();
        assert_eq!(shown, [".env", ".envrc", "config/local"]);
        assert_eq!(
            fs::read_to_string(wt.join("config/local/keys.json")).unwrap(),
            "{}"
        );
        assert_eq!(fs::read_to_string(wt.join(".env.local")).unwrap(), "mine");
        assert!(copy_in(&main, &wt, &list, None).unwrap().is_empty());
        assert!(star("a*b*c", "aXbYc") && !star("a*b*c", "aXc") && star(".env*", ".env"));
    }

    #[test]
    fn rewrite_changes_only_the_listed_keys() {
        let text = "# main's\nDB_DATABASE=shop\nexport APP_URL=\"http://shop.test\"\nOTHER = keep me \r\nDB_DATABASE=again\n#DB_DATABASE=shop\nno newline";
        let values = [
            ("DB_DATABASE".to_string(), "shop_x".to_string()),
            ("APP_URL".into(), "http://shop--x.test".into()),
            ("REDIS_PREFIX".into(), "shop_x_".into()),
        ];
        let (out, missing) = set_keys(text, &values);
        assert_eq!(
            out,
            "# main's\nDB_DATABASE=shop_x\nexport APP_URL=\"http://shop--x.test\"\nOTHER = keep me \r\nDB_DATABASE=shop_x\n#DB_DATABASE=shop\nno newline"
        );
        assert_eq!(missing, ["REDIS_PREFIX"]);
        // Through the worktree's files: `check` warns while a value is main's, not after.
        let dotenv = "APP_NAME=Shop\nDB_CONNECTION=pgsql\nDB_DATABASE=shop\nSESSION_COOKIE=s\nREDIS_PREFIX=\n";
        let (cfg, site) = worktree_site("rewrite", dotenv);
        assert!(rewrite(&cfg, &site).is_err(), "no .env to rewrite");
        fs::write(site.path.join(".env"), dotenv).unwrap();
        assert_eq!(same_as_main(&cfg, &site), ["DB_DATABASE"]);
        rewrite(&cfg, &site).unwrap();
        assert_eq!(
            fs::read_to_string(site.path.join(".env")).unwrap(),
            "APP_NAME=Shop\nDB_CONNECTION=pgsql\nDB_DATABASE=shop_feature_x\nSESSION_COOKIE=s\nREDIS_PREFIX=shop_feature_x_\nAPP_URL=http://shop--feature-x.test\nCACHE_PREFIX=shop_feature_x_\n"
        );
        // Rewritten: nothing of main's left to warn about (a cookie belongs to its host anyway).
        assert!(same_as_main(&cfg, &site).is_empty());
    }

    #[test]
    fn check_leaves_out_values_that_resolve_inside_the_worktree() {
        let dotenv = "DB_CONNECTION=sqlite\nDB_DATABASE=database/database.sqlite\nDATABASE_URL=file:./dev.db\nREDIS_PREFIX=shop_\n";
        let (cfg, site) = worktree_site("same-rel", dotenv);
        fs::write(site.path.join(".env"), dotenv).unwrap();
        // The relative SQLite file and Prisma's `file:` are the worktree's own already.
        assert_eq!(same_as_main(&cfg, &site), ["REDIS_PREFIX"]);
        let abs = format!(
            "DATABASE_URL=file:{}/dev.db\n",
            main_of(&site.path).unwrap().display()
        );
        fs::write(main_of(&site.path).unwrap().join(".env"), &abs).unwrap();
        fs::write(site.path.join(".env"), &abs).unwrap();
        assert_eq!(
            same_as_main(&cfg, &site),
            ["DATABASE_URL"],
            "main's file by its path"
        );
    }

    #[test]
    fn rewrite_keeps_comments_spacing_line_endings_and_each_files_own_value() {
        let values = [
            ("A".to_string(), "new".to_string()),
            ("B".into(), "new".into()),
            ("C".into(), "new".into()),
            ("D".into(), "two words".into()),
        ];
        let (out, missing) = set_keys("A=old # main db\r\nB = old\nC=\"old\" # q\n", &values);
        assert_eq!(out, "A=new # main db\r\nB = new\nC=\"new\" # q\n");
        assert_eq!(missing, ["D"]);
        // Added to a CRLF file with its line ending, quoted like a replaced value.
        let mut text = "X=1\r\n".to_string();
        append_keys(&mut text, &values[3..]);
        assert_eq!(text, "X=1\r\nD=\"two words\"\r\n");
        // Each file keeps its own host: only the database part is the worktree's.
        let dotenv = "DATABASE_URL=postgres://u@localhost/shop\n";
        let (cfg, site) = worktree_site("own-value", dotenv);
        let main = main_of(&site.path).unwrap();
        let local = "DATABASE_URL=postgres://u@staging/shop_staging\n";
        fs::write(main.join(".worktreeinclude"), ".env\n.env.local\n").unwrap();
        fs::write(main.join(".env.local"), local).unwrap();
        fs::write(site.path.join(".env"), dotenv).unwrap();
        fs::write(site.path.join(".env.local"), local).unwrap();
        rewrite(&cfg, &site).unwrap();
        let read = |f: &str| fs::read_to_string(site.path.join(f)).unwrap();
        assert!(
            read(".env").starts_with("DATABASE_URL=postgres://u@localhost/shop_feature_x\n"),
            "{}",
            read(".env")
        );
        assert!(
            read(".env.local").starts_with("DATABASE_URL=postgres://u@staging/shop_feature_x\n"),
            "{}",
            read(".env.local")
        );
    }

    #[test]
    fn symfonys_app_env_comes_from_the_process_then_env_then_env_local() {
        let dir = tmp("symfony-app-env");
        let files =
            |process: Option<&str>| symfony_env_files(&dir, false, process.map(String::from));
        assert_eq!(
            files(None),
            [".env", ".env.local", ".env.dev", ".env.dev.local"]
        );
        // `.env` says test: `.env.local` is skipped, whatever it says.
        fs::write(dir.join(".env"), "APP_ENV=test\n").unwrap();
        fs::write(dir.join(".env.local"), "APP_ENV=dev\n").unwrap();
        assert_eq!(files(None), [".env", ".env.test", ".env.test.local"]);
        // `.env.local` over `.env` otherwise.
        fs::write(dir.join(".env"), "APP_ENV=prod\n").unwrap();
        assert_eq!(
            files(None),
            [".env", ".env.local", ".env.dev", ".env.dev.local"]
        );
        // The process env wins over both.
        assert_eq!(
            files(Some("staging")),
            [".env", ".env.local", ".env.staging", ".env.staging.local"]
        );
        assert_eq!(
            symfony_env_files(&dir, true, Some("prod".into())),
            [".env", ".env.test", ".env.test.local"]
        );
    }

    #[test]
    fn mains_database_named_in_any_of_its_env_files_is_mains() {
        let (_, site) = worktree_site("every-env", "DB_CONNECTION=pgsql\nDB_DATABASE=shop\n");
        let main = main_of(&site.path).unwrap();
        fs::write(
            main.join(".env.prod"),
            "DATABASE_URL=postgresql://h/shop_prod\n",
        )
        .unwrap();
        fs::write(
            main.join(".env.test"),
            "DATABASE_URL=postgresql://h/shop_test\n",
        )
        .unwrap();
        fs::write(main.join(".env.example"), "DB_DATABASE=example\n").unwrap();
        fs::write(main.join(".env.dist"), "DB_DATABASE=dist\n").unwrap();
        for db in ["shop", "shop_prod", "shop_test"] {
            assert!(is_mains_database(&site, db), "{db}");
        }
        for db in ["example", "dist"] {
            assert!(!is_mains_database(&site, db), "{db}");
        }
    }

    #[test]
    fn symfonys_dotenv_overload_stops_the_worktree() {
        let (cfg, mut site) = worktree_site("overload", "DATABASE_URL=postgresql://h/shop\n");
        site.kind = "symfony".into();
        let composer = |option: &str, value: &str| {
            fs::write(
                site.path.join("composer.json"),
                format!(r#"{{"extra": {{"runtime": {{"{option}": {value}}}}}}}"#),
            )
            .unwrap()
        };
        // PHP's falsy values leave it off.
        for off in ["false", "null", "0", r#""""#, r#""0""#] {
            composer("dotenv_overload", off);
            assert!(env(&cfg, &site).is_ok(), "{off}");
        }
        for on in ["true", r#""true""#, "1", r#""1""#] {
            composer("dotenv_overload", on);
            for test in [false, true] {
                let err = run_env(&cfg, &site, test).unwrap_err();
                assert!(err.contains("extra.runtime.dotenv_overload"), "{on}: {err}");
            }
        }
        // Env files moved (`config/.env`): Anvil reads the root ones only.
        composer("dotenv_path", r#""config/.env""#);
        let err = env(&cfg, &site).unwrap_err();
        assert!(err.contains("extra.runtime.dotenv_path"), "{err}");
    }

    #[test]
    fn a_laravel_test_run_reads_env_testing_and_points_its_url_at_the_worktree() {
        // The re-review's layout: `.env` names DB_DATABASE, the committed `.env.testing` a DB_URL.
        let (cfg, site) = worktree_site("env-testing", "DB_CONNECTION=mysql\nDB_DATABASE=shop\n");
        let main = main_of(&site.path).unwrap();
        let testing = "DB_URL=mysql://root@127.0.0.1/shop_testing\n";
        fs::write(main.join(".env.testing"), testing).unwrap();
        fs::write(site.path.join(".env.testing"), testing).unwrap();
        // `php artisan test` runs, on the worktree's own database.
        let env = checked_env(&cfg, &site, true).unwrap();
        assert_eq!(
            get(&env, "DB_URL"),
            Some("mysql://root@127.0.0.1/shop_feature_x")
        );
        assert_eq!(get(&env, "DB_DATABASE"), Some("shop_feature_x"));
        // Not a test run: `.env.testing` isn't read, but its DB_URL (main's test database) would
        // win in a test run Anvil doesn't tell apart (`composer test`): set to the worktree's own.
        let env = checked_env(&cfg, &site, false).unwrap();
        assert_eq!(
            get(&env, "DB_URL"),
            Some("mysql://root@127.0.0.1/shop_feature_x")
        );
        assert_eq!(get(&env, "DB_DATABASE"), Some("shop_feature_x"));
        // One Anvil can't rewrite: refused.
        let other = "DB_URL=mongodb://root@127.0.0.1/shop_testing\n";
        fs::write(main.join(".env.testing"), other).unwrap();
        fs::write(site.path.join(".env.testing"), other).unwrap();
        assert_eq!(
            run_env(&cfg, &site, false).unwrap_err(),
            ".env.testing's DB_URL uses a kind of database Anvil can't rewrite"
        );
    }

    #[test]
    fn a_symfony_env_dist_stands_in_for_a_missing_env() {
        let (cfg, mut site) = worktree_site("env-dist", "");
        site.kind = "symfony".into();
        let main = main_of(&site.path).unwrap();
        fs::remove_file(main.join(".env")).unwrap();
        let both = |f: &str, text: &str| {
            fs::write(main.join(f), text).unwrap();
            fs::write(site.path.join(f), text).unwrap();
        };
        both(
            ".env.dist",
            "DATABASE_URL=postgresql://app@127.0.0.1:5432/shop\n",
        );
        let own = Some("postgresql://app@127.0.0.1:5432/shop_feature_x");
        assert_eq!(get(&env(&cfg, &site).unwrap(), "DATABASE_URL"), own);
        assert_eq!(
            get(&run_env(&cfg, &site, true).unwrap(), "DATABASE_URL"),
            own
        );
        assert!(is_mains_database(&site, "shop"));
        // Its APP_ENV picks the files, as `.env`'s would.
        both(".env.dist", "APP_ENV=staging\n");
        both(".env.staging", "DATABASE_URL=postgresql://st@h/shop\n");
        assert_eq!(
            get(&env(&cfg, &site).unwrap(), "DATABASE_URL"),
            Some("postgresql://st@h/shop_feature_x")
        );
        // Rewrite never writes the committed file.
        both(
            ".env.dist",
            "DATABASE_URL=postgresql://app@127.0.0.1:5432/shop\n",
        );
        fs::write(main.join(".worktreeinclude"), ".env*\n").unwrap();
        let _ = rewrite(&cfg, &site);
        assert_eq!(
            fs::read_to_string(site.path.join(".env.dist")).unwrap(),
            "DATABASE_URL=postgresql://app@127.0.0.1:5432/shop\n"
        );
        // A `.env` wins: `.env.dist` isn't read then (nor its APP_ENV, so not `.env.staging`).
        both(".env.dist", "APP_ENV=staging\n");
        both(".env", "DATABASE_URL=postgresql://real@h/shop\n");
        assert_eq!(
            get(&env(&cfg, &site).unwrap(), "DATABASE_URL"),
            Some("postgresql://real@h/shop_feature_x")
        );
    }

    #[test]
    fn a_symfony_worktree_reads_its_app_env_files() {
        let (cfg, mut site) = worktree_site("symfony-env", "");
        site.kind = "symfony".into();
        let main = main_of(&site.path).unwrap();
        let url = |k: &str, e: &[(String, String)]| get(e, k).map(String::from);
        let both = |f: &str, text: &str| {
            fs::write(main.join(f), text).unwrap();
            fs::write(site.path.join(f), text).unwrap();
        };
        // The committed `.env.dev` (APP_ENV defaults to dev), and `.env.dev.local` over it.
        both(
            ".env.dev",
            "DATABASE_URL=postgresql://dev@127.0.0.1:5432/shop\n",
        );
        let dev = env(&cfg, &site).unwrap();
        assert_eq!(
            url("DATABASE_URL", &dev).as_deref(),
            Some("postgresql://dev@127.0.0.1:5432/shop_feature_x")
        );
        // Main's database named in `.env.dev` only is still main's.
        assert!(is_mains_database(&site, "shop"));
        both(
            ".env.dev.local",
            "DATABASE_URL=postgresql://devlocal@h/shop\n",
        );
        assert_eq!(
            url("DATABASE_URL", &env(&cfg, &site).unwrap()).as_deref(),
            Some("postgresql://devlocal@h/shop_feature_x")
        );
        // APP_ENV from the files: `.env.staging` then.
        both(".env.local", "APP_ENV=staging\n");
        both(".env.staging", "DATABASE_URL=postgresql://staging@h/shop\n");
        assert_eq!(
            url("DATABASE_URL", &env(&cfg, &site).unwrap()).as_deref(),
            Some("postgresql://staging@h/shop_feature_x")
        );
        // A test run: `.env`, `.env.test`, `.env.test.local`; never `.env.local`.
        fs::remove_file(site.path.join(".env.local")).unwrap();
        both(".env", "DATABASE_URL=postgresql://base@h/shop\n");
        both(".env.local", "DATABASE_URL=postgresql://local@h/shop\n");
        let test = run_env(&cfg, &site, true).unwrap();
        assert_eq!(
            url("DATABASE_URL", &test).as_deref(),
            Some("postgresql://base@h/shop_feature_x")
        );
        both(".env.test", "DATABASE_URL=postgresql://test@h/shop\n");
        let test = run_env(&cfg, &site, true).unwrap();
        assert_eq!(
            url("DATABASE_URL", &test).as_deref(),
            Some("postgresql://test@h/shop_feature_x")
        );
        // Rewrite writes the dev files, never the test ones; check sees `.env.dev.local`.
        fs::write(main.join(".worktreeinclude"), ".env*\n").unwrap();
        both(".env.test.local", "DATABASE_URL=postgresql://tl@h/shop\n");
        assert!(same_as_main(&cfg, &site).contains(&"DATABASE_URL".to_string()));
        rewrite(&cfg, &site).unwrap();
        for f in [".env.dev", ".env.dev.local"] {
            let text = fs::read_to_string(site.path.join(f)).unwrap();
            assert!(text.contains("/shop_feature_x"), "{f}: {text}");
        }
        for f in [".env.test", ".env.test.local"] {
            let text = fs::read_to_string(site.path.join(f)).unwrap();
            assert!(text.ends_with("/shop\n"), "{f}: {text}");
        }
    }

    #[test]
    fn rewrite_writes_only_the_env_files_processes_read() {
        let dotenv = "DB_CONNECTION=pgsql\nDB_DATABASE=shop\n";
        let (cfg, site) = worktree_site("runtime", dotenv);
        let main = main_of(&site.path).unwrap();
        fs::write(main.join(".worktreeinclude"), ".env*\n").unwrap();
        for f in [".env", ".env.local", ".env.example", ".env.testing"] {
            fs::write(main.join(f), dotenv).unwrap();
            fs::write(site.path.join(f), dotenv).unwrap();
        }
        rewrite(&cfg, &site).unwrap();
        for f in [".env", ".env.local"] {
            let text = fs::read_to_string(site.path.join(f)).unwrap();
            assert!(text.contains("DB_DATABASE=shop_feature_x"), "{f}: {text}");
        }
        // The committed example and the test config stay as they are.
        for f in [".env.example", ".env.testing"] {
            assert_eq!(
                fs::read_to_string(site.path.join(f)).unwrap(),
                dotenv,
                "{f}"
            );
        }
    }

    #[test]
    fn after_create_runs_only_when_asked() {
        let (mut cfg, app) = parked_app("after", "APP_NAME=Shop\n");
        fs::write(app.path.join("package.json"), "{}").unwrap();
        fs::write(app.path.join("package-lock.json"), "{}").unwrap();
        cfg.sites
            .entry("shop".into())
            .or_default()
            .worktree
            .after_create = Some(vec!["{install}".into(), "touch ran".into()]);
        let list = ["{install}".to_string(), "touch ran".into()];
        assert_eq!(commands(&list, &app.path), ["npm ci", "touch ran"]);
        // No lock file: no install to offer (`npm ci` needs one).
        fs::remove_file(app.path.join("package-lock.json")).unwrap();
        assert_eq!(commands(&list, &app.path), ["touch ran"]);
        fs::write(app.path.join("pnpm-lock.yaml"), "").unwrap();
        assert_eq!(
            commands(&list, &app.path)[0],
            "pnpm install --config.strict-dep-builds=false"
        );
        let (folder, _) = add(&cfg, &app, "feat", true, None).unwrap();
        // `add` made it and ran nothing: the caller asks first.
        assert!(!folder.join("ran").exists());
        let site = site_at(&cfg, &folder).unwrap();
        // No package.json in the new worktree (untracked in main): no install to offer.
        assert_eq!(after_create(&cfg, &site), ["touch ran"]);
        run_after(&cfg, &site, "touch ran", true).unwrap();
        assert!(folder.join("ran").exists());
        let e = run_after(&cfg, &site, "echo no >&2; exit 3", true).unwrap_err();
        assert_eq!(e.to_string(), "echo no >&2; exit 3 failed: no");
    }
}
