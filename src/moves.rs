//! An app folder or a parked folder renamed or moved by hand keeps its app: the same
//! name, so the same address, settings, database, mail, logs and worktrees. Anvil
//! remembers each one's file identity (`remember`, from every scan of the app); `follow`,
//! on every `load`, finds where a folder that isn't at its path any more went (same disk:
//! a rename or a move keeps the identity) and points the config there. A folder moved to
//! the Trash, deleted, or copied to another disk is not followed.
use crate::{Config, Site};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// A folder's identity on its disk: (device, inode).
type Id = (u64, u64);

#[derive(Serialize, Deserialize, Default, PartialEq, Clone)]
struct Known {
    /// Parked folders.
    roots: BTreeMap<PathBuf, Id>,
    /// Apps (not worktrees) by name: their folder and its identity.
    apps: BTreeMap<String, (PathBuf, Id)>,
}

fn file() -> PathBuf {
    crate::home().join("folders.json")
}

#[cfg(unix)]
fn id(p: &Path) -> Option<Id> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(p)
        .ok()
        .filter(|m| m.is_dir())
        .map(|m| (m.dev(), m.ino()))
}

#[cfg(not(unix))]
fn id(_: &Path) -> Option<Id> {
    None // ponytail: Windows' file IDs (GetFileInformationByHandle) when it needs this
}

/// What `remember` wrote last.
static LAST: std::sync::Mutex<Option<Known>> = std::sync::Mutex::new(None);

/// Record the folders of `cfg` and `sites` (`anvil::sites(cfg)`); written only on a change.
pub fn remember(cfg: &Config, sites: &[Site]) {
    let mut known = Known::default();
    for d in &cfg.paths {
        known.roots.extend(id(d).map(|i| (d.clone(), i)));
    }
    for s in sites.iter().filter(|s| s.worktree_of.is_none()) {
        known
            .apps
            .extend(id(&s.path).map(|i| (s.name.clone(), (s.path.clone(), i))));
    }
    let mut last = crate::lock(&LAST);
    if last.as_ref() == Some(&known) {
        return;
    }
    if let Ok(json) = serde_json::to_vec(&known)
        && std::fs::write(file(), json).is_ok()
    {
        *last = Some(known);
    }
}

/// A folder that moved: from, to, and the app's name (none for a parked folder).
pub type Move = (PathBuf, PathBuf, Option<String>);

/// Point `cfg` at the folders that moved since `remember`, and remember them where they
/// are now, so the next `follow` has nothing to do: the caller saves `cfg` (`update`)
/// or applies the moves again under the lock (`load`).
pub fn follow(cfg: &mut Config) -> Vec<Move> {
    let Some(mut known) = std::fs::read(file())
        .ok()
        .and_then(|b| serde_json::from_slice::<Known>(&b).ok())
    else {
        return vec![];
    };
    let gone = |p: &Path, i: &Id| id(p).as_ref() != Some(i);
    let mut moves = vec![];
    // Parked folders first: the apps in one moved with it.
    let roots: Vec<(PathBuf, Id)> = known
        .roots
        .iter()
        .filter(|(p, i)| gone(p, i))
        .map(|(p, i)| (p.clone(), *i))
        .collect();
    for (old, i) in roots {
        if let Some(new) = locate(i, &old, cfg) {
            apply(cfg, &old, &new, None);
            known.roots.remove(&old);
            known.roots.insert(new.clone(), i);
            moves.push((old, new, None));
        }
    }
    let apps: Vec<(String, PathBuf, Id)> = known
        .apps
        .iter()
        .filter(|(_, (p, i))| gone(p, i))
        .map(|(n, (p, i))| (n.clone(), p.clone(), *i))
        .collect();
    for (name, old, i) in apps {
        if let Some(new) = locate(i, &old, cfg) {
            apply(cfg, &old, &new, Some(&name));
            known.apps.insert(name.clone(), (new.clone(), i));
            repair_worktrees(&old, &new);
            moves.push((old, new, Some(name)));
        }
    }
    if !moves.is_empty()
        && let Ok(json) = serde_json::to_vec(&known)
    {
        let _ = std::fs::write(file(), json);
        *crate::lock(&LAST) = None;
    }
    moves
}

