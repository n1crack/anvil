//! Checks for names, paths and links that come from the UI or the command line before
//! they reach a file name, a command or `open`.

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

/// The name of an application Anvil creates: also its folder and its address.
pub fn new_app_name(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && !name.starts_with(['.', '-'])
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.');
    if !ok {
        bail!("Use lowercase letters, numbers, dots and dashes for the name");
    }
    Ok(())
}

/// A service id: also its folder, pid and log file names.
pub fn service_id(id: &str) -> Result<()> {
    let ok = id.len() <= 40
        && id.starts_with(|c: char| c.is_ascii_lowercase())
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !ok {
        bail!("Use lowercase letters, numbers and dashes for the service name");
    }
    Ok(())
}

/// One plain path segment: letters, digits, `-`, `_`, and `.` when `dots`; never `.`
/// or `..` on their own.
fn plain(s: &str, dots: bool) -> bool {
    !s.is_empty()
        && !s.starts_with("..")
        && s != "."
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || (dots && c == '.'))
}

/// An existing app's name as it appears in a key or a file name (a parked folder's
/// name, so dots and capitals are allowed).
pub fn site_name(s: &str) -> bool {
    plain(s, true)
}

/// A process id within an app (`web`, `queue`, `vite-2`).
pub fn process_id(s: &str) -> bool {
    plain(s, false)
}

/// `path` (relative to `root`, or absolute) resolved with symlinks followed, refused
/// unless it stays inside `root`. Returns the resolved path.
pub fn path_in(root: &Path, path: impl AsRef<Path>) -> Result<PathBuf> {
    let path = path.as_ref();
    let root = crate::platform::canonicalize(root)
        .with_context(|| format!("{} not found", root.display()))?;
    let full = crate::platform::canonicalize(root.join(path))
        .with_context(|| format!("{} not found", path.display()))?;
    if !full.starts_with(&root) {
        bail!("{} is outside {}", path.display(), root.display());
    }
    Ok(full)
}

/// A file (not a folder) inside `root`; see `path_in`.
pub fn file_in(root: &Path, path: &str) -> Result<PathBuf> {
    let file = path_in(root, path)?;
    if !file.is_file() {
        bail!("{path} is not a file of this app");
    }
    Ok(file)
}

/// The `[userinfo@]host[:port]` of a URL: after `://`, up to the path or query.
pub fn authority(url: &str) -> Option<&str> {
    url.split("://").nth(1)?.split(['/', '?']).next()
}

/// A plain-http link to this Mac: 127.0.0.1, localhost, or an app's `.<tld>` /
/// `.localhost` address. The host is read up to the port or path; userinfo (`@`) is
/// refused so `http://x.test@elsewhere` can't pass for a local address.
pub fn local_http(url: &str, tld: &str) -> bool {
    if !url.starts_with("http://") {
        return false;
    }
    let authority = authority(url).unwrap_or("").split('#').next().unwrap_or("");
    if authority.contains('@') {
        return false;
    }
    let host = authority
        .split(':')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let under = |suffix: &str| !suffix.is_empty() && host.ends_with(&format!(".{suffix}"));
    host == "127.0.0.1" || host == "localhost" || under("localhost") || under(tld)
}

