//! OPcache for PHP 8.1–8.4: the default builds lack it, so Anvil builds `opcache.so` from
//! php.net's source of the same patch version on this Mac. 8.5 has it built in. So is
//! `mysqli.so`, which every macOS build lacks and WordPress needs.

use crate::{fetch_json, fetch_tgz, home, php_dir, run};
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

/// The versions whose default build lacks OPcache.
pub const BUILDS: [&str; 4] = ["8.1", "8.2", "8.3", "8.4"];

/// Tuned for development: a saved file shows on the next request. JIT stays off: it
/// breaks step debugging and gains little here.
const DEV: [(&str, &str); 5] = [
    ("opcache.enable", "1"),
    ("opcache.enable_cli", "0"),
    ("opcache.validate_timestamps", "1"),
    ("opcache.revalidate_freq", "0"),
    ("opcache.memory_consumption", "256"),
];

#[derive(Serialize, Debug, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum State {
    BuiltIn,
    Ready,
    Missing,
}

pub fn so_path(minor: &str) -> PathBuf {
    php_dir(minor).join("ext").join("opcache.so")
}

pub fn state(minor: &str) -> State {
    // Windows' PHP ships OPcache; its php.ini loads it.
    if cfg!(windows) || !BUILDS.contains(&minor) {
        State::BuiltIn
    } else if so_path(minor).exists() {
        State::Ready
    } else {
        State::Missing
    }
}

/// Remove a built OPcache: PHP-FPM runs without it from its next start. Building it again
/// takes minutes. A PHP that ships it (8.5, Windows) keeps it.
pub fn remove(minor: &str) -> Result<()> {
    if state(minor) != State::Ready {
        bail!("PHP {minor} has no OPcache of Anvil's to remove");
    }
    Ok(std::fs::remove_file(so_path(minor))?)
}

/// PHP-FPM's `-d` arguments: the extension when built, then the dev settings the user
/// hasn't set in `ini` (the php.ini overrides of the master's apps: global, or with one
/// app's own over them), then theirs.
pub fn fpm_args(ini: &std::collections::BTreeMap<String, String>, minor: &str) -> Vec<String> {
    let mysqli: Vec<String> = match mysqli_path(minor) {
        p if p.exists() => vec!["-d".into(), format!("extension={}", p.display())],
        _ => vec![],
    };
    let mut args = match state(minor) {
        State::Missing => return mysqli,
        State::BuiltIn => vec![],
        State::Ready => vec![
            "-d".into(),
            format!("zend_extension={}", so_path(minor).display()),
        ],
    };
    // OPcache reads most of its settings once, when FPM starts: a pool's php_value
    // comes too late, so the user's opcache.* values go here too, over the dev ones.
    let mine = ini.iter().filter(|(k, _)| k.starts_with("opcache."));
    let dev = DEV.iter().filter(|(k, _)| !ini.contains_key(*k));
    for (k, v) in dev
        .map(|(k, v)| (*k, *v))
        .chain(mine.map(|(k, v)| (k.as_str(), v.as_str())))
    {
        args.extend(["-d".into(), format!("{k}={v}")]);
    }
    args.extend(mysqli);
    args
}

/// Anvil's build of mysqli for PHP `minor`, loaded with OPcache's `-d`s when there.
pub fn mysqli_path(minor: &str) -> PathBuf {
    php_dir(minor).join("ext").join("mysqli.so")
}

/// Whether PHP `minor` has mysqli: Windows' PHP loads its own (`PHP_WINDOWS_EXT`).
pub fn has_mysqli(minor: &str) -> bool {
    cfg!(windows) || mysqli_path(minor).exists()
}

/// Build `mysqli.so` for the installed PHP `minor` from php.net's source of its exact
/// version (a few seconds once the source is configured), and put it in place once it loads.
pub fn build_mysqli(minor: &str) -> Result<()> {
    if has_mysqli(minor) {
        return Ok(());
    }
    if cfg!(target_os = "linux") {
        bail!("Anvil's PHP on Linux can't load extensions yet, mysqli included");
    }
    let _held = crate::phpext::build_lock(minor, "mysqli", "mysqli")?;
    let (full, src) = source_tree(minor)?;
    println!("Building mysqli for PHP {full}…");
    let jobs = std::thread::available_parallelism().map_or(4, |n| n.get());
    run(crate::platform::command("make")
        .args(["-s", &format!("-j{jobs}"), "ext/mysqli/mysqli.la"])
        .current_dir(&src))
    .map_err(|e| tools_error("mysqli", e))?;
    put_mysqli(minor, &src)?;
    println!("mysqli built for PHP {full}");
    stage_in(&src)?;
    prune(&src);
    Ok(())
}