/// The git worktrees of an app moved from `old` to `new`: their links to it and back name
/// the old path. Those that were inside it moved with it, and git wants their new paths.
fn repair_worktrees(old: &Path, new: &Path) {
    if !new.join(".git").is_dir() {
        return;
    }
    // Git writes real paths (/private/var for /var); `old` is gone, so from what's above it.
    let real_old = old
        .ancestors()
        .find_map(|a| {
            Some(
                crate::platform::canonicalize(a)
                    .ok()?
                    .join(old.strip_prefix(a).ok()?),
            )
        })
        .unwrap_or_else(|| old.to_path_buf());
    let inside: Vec<PathBuf> = std::fs::read_dir(new.join(".git/worktrees"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| std::fs::read_to_string(e.path().join("gitdir")).ok())
        .filter_map(|g| {
            let wt = Path::new(g.trim()).parent()?;
            let dir = wt
                .strip_prefix(old)
                .or_else(|_| wt.strip_prefix(&real_old))
                .ok()?;
            Some(new.join(dir))
        })
        .filter(|p| p.is_dir())
        .collect();
    let _ = crate::platform::command("git")
        .arg("worktree")
        .arg("repair")
        .args(&inside)
        .current_dir(new)
        .output();
}

/// `follow`'s moves applied to `cfg` again (the config as it is under the lock).
pub fn apply_all(cfg: &mut Config, moves: &[Move]) {
    for (old, new, name) in moves {
        apply(cfg, old, new, name.as_deref());
    }
}

/// Where the folder with identity `i`, last at `old`, is now: never in a Trash. Without
/// macOS's lookup by identity, only beside `old` (renamed) or in a parked folder.
fn locate(i: Id, old: &Path, cfg: &Config) -> Option<PathBuf> {
    let found = by_id(i).or_else(|| {
        old.parent()
            .into_iter()
            .chain(cfg.paths.iter().map(PathBuf::as_path))
            .flat_map(|d| std::fs::read_dir(d).into_iter().flatten().flatten())
            .map(|e| e.path())
            .find(|p| id(p) == Some(i))
    })?;
    let trashed = found
        .components()
        .any(|c| c.as_os_str().to_string_lossy().starts_with(".Trash"));
    (!trashed && id(&found) == Some(i)).then_some(found)
}

/// macOS finds any folder on its disk by identity: /.vol/<device>/<inode>.
#[cfg(target_os = "macos")]
fn by_id((dev, ino): Id) -> Option<PathBuf> {
    let f = std::fs::File::open(format!("/.vol/{dev}/{ino}")).ok()?;
    let mut path = PathBuf::new();
    nix::fcntl::fcntl(&f, nix::fcntl::FcntlArg::F_GETPATH(&mut path)).ok()?;
    Some(path)
}

#[cfg(not(target_os = "macos"))]
fn by_id(_: Id) -> Option<PathBuf> {
    None
}

/// `base`/`rest`, without the trailing slash `join` leaves for an empty `rest`.
fn join(base: &Path, rest: &Path) -> PathBuf {
    if rest.as_os_str().is_empty() {
        base.to_path_buf()
    } else {
        base.join(rest)
    }
}

/// `new` (a real path, as macOS gives it: /private/var for /var) spelled from the
/// nearest folder above `old` that is still there, as the config spells paths.
fn spelled(old: &Path, new: &Path) -> PathBuf {
    old.ancestors()
        .skip(1)
        .find_map(|a| {
            let real = crate::platform::canonicalize(a).ok()?;
            Some(join(a, new.strip_prefix(real).ok()?))
        })
        .unwrap_or_else(|| new.to_path_buf())
}

/// Point `cfg` from `old` to `new`: the parked folders, links and kept names in it; an
/// app's (`name`) stays its name: kept for its new folder when parked, else a link.
fn apply(cfg: &mut Config, old: &Path, new: &Path, name: Option<&str>) {
    let new = &spelled(old, new);
    let moved = |p: &Path| p.strip_prefix(old).ok().map(|rest| join(new, rest));
    for p in cfg.paths.iter_mut().chain(cfg.links.values_mut()) {
        if let Some(n) = moved(p) {
            *p = n;
        }
    }
    cfg.folder_names = std::mem::take(&mut cfg.folder_names)
        .into_iter()
        .map(|(p, n)| (moved(&p).unwrap_or(p), n))
        .filter(|(p, _)| p.is_dir())
        .collect();
    let Some(name) = name else {
        return;
    };
    let same = |a: &Path, b: &Path| {
        let real = |p: &Path| crate::platform::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        real(a) == real(b)
    };
    // In a parked folder: its path as that folder spells it (the scan's), not the real
    // one macOS gives (/private/var for /var).
    let parked = new.file_name().and_then(|f| {
        let up = new.parent()?;
        Some(cfg.paths.iter().find(|d| same(d, up))?.join(f))
    });
    if let Some(new) = parked {
        cfg.links.remove(name);
        let folder = crate::site_name(&new.file_name().unwrap_or_default().to_string_lossy());
        if folder == name {
            cfg.folder_names.remove(&new);
        } else {
            cfg.folder_names.insert(new, name.to_string());
        }
    } else {
        cfg.folder_names.remove(new);
        cfg.links.insert(name.to_string(), new.to_path_buf());
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs;

    fn names(cfg: &Config) -> BTreeMap<String, PathBuf> {
        crate::sites(cfg)
            .into_iter()
            .map(|s| (s.name, s.path))
            .collect()
    }

    #[test]
    fn a_moved_app_keeps_its_worktrees() {
        use crate::worktrees::tests::git;
        let _env = crate::lock(&crate::ENV_LOCK);
        let d = tempfile::tempdir().unwrap();
        let d = d.path().to_path_buf(); // /var/…: not the real /private/var/… on macOS
        // SAFETY: every test that sets ANVIL_HOME holds ENV_LOCK.
        unsafe { std::env::set_var("ANVIL_HOME", d.join("home")) };
        let sites = d.join("Sites");
        let shop = sites.join("shop");
        fs::create_dir_all(&shop).unwrap();
        git(&shop, &["init", "-q", "-b", "main"]);
        fs::write(shop.join("index.php"), "").unwrap();
        git(&shop, &["add", "."]);
        git(&shop, &["commit", "-q", "-m", "i"]);
        let wt = shop.join(".claude/worktrees/fx");
        git(
            &shop,
            &["worktree", "add", "-q", wt.to_str().unwrap(), "-b", "fx"],
        );
        let mut cfg = Config::default();
        cfg.paths.push(sites.clone());
        crate::save(&cfg).unwrap();
        let cfg = crate::load().unwrap();
        remember(&cfg, &crate::sites(&cfg));
        assert!(names(&cfg).contains_key("shop--fx"));
        fs::rename(&shop, sites.join("shop-v2")).unwrap();
        let got = names(&crate::load().unwrap());
        assert_eq!(got["shop"], sites.join("shop-v2"));
        let real = |p: &Path| crate::platform::canonicalize(p).unwrap();
        assert_eq!(
            real(&got["shop--fx"]),
            real(&sites.join("shop-v2/.claude/worktrees/fx"))
        );
    }

    #[test]
    fn renamed_and_moved_folders_keep_their_app() {
        let _env = crate::lock(&crate::ENV_LOCK);
        let d = tempfile::tempdir().unwrap();
        let d = d.path().to_path_buf(); // /var/…: not the real /private/var/… on macOS
        // SAFETY: every test that sets ANVIL_HOME holds ENV_LOCK.
        unsafe { std::env::set_var("ANVIL_HOME", d.join("home")) };
        let (sites, other) = (d.join("Sites"), d.join("Elsewhere"));
        for f in [sites.join("shop"), sites.join("blog"), other.clone()] {
            fs::create_dir_all(f).unwrap();
        }
        let mut cfg = Config::default();
        cfg.paths.push(sites.clone());
        crate::save(&cfg).unwrap();
        let check = |want: &[(&str, PathBuf)]| {
            let cfg = crate::load().unwrap();
            let got = names(&cfg);
            remember(&cfg, &crate::sites(&cfg));
            let want: BTreeMap<String, PathBuf> = want
                .iter()
                .map(|(n, p)| (n.to_string(), p.clone()))
                .collect();
            assert_eq!(got, want);
        };
        check(&[("blog", sites.join("blog")), ("shop", sites.join("shop"))]);
        // Renamed in place: same name, and a new `shop` folder gets a name of its own.
        fs::rename(sites.join("shop"), sites.join("shop-v2")).unwrap();
        check(&[
            ("blog", sites.join("blog")),
            ("shop", sites.join("shop-v2")),
        ]);
        fs::create_dir(sites.join("shop")).unwrap();
        let cfg = crate::load().unwrap();
        let got = names(&cfg);
        assert_eq!(got["shop"], sites.join("shop-v2"));
        assert!(
            got.iter()
                .any(|(n, p)| n.starts_with("shop-") && *p == sites.join("shop"))
        );
        fs::remove_dir(sites.join("shop")).unwrap();
        if cfg!(target_os = "macos") {
            // Out of the parked folder: a link by the same name (found anywhere on the
            // disk by its identity: macOS only).
            fs::rename(sites.join("shop-v2"), other.join("store")).unwrap();
            check(&[("blog", sites.join("blog")), ("shop", other.join("store"))]);
            assert_eq!(crate::load().unwrap().links["shop"], other.join("store"));
            fs::rename(other.join("store"), sites.join("shop")).unwrap();
        } else {
            fs::rename(sites.join("shop-v2"), sites.join("shop")).unwrap();
        }
        // Back under its own name: parked again, nothing kept.
        check(&[("blog", sites.join("blog")), ("shop", sites.join("shop"))]);
        let cfg = crate::load().unwrap();
        assert!(cfg.links.is_empty() && cfg.folder_names.is_empty());
        // The parked folder itself renamed: its apps come along.
        fs::rename(&sites, d.join("Projects")).unwrap();
        let p = d.join("Projects");
        check(&[("blog", p.join("blog")), ("shop", p.join("shop"))]);
        assert_eq!(crate::load().unwrap().paths, std::slice::from_ref(&p));
        // In the Trash: not followed, the app is gone.
        let trash = d.join(".Trash");
        fs::create_dir(&trash).unwrap();
        fs::rename(p.join("blog"), trash.join("blog")).unwrap();
        check(&[("shop", p.join("shop"))]);
    }
}