/// A link Anvil opens in the browser: any https link, or a local http one.
pub fn link(url: &str, tld: &str) -> Result<()> {
    if !url.starts_with("https://") && !local_http(url, tld) {
        bail!("Only local and https links can be opened");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::fs;

    #[test]
    fn names() {
        assert!(new_app_name("my-app").is_ok());
        assert!(new_app_name("shop.be").is_ok());
        for bad in ["", "My", "a b", ".x", "-x", "a/b", "a_b", &"x".repeat(65)] {
            assert!(new_app_name(bad).is_err(), "{bad}");
        }
        assert!(service_id("mysql-2").is_ok());
        for bad in ["", "2db", "My", "a.b", "a_b", "a/b", &"x".repeat(41)] {
            assert!(service_id(bad).is_err(), "{bad}");
        }
        assert!(site_name("Shop.be") && site_name("my_app") && process_id("vite-2"));
        for bad in ["", ".", "..", "..x", "a/b", "a b", "a@b", "a#b"] {
            assert!(!site_name(bad) && !process_id(bad), "{bad}");
        }
        assert!(!process_id("a.b"));
    }

    #[test]
    fn paths_stay_inside_their_root() {
        let dir = tempfile::tempdir().unwrap();
        let dir = crate::platform::canonicalize(dir.path()).unwrap();
        let root = dir.join("site");
        fs::create_dir_all(root.join("app")).unwrap();
        fs::write(root.join("app/X.php"), "").unwrap();
        fs::write(dir.join("secret.txt"), "").unwrap();
        std::os::unix::fs::symlink(dir.join("secret.txt"), root.join("link.txt")).unwrap();
        std::os::unix::fs::symlink(&dir, root.join("up")).unwrap();
        assert!(file_in(&root, "app/X.php").is_ok());
        assert!(file_in(&root, &root.join("app/X.php").to_string_lossy()).is_ok());
        assert!(path_in(&root, "app").is_ok());
        assert!(path_in(&root, "").is_ok());
        for bad in ["../secret.txt", "link.txt", "up/secret.txt", "up"] {
            assert!(path_in(&root, bad).is_err(), "{bad}");
        }
        assert!(file_in(&root, &dir.join("secret.txt").to_string_lossy()).is_err());
        assert!(file_in(&root, "app").is_err());
        let missing = file_in(&root, "app/missing.php").unwrap_err();
        assert_eq!(missing.to_string(), "app/missing.php not found");
    }

    #[test]
    fn local_links() {
        assert!(local_http("http://shop.test", "test"));
        assert!(local_http("http://docs.vuefinder.test:8080/x?y", "test"));
        assert!(local_http("http://web.localhost/", "test"));
        assert!(local_http("http://127.0.0.1:5173", "test"));
        assert!(local_http("http://shop.test#top", "test"));
        assert!(!local_http("http://example.com", "test"));
        assert!(!local_http("http://shop.test@evil.com", "test"));
        assert!(!local_http("http://evil.com/?a=.test", "test"));
        assert!(!local_http("http://evil.com#.test", "test"));
        assert!(!local_http("http://test", "test"));
        assert!(!local_http("http://shop.test", ""));
        assert!(!local_http("file:///etc/passwd", "test"));
        assert!(link("https://example.com", "test").is_ok());
        assert!(link("http://example.com", "test").is_err());
        assert!(link("javascript:alert(1)", "test").is_err());
        assert_eq!(
            authority("postgres://u:p@127.0.0.1:5433/db"),
            Some("u:p@127.0.0.1:5433")
        );
        assert_eq!(authority("redis://h:6379?x"), Some("h:6379"));
        assert_eq!(authority("nope"), None);
    }

    proptest! {
        /// Whatever passes as a site name or an id is one segment: joined to a folder it
        /// never leaves it.
        #[test]
        fn plain_names_are_one_segment(s in "\\PC{0,12}") {
            for ok in [site_name(&s), process_id(&s), service_id(&s).is_ok(), new_app_name(&s).is_ok()] {
                if ok {
                    let p = Path::new("/r").join(&s);
                    prop_assert_eq!(p.parent(), Some(Path::new("/r")));
                    prop_assert!(!s.contains('/') && s != "." && s != "..");
                }
            }
        }

        /// A resolved path inside the root never names anything outside it, whatever
        /// mix of segments, `..` and symlinks is asked for.
        #[test]
        fn path_in_never_escapes(parts in prop::collection::vec(
            prop_oneof![Just(".."), Just("."), Just("a"), Just("up"), Just("link"), Just("f")], 0..6)
        ) {
            let dir = tempfile::tempdir().unwrap();
            let dir = crate::platform::canonicalize(dir.path()).unwrap();
            let root = dir.join("root");
            fs::create_dir_all(root.join("a")).unwrap();
            fs::write(root.join("f"), "").unwrap();
            std::os::unix::fs::symlink(&dir, root.join("up")).unwrap();
            std::os::unix::fs::symlink(root.join("a"), root.join("link")).unwrap();
            let rel = parts.join("/");
            if let Ok(p) = path_in(&root, &rel) {
                prop_assert!(p.starts_with(&root), "{rel} -> {}", p.display());
            }
        }
    }
}