/// The tree's built mysqli in place for PHP `minor`, once PHP loads it.
fn put_mysqli(minor: &str, src: &Path) -> Result<()> {
    let built = src.join("ext/mysqli/.libs/mysqli.so");
    let check = crate::platform::command(php_dir(minor).join("php"))
        .arg("-n")
        .arg("-d")
        .arg(format!("extension={}", built.display()))
        .arg("-m")
        .output()?;
    if !String::from_utf8_lossy(&check.stdout)
        .lines()
        .any(|l| l.trim() == "mysqli")
    {
        bail!("the mysqli built for PHP {minor} doesn't load");
    }
    install_so(&built, &mysqli_path(minor))
}

/// Build `opcache.so` for the installed PHP `minor` from php.net's source of its exact
/// version, verified against php.net's checksum, and put it in place once it loads.
pub fn build(minor: &str) -> Result<()> {
    if !BUILDS.contains(&minor) {
        bail!("PHP {minor} has OPcache built in");
    }
    // Linux's builds are fully static: they load no .so at all.
    if cfg!(target_os = "linux") {
        bail!("on Linux, OPcache comes with PHP 8.5; Anvil's PHP {minor} can't load it");
    }
    let _held = crate::phpext::build_lock(minor, "opcache", "OPcache")?;
    let php = php_dir(minor).join("php");
    let (full, src) = source_tree(minor)?;
    println!("Building OPcache for PHP {full}…");
    let jobs = std::thread::available_parallelism().map_or(4, |n| n.get());
    run(crate::platform::command("make")
        .args(["-s", &format!("-j{jobs}")])
        .current_dir(&src))
    .map_err(|e| tools_error("OPcache", e))?;
    let built = src.join("modules/opcache.so");
    // A `.so` PHP rejects would only print a warning on every start: check it first.
    let check = crate::platform::command(&php)
        .arg("-n")
        .arg("-d")
        .arg(format!("zend_extension={}", built.display()))
        .arg("-v")
        .output()?;
    if !String::from_utf8_lossy(&check.stdout).contains("Zend OPcache") {
        bail!("the OPcache built for PHP {full} doesn't load");
    }
    install_so(&built, &so_path(minor))?;
    println!("OPcache built for PHP {full}");
    // The same make built mysqli too: no reason to leave it out.
    if let Err(e) = put_mysqli(minor, &src) {
        println!("PHP {full} works without mysqli for now: {e}");
    }
    // Extensions need only the staged headers: keep those, not the whole tree.
    stage_in(&src)?;
    prune(&src);
    Ok(())
}

/// The installed PHP `minor`'s headers, phpize and php-config (`<tree>/stage`), all
/// extensions build against: staged once from the configured tree, which then goes
/// (286 MB for 8.4, against 6.7 MB staged).
pub fn stage(minor: &str) -> Result<(String, PathBuf)> {
    let full = installed_full(minor)?;
    let stage = home()
        .join("cache")
        .join("php-src")
        .join(&full)
        .join("stage");
    if stage.join(".anvil-staged").exists() {
        return Ok((full, stage));
    }
    let (full, src) = source_tree(minor)?;
    stage_in(&src)?;
    prune(&src);
    Ok((full, stage))
}

fn stage_in(src: &Path) -> Result<()> {
    let staged = src.join("stage").join(".anvil-staged");
    if !staged.exists() {
        run(crate::platform::command("make")
            .args(["-s", "install-headers", "install-build", "install-programs"])
            .current_dir(src))
        .map_err(|e| tools_error("PHP's headers", e))?;
        fs::write(&staged, "")?;
    }
    Ok(())
}

/// Everything in the source folder but its staged headers.
fn prune(src: &Path) {
    for e in fs::read_dir(src).into_iter().flatten().flatten() {
        if e.file_name() != "stage" {
            let p = e.path();
            let _ = if p.is_dir() && !p.is_symlink() {
                fs::remove_dir_all(&p)
            } else {
                fs::remove_file(&p)
            };
        }
    }
}

