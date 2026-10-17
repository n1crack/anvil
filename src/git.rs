//! Git status of a project folder, read with as few git processes as possible.

use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Serialize, Default, Debug, PartialEq)]
pub struct GitInfo {
    pub is_repo: bool,
    /// Branch name, or the short commit hash when detached.
    pub branch: String,
    pub detached: bool,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    /// Modified, staged and untracked files.
    pub changed: u32,
    pub commit: Option<Commit>,
    /// Web page of `origin`, e.g. https://github.com/owner/repo.
    pub remote: Option<String>,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Commit {
    pub hash: String,
    pub subject: String,
    pub author: String,
    /// Seconds since the Unix epoch.
    pub time: u64,
}

/// Git info for a project folder. Only the folder itself is checked for `.git`:
/// projects inside a bigger repository get `None` rather than a third git process.
pub fn info(dir: &Path) -> Option<GitInfo> {
    let git_dir = git_dir(dir)?;
    let git = |args: &[&str]| {
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
    };
    // Both run at the same time.
    let status = git(&["status", "--porcelain=v2", "--branch", "-z"]).ok()?;
    let log = git(&["log", "-1", "--format=%h%x00%s%x00%an%x00%ct"]).ok()?;
    let status = status.wait_with_output().ok()?;
    let log = log.wait_with_output().ok()?;
    if !status.status.success() {
        return None;
    }
    let mut info = parse_status(&String::from_utf8_lossy(&status.stdout));
    if log.status.success() {
        info.commit = parse_log(&String::from_utf8_lossy(&log.stdout));
    }
    info.remote = fs::read_to_string(config_path(&git_dir))
        .ok()
        .and_then(|c| origin_url(&c))
        .and_then(|u| web_url(&u));
    Some(info)
}

/// The repository folder: `.git` itself, or where a `.git` file points to.
fn git_dir(dir: &Path) -> Option<PathBuf> {
    let dot = dir.join(".git");
    if dot.is_dir() {
        return Some(dot);
    }
    let file = fs::read_to_string(&dot).ok()?;
    Some(dir.join(gitdir_of(&file)?))
}

/// The path in a `.git` file (worktrees, submodules): `gitdir: <path>`.
fn gitdir_of(file: &str) -> Option<&str> {
    file.lines()
        .find_map(|l| l.strip_prefix("gitdir:"))
        .map(str::trim)
        .filter(|p| !p.is_empty())
}

/// Worktrees keep their config in the main repository (named in `commondir`).
fn config_path(git_dir: &Path) -> PathBuf {
    match fs::read_to_string(git_dir.join("commondir")) {
        Ok(common) => git_dir.join(common.trim()).join("config"),
        Err(_) => git_dir.join("config"),
    }
}

fn parse_status(out: &str) -> GitInfo {
    let mut info = GitInfo {
        is_repo: true,
        ..Default::default()
    };
    let mut oid = "";
    let mut fields = out.split('\0');
    while let Some(f) = fields.next() {
        if let Some(h) = f.strip_prefix("# branch.head ") {
            info.detached = h == "(detached)";
            info.branch = h.into();
        } else if let Some(o) = f.strip_prefix("# branch.oid ") {
            oid = o;
        } else if let Some(u) = f.strip_prefix("# branch.upstream ") {
            info.upstream = Some(u.into());
        } else if let Some(ab) = f.strip_prefix("# branch.ab ") {
            for n in ab.split(' ') {
                if let Some(a) = n.strip_prefix('+') {
                    info.ahead = a.parse().unwrap_or(0);
                } else if let Some(b) = n.strip_prefix('-') {
                    info.behind = b.parse().unwrap_or(0);
                }
            }
        } else if f.starts_with("1 ") || f.starts_with("u ") || f.starts_with("? ") {
            info.changed += 1;
        } else if f.starts_with("2 ") {
            // Renames carry the original path as an extra field.
            info.changed += 1;
            fields.next();
        }
    }
    if info.detached {
        info.branch = oid.chars().take(7).collect();
    }
    info
}

fn parse_log(out: &str) -> Option<Commit> {
    let mut p = out.trim_end_matches('\n').splitn(4, '\0');
    Some(Commit {
        hash: p.next().filter(|h| !h.is_empty())?.into(),
        subject: p.next()?.into(),
        author: p.next()?.into(),
        time: p.next()?.trim().parse().ok()?,
    })
}

/// `url` of `[remote "origin"]` in a git config file.
fn origin_url(config: &str) -> Option<String> {
    let mut in_origin = false;
    for line in config.lines().map(str::trim) {
        if line.starts_with('[') {
            in_origin = line.replace(' ', "") == "[remote\"origin\"]";
        } else if in_origin
            && let Some((k, v)) = line.split_once('=')
            && k.trim() == "url"
        {
            return Some(v.trim().trim_matches('"').into());
        }
    }
    None
}

/// Web page of a remote: git@host:o/r.git and https://host/o/r.git become https://host/o/r.
fn web_url(remote: &str) -> Option<String> {
    let (host, path) = match remote.split_once("://") {
        Some((scheme, rest)) => {
            if scheme == "file" {
                return None;
            }
            rest.split_once('/')?
        }
        // scp-like: [user@]host:path
        None => {
            let (host, path) = remote.split_once(':')?;
            if host.contains('/') {
                return None;
            }
            (host, path)
        }
    };
    let host = host.rsplit('@').next()?;
    let host = host.split(':').next()?;
    let path = path.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    if host.is_empty() || path.is_empty() {
        return None;
    }
    Some(format!("https://{host}/{path}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_on_a_branch() {
        let out = "# branch.oid 1234567890abcdef\0# branch.head main\0\
                   # branch.upstream origin/main\0# branch.ab +2 -3\0\
                   1 .M N... 100644 100644 100644 aaa bbb src/a.rs\0\
                   2 R. N... 100644 100644 100644 aaa bbb R100 new.rs\0old.rs\0\
                   u UU N... 100644 100644 100644 100644 a b c conflict.rs\0\
                   ? notes.txt\0! ignored.log\0";
        let i = parse_status(out);
        assert_eq!(i.branch, "main");
        assert!(!i.detached);
        assert_eq!(i.upstream.as_deref(), Some("origin/main"));
        assert_eq!((i.ahead, i.behind, i.changed), (2, 3, 4));
    }

    #[test]
    fn status_detached_and_clean() {
        let i = parse_status("# branch.oid 1234567890abcdef\0# branch.head (detached)\0");
        assert!(i.detached);
        assert_eq!(i.branch, "1234567");
        assert_eq!((i.upstream, i.changed), (None, 0));
    }

    #[test]
    fn log_line() {
        let c = parse_log("abc1234\0Fix the thing\0Jane Doe\x001700000000\n").unwrap();
        assert_eq!(c.hash, "abc1234");
        assert_eq!(c.subject, "Fix the thing");
        assert_eq!(c.author, "Jane Doe");
        assert_eq!(c.time, 1700000000);
        assert_eq!(parse_log(""), None);
    }

    #[test]
    fn remote_web_urls() {
        let cases = [
            (
                "git@github.com:owner/repo.git",
                Some("https://github.com/owner/repo"),
            ),
            (
                "https://github.com/owner/repo.git",
                Some("https://github.com/owner/repo"),
            ),
            (
                "https://github.com/owner/repo",
                Some("https://github.com/owner/repo"),
            ),
            (
                "git@gitlab.com:group/sub/repo.git",
                Some("https://gitlab.com/group/sub/repo"),
            ),
            (
                "https://user@bitbucket.org/team/repo.git",
                Some("https://bitbucket.org/team/repo"),
            ),
            (
                "ssh://git@git.example.com:2222/o/r.git",
                Some("https://git.example.com/o/r"),
            ),
            (
                "http://git.example.com/o/r/",
                Some("https://git.example.com/o/r"),
            ),
            ("/srv/repos/r.git", None),
            ("file:///srv/repos/r.git", None),
            ("../other", None),
        ];
        for (remote, want) in cases {
            assert_eq!(web_url(remote).as_deref(), want, "{remote}");
        }
    }

    #[test]
    fn origin_from_config() {
        let cfg = "[core]\n\tbare = false\n[remote \"upstream\"]\n\turl = git@github.com:up/r.git\n\
                   [remote \"origin\"]\n\turl = git@github.com:me/r.git\n\tfetch = +refs/heads/*:refs/remotes/origin/*\n";
        assert_eq!(origin_url(cfg).as_deref(), Some("git@github.com:me/r.git"));
        assert_eq!(origin_url("[core]\n"), None);
    }

    #[test]
    fn gitdir_file() {
        assert_eq!(
            gitdir_of("gitdir: ../.git/worktrees/x\n"),
            Some("../.git/worktrees/x")
        );
        assert_eq!(gitdir_of("nonsense"), None);
    }

    #[test]
    fn this_checkout() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        if !dir.join(".git").exists() {
            return; // built from a source archive
        }
        let start = std::time::Instant::now();
        let i = info(dir).expect("a git checkout");
        println!("{i:?} in {:?}", start.elapsed());
        assert!(i.is_repo && !i.branch.is_empty() && i.commit.is_some());
    }

    #[test]
    fn not_a_repo() {
        let dir = std::env::temp_dir().join(format!("anvil-git-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        assert_eq!(info(&dir), None);
        fs::remove_dir_all(&dir).unwrap();
    }
}
