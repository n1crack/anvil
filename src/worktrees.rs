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
    name.truncate(63); // Postgres' limit; MySQL's is 64
    name
}

/// `url` pointing at database `db` instead: `postgres://u@h:5433/shop?x` -> `.../<db>?x`.
/// Only server databases; a SQLite `file:` URL already lives in the worktree.
fn with_db(url: &str, db: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    if !["postgres", "postgresql", "mysql", "mariadb"].contains(&scheme) {
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

/// What a worktree's processes get on top of their usual environment, so they never
/// reach the main app's database or Redis keys. Empty for anything but a worktree.
/// Set variables win over the worktree's `.env` (Laravel, dotenv, Next.js, Prisma).
pub fn env(cfg: &Config, site: &Site) -> Vec<(String, String)> {
    if site.worktree_of.is_none() {
        return Vec::new();
    }
    let db = db_name(site);
    let mut env = Vec::new();
    let choice = cfg
        .worktrees
        .get(&site.name)
        .map(|w| w.db)
        .unwrap_or_default();
    if choice != DbChoice::Share {
        let main = main_of(&site.path).unwrap_or_default();
        let dotenv = crate::resources::dotenv(&main);
        if let Some(current) = dotenv.get("DB_DATABASE") {
            if dotenv.get("DB_CONNECTION").map(String::as_str) == Some("sqlite") {
                // A file in the main checkout: the same file in the worktree.
                if let Ok(rel) = Path::new(current).strip_prefix(&main) {
                    let file = site.path.join(rel).display().to_string();
                    env.push(("DB_DATABASE".into(), file));
                }
            } else {
                env.push(("DB_DATABASE".into(), db.clone()));
            }
        }
        for key in ["DB_URL", "DATABASE_URL"] {
            if let Some(url) = dotenv.get(key).and_then(|u| with_db(u, &db)) {
                env.push((key.into(), url));
            }
        }
    }
    let prefix = format!("{db}_");
    env.push(("APP_URL".into(), crate::site_url(cfg, site)));
    env.push(("REDIS_PREFIX".into(), prefix.clone()));
    env.push(("CACHE_PREFIX".into(), prefix));
    env
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
        let env = env(&cfg, &site);
        assert_eq!(get(&env, "DB_DATABASE"), Some("shop_feature_x"));
        assert_eq!(get(&env, "APP_URL"), Some("http://shop--feature-x.test"));
        assert_eq!(get(&env, "REDIS_PREFIX"), Some("shop_feature_x_"));
        assert_eq!(get(&env, "CACHE_PREFIX"), Some("shop_feature_x_"));
        assert_eq!(get(&env, "DATABASE_URL"), None);
        // The main app itself gets nothing.
        let main = crate::tests::site("shop", "/s/shop", None);
        assert!(super::env(&cfg, &main).is_empty());
    }

    #[test]
    fn env_rewrites_database_url() {
        let url = "DATABASE_URL=\"postgres://postgres@127.0.0.1:5433/shop?schema=public\"\n";
        let (cfg, site) = worktree_site("url", url);
        let env = env(&cfg, &site);
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
        let env = env(&cfg, &site);
        assert_eq!(get(&env, "DB_DATABASE"), None);
        assert_eq!(get(&env, "DATABASE_URL"), None);
        assert_eq!(get(&env, "REDIS_PREFIX"), Some("shop_feature_x_"));
        assert_eq!(get(&env, "CACHE_PREFIX"), Some("shop_feature_x_"));
    }

    #[test]
    fn no_database_settings_only_the_address_and_prefixes() {
        let (cfg, site) = worktree_site("nodb", "APP_NAME=Shop\n");
        let keys: Vec<_> = env(&cfg, &site).into_iter().map(|(k, _)| k).collect();
        assert_eq!(keys, ["APP_URL", "REDIS_PREFIX", "CACHE_PREFIX"]);
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
}