/// `cache/php-src/<full>` of a patch release no longer installed goes (its staged headers
/// serve no build again): after an install brought another, or before one is fetched.
pub fn prune_sources() {
    let fulls: Vec<String> = crate::installed_php()
        .iter()
        .filter_map(|m| installed_full(m).ok())
        .collect();
    for e in fs::read_dir(home().join("cache").join("php-src"))
        .into_iter()
        .flatten()
        .flatten()
    {
        if !fulls.iter().any(|f| *f == e.file_name().to_string_lossy()) {
            let _ = fs::remove_dir_all(e.path());
        }
    }
}

/// The exact version (`8.4.23`) of the installed PHP `minor`.
pub(crate) fn installed_full(minor: &str) -> Result<String> {
    let out = crate::platform::command(crate::php_bin(minor))
        .args(["-n", "-r", "echo PHP_VERSION;"])
        .output()
        .with_context(|| format!("PHP {minor} is not installed"))?;
    let full = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() || !full.starts_with(&format!("{minor}.")) {
        bail!("couldn't read the version of PHP {minor}");
    }
    Ok(full)
}

/// php.net's source of the installed PHP `minor`'s exact version, verified and
/// configured like that binary, in `~/.anvil/cache/php-src/<full>`, until a build has
/// staged its headers, phpize and php-config in its `stage` folder (`stage`), which
/// is all that stays. Returns the full version and the folder.
pub fn source_tree(minor: &str) -> Result<(String, PathBuf)> {
    let full = installed_full(minor)?;
    let src = home().join("cache").join("php-src").join(&full);
    let done = src.join(".anvil-configured");
    // A tree configured before mysqli was built from it is configured again.
    let with_mysqli =
        || fs::read_to_string(src.join("Makefile")).is_ok_and(|m| m.contains("mysqli.la"));
    if done.exists() && with_mysqli() {
        return Ok((full, src));
    }
    println!("Getting PHP {full}'s source…");
    let sum = source_sum(&full)?;
    prune_sources();
    prune(&src);
    fetch_tgz(
        &format!("https://www.php.net/distributions/php-{full}.tar.gz"),
        &src,
        true,
        Some(&sum),
    )?;
    run(crate::platform::command("./configure")
        .arg("--quiet")
        // php-config and phpize carry the prefix: the stage is where they are.
        .arg(format!("--prefix={}", src.join("stage").display()))
        .args([
            "--disable-all",
            "--disable-cgi",
            "--disable-phpdbg",
            "--without-sqlite3",
            "--without-pdo-sqlite",
            "--enable-opcache=shared",
            // mysqli as a module, against the binary's own mysqlnd (whose structs keep
            // their size without compression support, so the module fits it either way).
            "--with-mysqli=shared",
            "--enable-mysqlnd",
            "--disable-mysqlnd-compression-support",
            // The default builds have sessions: igbinary and msgpack add their
            // session serializers only when the headers say so.
            "--enable-session",
        ])
        .current_dir(&src))
    .map_err(|e| tools_error("PHP's source", e))?;
    let info = crate::platform::command(php_dir(minor).join("php"))
        .args(["-n", "-i"])
        .output()?;
    let config_h = fs::read_to_string(src.join("main/php_config.h"))?;
    build_matches(&String::from_utf8_lossy(&info.stdout), &config_h)
        .map_err(|why| anyhow::anyhow!("nothing can be built for PHP {full}: {why}"))?;
    fs::write(&done, "")?;
    Ok((full, src))
}

/// The SHA-256 php.net publishes for `php-<full>.tar.gz`.
pub(crate) fn source_sum(full: &str) -> Result<String> {
    sum_from(
        &fetch_json(&format!(
            "https://www.php.net/releases/index.php?json&version={full}"
        ))?,
        full,
    )
}

fn sum_from(releases: &serde_json::Value, full: &str) -> Result<String> {
    let file = format!("php-{full}.tar.gz");
    releases["source"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|s| s["filename"] == file.as_str())
        .and_then(|s| s["sha256"].as_str())
        .map(String::from)
        .with_context(|| format!("php.net lists no checksum for {file}"))
}

