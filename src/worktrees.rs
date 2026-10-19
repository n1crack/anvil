//! Parallel workspaces: git worktrees of an app, each served on its own address.

/// `<first>--<rest>`: the address label of an app's extra server or worktree.
pub fn joined(first: &str, rest: &str) -> String {
    format!("{first}--{rest}")
}

/// A worktree's own part of its name: the folder slugged like site names, less a
/// leading `<app>--` (an Anvil-made `myapp--feature-x` is `feature-x`).
pub fn worktree_name(folder: &Path) -> String {
    let name = crate::site_name(&folder.file_name().unwrap_or_default().to_string_lossy());
    match name.split_once("--") {
        Some((_, rest)) if !rest.is_empty() => rest.into(),
        _ => name,
    }
}

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

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
    fn repo(name: &str) -> (PathBuf, PathBuf) {
        let root = tmp(name);
        let main = root.join("main");
        fs::create_dir(&main).unwrap();
        git(&main, &["init", "-q", "-b", "main"]);
        git(&main, &["commit", "-q", "--allow-empty", "-m", "i"]);
        git(&main, &["worktree", "add", "-q", "../x", "-b", "x"]);
        (main, root.join("x"))
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
