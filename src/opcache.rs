//! OPcache for PHP 8.1–8.4: the default builds lack it, so Anvil builds `opcache.so` from
//! php.net's source of the same patch version on this Mac. 8.5 has it built in.

use crate::{Config, fetch_json, fetch_tgz, home, php_dir, run};
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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
    if !BUILDS.contains(&minor) {
        State::BuiltIn
    } else if so_path(minor).exists() {
        State::Ready
    } else {
        State::Missing
    }
}

/// PHP-FPM's `-d` arguments: the extension when built, then the dev settings the user
/// hasn't set in the global php.ini overrides (theirs reach FPM through the pool).
pub fn fpm_args(cfg: &Config, minor: &str) -> Vec<String> {
    let mut args = match state(minor) {
        State::Missing => return vec![],
        State::BuiltIn => vec![],
        State::Ready => vec![
            "-d".into(),
            format!("zend_extension={}", so_path(minor).display()),
        ],
    };
    // OPcache reads most of its settings once, when FPM starts: a pool's php_value
    // comes too late, so the user's opcache.* values go here too, over the dev ones.
    let mine = cfg.ini.iter().filter(|(k, _)| k.starts_with("opcache."));
    let dev = DEV.iter().filter(|(k, _)| !cfg.ini.contains_key(*k));
    for (k, v) in dev
        .map(|(k, v)| (*k, *v))
        .chain(mine.map(|(k, v)| (k.as_str(), v.as_str())))
    {
        args.extend(["-d".into(), format!("{k}={v}")]);
    }
    args
}

/// Build `opcache.so` for the installed PHP `minor` from php.net's source of its exact
/// version, verified against php.net's checksum, and put it in place once it loads.
pub fn build(minor: &str) -> Result<()> {
    if !BUILDS.contains(&minor) {
        bail!("PHP {minor} has OPcache built in");
    }
    let php = php_dir(minor).join("php");
    let out = Command::new(&php)
        .args(["-n", "-r", "echo PHP_VERSION;"])
        .output()
        .with_context(|| format!("PHP {minor} is not installed"))?;
    let full = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() || !full.starts_with(&format!("{minor}.")) {
        bail!("couldn't read the version of PHP {minor}");
    }
    println!("Building OPcache for PHP {full}…");
    let sum = source_sum(&full)?;
    let src = home().join("cache").join("php-src").join(&full);
    let _ = fs::remove_dir_all(&src);
    fetch_tgz(
        &format!("https://www.php.net/distributions/php-{full}.tar.gz"),
        &src,
        true,
        Some(&sum),
    )?;
    run(Command::new("./configure")
        .args([
            "--quiet",
            "--disable-all",
            "--disable-cgi",
            "--disable-phpdbg",
            "--without-sqlite3",
            "--without-pdo-sqlite",
            "--enable-opcache=shared",
        ])
        .current_dir(&src))
    .map_err(tools_error)?;
    let info = Command::new(&php).args(["-n", "-i"]).output()?;
    let config_h = fs::read_to_string(src.join("main/php_config.h"))?;
    build_matches(&String::from_utf8_lossy(&info.stdout), &config_h)
        .map_err(|why| anyhow::anyhow!("OPcache can't be built for PHP {full}: {why}"))?;
    let jobs = std::thread::available_parallelism().map_or(4, |n| n.get());
    run(Command::new("make")
        .args(["-s", &format!("-j{jobs}")])
        .current_dir(&src))
    .map_err(tools_error)?;
    let built = src.join("modules/opcache.so");
    // A `.so` PHP rejects would only print a warning on every start: check it first.
    let check = Command::new(&php)
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
    Ok(())
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
fn install_so(from: &Path, to: &Path) -> Result<()> {
    let dir = to.parent().context("no parent folder")?;
    fs::create_dir_all(dir)?;
    let tmp = dir.join(".opcache.so.tmp");
    let ok = fs::copy(from, &tmp)
        .map_err(anyhow::Error::from)
        .and_then(|_| Ok(fs::rename(&tmp, to)?));
    if ok.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    ok
}

fn tools_error(e: anyhow::Error) -> anyhow::Error {
    e.context(
        "Building OPcache failed — OPcache needs the Xcode Command Line Tools: xcode-select --install",
    )
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

        let args = fpm_args(&cfg, "8.4");
        let so = format!(
            "zend_extension={}",
            home().join("php/8.4/ext/opcache.so").display()
        );
        assert_eq!(args[..2], ["-d".to_string(), so]);
        assert!(args.contains(&"opcache.enable=1".into()));
        assert!(args.contains(&"opcache.revalidate_freq=0".into()));
        assert_eq!(state("8.4"), State::Ready);

        assert!(fpm_args(&cfg, "8.3").is_empty());
        assert_eq!(state("8.3"), State::Missing);

        let args = fpm_args(&cfg, "8.5");
        assert!(!args.iter().any(|a| a.starts_with("zend_extension")));
        assert!(args.contains(&"opcache.enable=1".into()));
        assert_eq!(state("8.5"), State::BuiltIn);
    }

    #[test]
    fn user_ini_wins_over_dev_settings() {
        let mut cfg = Config::default();
        cfg.ini.insert("opcache.enable".into(), "0".into());
        let args = fpm_args(&cfg, "8.5");
        assert!(!args.contains(&"opcache.enable=1".into()));
        assert!(args.contains(&"opcache.enable=0".into()));
        assert!(args.contains(&"opcache.validate_timestamps=1".into()));
        // Read once at FPM start: a pool's php_value would be ignored.
        cfg.ini
            .insert("opcache.memory_consumption".into(), "512".into());
        let args = fpm_args(&cfg, "8.5");
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
        let e = tools_error(anyhow::anyhow!("\"make\" exited with exit status: 1"));
        assert!(e.to_string().contains("xcode-select --install"), "{e}");
    }
}