/// Whether the configured source (`main/php_config.h`) builds what the installed binary
/// (`php -i`) is: thread safety, debug and Zend signals. Err names the first mismatch.
pub(crate) fn build_matches(php_i: &str, php_config_h: &str) -> Result<(), String> {
    let binary = |label: &str| {
        php_i.lines().find_map(|l| {
            let (k, v) = l.split_once("=>")?;
            k.trim()
                .starts_with(label)
                .then(|| matches!(v.trim(), "enabled" | "yes"))
        })
    };
    let source = |define: &str| {
        php_config_h
            .lines()
            .any(|l| l.split_whitespace().eq(["#define", define, "1"]))
    };
    for (label, define, what) in [
        ("Thread Safety", "ZTS", "thread safe"),
        ("Debug Build", "ZEND_DEBUG", "a debug build"),
        // `php -i` says "Zend Signal Handling".
        ("Zend Signal", "ZEND_SIGNALS", "built with Zend signals"),
    ] {
        let Some(bin) = binary(label) else {
            return Err(format!("the installed PHP doesn't say if it is {what}"));
        };
        if bin != source(define) {
            let (is, with) = if bin {
                ("is", "without")
            } else {
                ("isn't", "with it")
            };
            return Err(format!(
                "the installed PHP {is} {what}; the source is configured {with}"
            ));
        }
    }
    Ok(())
}

/// Copy the built `.so` in under a temporary name and rename it into place, so `to`
/// is never a half-written file.
pub(crate) fn install_so(from: &Path, to: &Path) -> Result<()> {
    let dir = to.parent().context("no parent folder")?;
    fs::create_dir_all(dir)?;
    let name = to.file_name().context("no file name")?.to_string_lossy();
    let tmp = dir.join(format!(".{name}.tmp"));
    let ok = fs::copy(from, &tmp)
        .map_err(anyhow::Error::from)
        .and_then(|_| Ok(fs::rename(&tmp, to)?));
    if ok.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    ok
}

/// A failed configure or make, with what to do about the usual reason.
pub(crate) fn tools_error(what: &str, e: anyhow::Error) -> anyhow::Error {
    e.context(format!(
        "Building {what} failed — it needs the Xcode Command Line Tools: xcode-select --install"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Config, ENV_LOCK, home};
    use std::fs;

    #[test]
    fn fpm_args_for_each_state() {
        let dir = tempfile::tempdir().unwrap();
        let _env = crate::lock(&ENV_LOCK);
        // SAFETY: every test that sets ANVIL_HOME holds ENV_LOCK.
        unsafe { std::env::set_var("ANVIL_HOME", dir.path()) };
        let cfg = Config::default();
        fs::create_dir_all(php_dir("8.4").join("ext")).unwrap();
        fs::write(so_path("8.4"), "").unwrap();

        let args = fpm_args(&cfg.ini, "8.4");
        let so = format!(
            "zend_extension={}",
            home().join("php/8.4/ext/opcache.so").display()
        );
        assert_eq!(args[..2], ["-d".to_string(), so]);
        assert!(args.contains(&"opcache.enable=1".into()));
        assert!(args.contains(&"opcache.revalidate_freq=0".into()));
        assert_eq!(state("8.4"), State::Ready);

        assert!(fpm_args(&cfg.ini, "8.3").is_empty());
        assert_eq!(state("8.3"), State::Missing);

        let args = fpm_args(&cfg.ini, "8.5");
        assert!(!args.iter().any(|a| a.starts_with("zend_extension")));
        assert!(args.contains(&"opcache.enable=1".into()));
        assert_eq!(state("8.5"), State::BuiltIn);

        // Removed: back to Missing, no zend_extension; one PHP ships can't be removed.
        remove("8.4").unwrap();
        assert_eq!(state("8.4"), State::Missing);
        assert!(fpm_args(&cfg.ini, "8.4").is_empty());
        assert!(remove("8.4").is_err());
        assert!(remove("8.5").is_err());
    }

    #[test]
    fn sources_of_patches_no_longer_installed_go() {
        let dir = tempfile::tempdir().unwrap();
        let _env = crate::lock(&ENV_LOCK);
        // SAFETY: every test that sets ANVIL_HOME holds ENV_LOCK.
        unsafe { std::env::set_var("ANVIL_HOME", dir.path()) };
        crate::phpext::tests::fake_php("8.4", "8.4.23");
        fs::write(php_dir("8.4").join(crate::PHP_SERVER), "").unwrap();
        let src = home().join("cache/php-src");
        for full in ["8.4.22", "8.4.23", "8.3.30"] {
            fs::create_dir_all(src.join(full).join("stage")).unwrap();
        }
        prune_sources();
        let left: Vec<_> = fs::read_dir(&src)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(left, ["8.4.23"]);
    }

    #[test]
    fn user_ini_wins_over_dev_settings() {
        let mut cfg = Config::default();
        cfg.ini.insert("opcache.enable".into(), "0".into());
        let args = fpm_args(&cfg.ini, "8.5");
        assert!(!args.contains(&"opcache.enable=1".into()));
        assert!(args.contains(&"opcache.enable=0".into()));
        assert!(args.contains(&"opcache.validate_timestamps=1".into()));
        // Read once at FPM start: a pool's php_value would be ignored.
        cfg.ini
            .insert("opcache.memory_consumption".into(), "512".into());
        let args = fpm_args(&cfg.ini, "8.5");
        assert!(args.contains(&"opcache.memory_consumption=512".into()));
        assert!(!args.contains(&"opcache.memory_consumption=256".into()));
    }

    #[test]
    fn mismatched_build_is_refused() {
        let config_h = "#define ZEND_DEBUG 0\n#define ZEND_SIGNALS 1\n";
        let err = build_matches(
            "Thread Safety => enabled\nDebug Build => no\nZend Signals => enabled",
            config_h,
        )
        .unwrap_err();
        assert!(err.contains("thread safe"), "{err}");
        assert_eq!(
            build_matches(
                "Thread Safety => disabled\nDebug Build => no\nZend Signals => enabled",
                config_h,
            ),
            Ok(())
        );
        // How the real `php -i` names it.
        assert_eq!(
            build_matches(
                "Debug Build => no\nThread Safety => disabled\nZend Signal Handling => enabled",
                &format!("/* #undef ZTS */\n{config_h}"),
            ),
            Ok(())
        );
    }

    #[test]
    fn no_published_sum_is_an_error() {
        let none = serde_json::json!({"source": [
            {"filename": "php-8.4.23.tar.xz", "sha256": "aa"}
        ]});
        assert_eq!(
            sum_from(&none, "8.4.23").unwrap_err().to_string(),
            "php.net lists no checksum for php-8.4.23.tar.gz"
        );
        let unknown = serde_json::json!({"error": "Unknown version"});
        assert!(sum_from(&unknown, "8.4.23").is_err());
        let some = serde_json::json!({"source": [
            {"filename": "php-8.4.23.tar.gz", "sha256": "f43b"},
            {"filename": "php-8.4.23.tar.xz", "sha256": "1ab9"}
        ]});
        assert_eq!(sum_from(&some, "8.4.23").unwrap(), "f43b");
    }

    #[test]
    fn only_the_stage_stays() {
        let dir = std::env::temp_dir().join(format!("anvil-prune-{}", std::process::id()));
        fs::create_dir_all(dir.join("stage/include")).unwrap();
        fs::create_dir_all(dir.join("Zend")).unwrap();
        fs::write(dir.join(".anvil-configured"), "").unwrap();
        fs::write(dir.join("stage/.anvil-staged"), "").unwrap();
        prune(&dir);
        let left: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(left, ["stage"]);
        assert!(dir.join("stage/.anvil-staged").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn build_writes_the_so_only_when_done() {
        let dir = tempfile::tempdir().unwrap();
        let to = dir.path().join("ext/opcache.so");
        assert!(install_so(&dir.path().join("missing.so"), &to).is_err());
        assert!(!to.exists());
        let from = dir.path().join("built.so");
        fs::write(&from, "so").unwrap();
        install_so(&from, &to).unwrap();
        assert_eq!(fs::read_to_string(&to).unwrap(), "so");
        // Nothing but the file itself is left in the folder.
        assert_eq!(fs::read_dir(to.parent().unwrap()).unwrap().count(), 1);
    }

    #[test]
    fn missing_tools_is_a_clear_error() {
        let e = tools_error(
            "OPcache",
            anyhow::anyhow!("\"make\" exited with exit status: 1"),
        );
        assert!(e.to_string().contains("xcode-select --install"), "{e}");
    }
}
