//! PHP extensions from PECL (xdebug, pcov, apcu, igbinary, msgpack), built on this Mac
//! with phpize against the source tree OPcache configured for the same exact version.

use crate::opcache::{install_so, installed_full, tools_error};
use crate::{Config, fetch_tgz, home, php_dir, run};
use anyhow::{Context, Result, bail};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct Ext {
    pub name: &'static str,
    pub pecl: &'static str,
    pub version: &'static str,
    /// PECL publishes no checksums: these are of the tarballs as Anvil first saw them.
    pub sha256: &'static str,
    /// Loaded with `zend_extension=`, which PHP reads only when it starts.
    pub zend: bool,
}

/// Each builds for PHP 8.1 to 8.5. igbinary's 3.2.17RC1 is the first with 8.5 support
/// (PECL marks it stable); 3.2.16 doesn't build there.
pub const CATALOG: &[Ext] = &[
    Ext {
        name: "xdebug",
        pecl: "xdebug",
        version: "3.5.3",
        sha256: "f073de91bea046106abf4d6071c963ea71e58571df6ce58948ceca89d121cb2d",
        zend: true,
    },
    Ext {
        name: "pcov",
        pecl: "pcov",
        version: "1.0.12",
        sha256: "23255c8c9335a9636ccb743f5302436a97a582a0bbde9869485be911bbc15da8",
        zend: false,
    },
    Ext {
        name: "apcu",
        pecl: "apcu",
        version: "5.1.28",
        sha256: "ca9c1820810a168786f8048a4c3f8c9e3fd941407ad1553259fb2e30b5f057bf",
        zend: false,
    },
    Ext {
        name: "igbinary",
        pecl: "igbinary",
        version: "3.2.17RC1",
        sha256: "91da821443db125282a6aea039f24588dd28ff5d71e8187f6ecc41165bceafbc",
        zend: false,
    },
    Ext {
        name: "msgpack",
        pecl: "msgpack",
        version: "3.0.1",
        sha256: "e30be355ab79aaea4568692fbd6073fd7c7f50ea8d3cf12edce40fc0c921d868",
        zend: false,
    },
];

/// GNU m4 and autoconf, which phpize needs and the Xcode Command Line Tools lack (macOS's
/// m4 1.4.6 is too old for autoconf). SHA-256 of ftp.gnu.org's tarballs, whose GPG
/// signatures checked out against the GNU keyring when pinned.
const GNU: [(&str, &str, &str); 2] = [
    (
        "m4",
        "1.4.19",
        "3be4a26d825ffdfda52a56fc43246456989a3630093cced3fbddf4771ee58a70",
    ),
    (
        "autoconf",
        "2.72",
        "afb181a76e1ee72832f6581c0eddf8df032b83e2e0239ef79ebedc4467d92d6e",
    ),
];

/// Xdebug's settings unless the user's php.ini overrides say otherwise: no slowdown
/// until a request asks for the debugger.
const XDEBUG: [(&str, &str); 2] = [
    ("xdebug.mode", "debug"),
    ("xdebug.start_with_request", "trigger"),
];

/// Rebuildable: m4 and autoconf, built again when an extension needs them.
pub fn tools_dir() -> PathBuf {
    home().join("tools")
}

fn find(name: &str) -> Result<&'static Ext> {
    CATALOG
        .iter()
        .find(|e| e.name == name)
        .with_context(|| format!("{name} is not in Anvil's catalog"))
}

/// Keyed by the exact version: a build is never loaded into another patch release.
pub fn so_path(minor: &str, full: &str, name: &str) -> PathBuf {
    php_dir(minor)
        .join("ext")
        .join(full)
        .join(format!("{name}.so"))
}

/// Whether `name` is built for the PHP `minor` installed now.
pub fn built(minor: &str, name: &str) -> bool {
    installed_full(minor).is_ok_and(|full| so_path(minor, &full, name).exists())
}

/// Build `name` for the installed PHP `minor` and put it in place once PHP loads it.
pub fn build(minor: &str, name: &str) -> Result<()> {
    let ext = find(name)?;
    let _held = build_lock(minor, name, name)?;
    let (full, stage) = crate::opcache::stage(minor)?;
    let bin = tools()?;
    println!("Building {name} {} for PHP {full}…", ext.version);
    let dir = home()
        .join("build")
        .join(format!("{}-{}-php{full}", ext.pecl, ext.version));
    let _ = fs::remove_dir_all(&dir);
    let file = format!("{}-{}", ext.pecl, ext.version);
    fetch_tgz(
        &format!("https://pecl.php.net/get/{file}.tgz"),
        &dir,
        true,
        Some(ext.sha256),
    )
    .with_context(|| format!("couldn't get {file} from pecl.php.net"))?;
    let ok = compile(&dir, &stage, &bin, name).and_then(|so| {
        check_loads(minor, ext, &so)?;
        install_so(&so, &so_path(minor, &full, name))
    });
    let _ = fs::remove_dir_all(&dir);
    ok?;
    println!("{name} built for PHP {full}");
    Ok(())
}

/// Held while `name` builds for PHP `minor`, in this process or another (the CLI and the
/// app): a second build of the same is refused at once. Different builds then take
/// turns, as they share the tools, the source tree and its staged headers.
pub(crate) fn build_lock(minor: &str, name: &str, what: &str) -> Result<[fs::File; 2]> {
    let dir = home().join("run");
    fs::create_dir_all(&dir)?;
    let open = |f: String| fs::File::create(dir.join(f));
    let mine = open(format!("build-{minor}-{name}.lock"))?;
    match mine.try_lock() {
        Ok(()) => {}
        Err(fs::TryLockError::WouldBlock) => {
            bail!("{what} for PHP {minor} is already being built")
        }
        Err(fs::TryLockError::Error(e)) => return Err(e.into()),
    }
    let all = open("build.lock".into())?;
    all.lock()?;
    Ok([mine, all])
}

/// Turn `name` on for every app on the installed PHP `minor`, building it first unless
/// this patch release has it already; the saved config. FPM loads it at its next start.
pub fn add(minor: &str, name: &str) -> Result<Config> {
    find(name)?;
    if !crate::installed_php().iter().any(|v| v == minor) {
        bail!("PHP {minor} is not installed — run `anvil php install {minor}`");
    }
    if !built(minor, name) {
        build(minor, name)?;
    }
    let (cfg, ()) = crate::update(|c| {
        let names = c.php_ext.entry(minor.into()).or_default();
        if !names.iter().any(|n| n == name) {
            names.push(name.into());
        }
        Ok(())
    })?;
    Ok(cfg)
}

/// Turn `name` off for PHP `minor`. Its `.so` stays: turning it on again is instant.
pub fn remove(minor: &str, name: &str) -> Result<Config> {
    find(name)?;
    let (cfg, ()) = crate::update(|c| {
        if let Some(names) = c.php_ext.get_mut(minor) {
            names.retain(|n| n != name);
            if names.is_empty() {
                c.php_ext.remove(minor);
            }
        }
        Ok(())
    })?;
    Ok(cfg)
}

/// Turn `name` on for one app only (a PHP-FPM master of its own: `mark_own_masters`),
/// building it first for the app's PHP unless built; the saved config.
pub fn add_site(site: &str, name: &str) -> Result<Config> {
    find(name)?;
    let site = crate::site_name(site);
    let s = crate::sites(&crate::load()?)
        .into_iter()
        .find(|s| s.name == site)
        .with_context(|| format!("No site named {site}"))?;
    let minor = s
        .php
        .with_context(|| format!("{site} runs on no PHP of Anvil's"))?;
    if !built(&minor, name) {
        build(&minor, name)?;
    }
    let (cfg, ()) = crate::update(|c| {
        let names = &mut c.sites.entry(site.clone()).or_default().ext;
        if !names.iter().any(|n| n == name) {
            names.push(name.into());
        }
        Ok(())
    })?;
    Ok(cfg)
}

/// Turn one app's own `name` off; its `.so` stays.
pub fn remove_site(site: &str, name: &str) -> Result<Config> {
    find(name)?;
    let site = crate::site_name(site);
    let (cfg, ()) = crate::update(|c| {
        if let Some(s) = c.sites.get_mut(&site) {
            s.ext.retain(|n| n != name);
        }
        Ok(())
    })?;
    Ok(cfg)
}

/// Whether `name` is on for PHP `minor`.
pub fn on(cfg: &Config, minor: &str, name: &str) -> bool {
    cfg.php_ext
        .get(minor)
        .is_some_and(|n| n.iter().any(|x| x == name))
}

/// The `-d` arguments every PHP Anvil starts on `minor` gets (FPM, the terminal shims,
/// app processes): OPcache first, as it must load before Xdebug, then the extensions
/// turned on for the version.
pub fn php_args(cfg: &Config, minor: &str) -> Vec<String> {
    php_args_with(cfg, minor, &[])
}

/// `php_args` with one app's own extensions (`Site::ext`) too, each loaded once.
pub fn php_args_with(cfg: &Config, minor: &str, own: &[String]) -> Vec<String> {
    let mut args = crate::opcache::fpm_args(cfg, minor);
    let mut names = cfg.php_ext.get(minor).cloned().unwrap_or_default();
    for n in own {
        if !names.contains(n) {
            names.push(n.clone());
        }
    }
    if !names.is_empty() {
        args.extend(ini_args(&cfg.ini, minor, &names));
    }
    args
}

/// `php_args_with` and then the php.ini values PHP-FPM's pool gives `site` (or, without
/// one, the shared pool): the global ones, the app's own over them. What a PHP Anvil
/// starts outside PHP-FPM reads. The CLI keeps its own `max_execution_time=0` and
/// `max_input_time=-1`: PHP sets them over any ini file.
pub fn cli_args(cfg: &Config, minor: &str, site: Option<&crate::Site>) -> Vec<String> {
    let mut args = php_args_with(cfg, minor, site.map_or(&[], |s| &s.ext));
    let mut ini = cfg.ini.clone();
    ini.extend(site.map(|s| s.ini.clone()).unwrap_or_default());
    for (k, v) in ini {
        let line = format!("{k}={v}");
        // An opcache.* or xdebug.* value php_args already has.
        if !args.contains(&line) {
            args.extend(["-d".into(), line]);
        }
    }
    args
}

/// `PHP_INI_SCAN_DIR` for a PHP Anvil starts on `minor` other than PHP-FPM (the shims,
/// an app's processes): `cli_args` as an ini file in `php_dir(minor)/conf.d`, so a PHP
/// those start in turn (PHP_BINARY: queue:listen's workers; `npm run dev` → `php
/// artisan`; composer scripts) loads the same. Never with the `-d`s too: loaded twice
/// warns. `None` when there is nothing to load.
/// An app with extensions or php.ini values of its own (`site`) gets a folder of its
/// own in `run`.
pub fn scan_env(
    cfg: &Config,
    minor: &str,
    site: Option<&crate::Site>,
) -> Result<Option<(String, String)>> {
    match site.filter(|s| !s.ext.is_empty() || !s.ini.is_empty()) {
        Some(s) => {
            let dir = home()
                .join("run")
                .join(format!("php-{minor}-{}.d", crate::pool(s)));
            scan_env_in(&dir, &cli_args(cfg, minor, Some(s)))
        }
        None => scan_env_in(&php_dir(minor).join("conf.d"), &cli_args(cfg, minor, None)),
    }
}

/// `args` (`-d` pairs) as `dir/anvil.ini`, rewritten only when it differs, and the
/// `PHP_INI_SCAN_DIR` that reads it: ours first, then the user's own, else PHP's
/// built-in folder (the empty entry). An Anvil folder in the user's (a terminal opened
/// from an app process) is left out: never two of Anvil's at once.
pub(crate) fn scan_env_in(dir: &Path, args: &[String]) -> Result<Option<(String, String)>> {
    if args.is_empty() {
        return Ok(None);
    }
    let text: String = args.chunks(2).map(|c| format!("{}\n", c[1])).collect();
    let file = dir.join("anvil.ini");
    if fs::read_to_string(&file).ok().as_deref() != Some(text.as_str()) {
        fs::create_dir_all(dir)?;
        let tmp = dir.join(format!(".anvil.ini.{}", std::process::id()));
        fs::write(&tmp, &text)?;
        fs::rename(&tmp, &file)?;
    }
    let home = home();
    let theirs = std::env::var("PHP_INI_SCAN_DIR").unwrap_or_default();
    let mut dirs = vec![dir.display().to_string()];
    if theirs.is_empty() {
        dirs.push(String::new());
    } else {
        dirs.extend(
            theirs
                .split(':')
                .filter(|p| !Path::new(p).starts_with(&home))
                .map(String::from),
        );
    }
    Ok(Some(("PHP_INI_SCAN_DIR".into(), dirs.join(":"))))
}

/// phpize, configure and make in `dir` against the staged PHP; the built `.so`.
fn compile(dir: &Path, stage: &Path, bin: &Path, name: &str) -> Result<PathBuf> {
    let jobs = std::thread::available_parallelism().map_or(4, |n| n.get());
    let steps: [(&Path, Vec<String>); 3] = [
        (&stage.join("bin/phpize"), vec![]),
        (
            Path::new("./configure"),
            vec![
                "--quiet".into(),
                format!(
                    "--with-php-config={}",
                    stage.join("bin/php-config").display()
                ),
            ],
        ),
        (Path::new("make"), vec!["-s".into(), format!("-j{jobs}")]),
    ];
    for (prog, args) in steps {
        run(Command::new(prog)
            .args(args)
            .env("PHP_AUTOCONF", bin.join("autoconf"))
            .env("PHP_AUTOHEADER", bin.join("autoheader"))
            .current_dir(dir))
        .map_err(|e| tools_error(name, e))?;
    }
    Ok(dir.join("modules").join(format!("{name}.so")))
}

/// PHP lists the module with the `.so` loaded: a build it rejects would only print a
/// warning on every start.
fn check_loads(minor: &str, ext: &Ext, so: &Path) -> Result<()> {
    let out = Command::new(php_dir(minor).join("php"))
        .arg("-n")
        .arg("-d")
        .arg(format!("{}={}", directive(ext), so.display()))
        .arg("-m")
        .output()?;
    if !String::from_utf8_lossy(&out.stdout)
        .lines()
        .any(|l| l.trim().eq_ignore_ascii_case(ext.name))
    {
        anyhow::bail!("the {} built for PHP {minor} doesn't load", ext.name);
    }
    Ok(())
}

fn directive(ext: &Ext) -> &'static str {
    if ext.zend {
        "zend_extension"
    } else {
        "extension"
    }
}

/// m4 and autoconf in `~/.anvil/tools`, built from GNU's source the first time; their
/// `bin` folder.
fn tools() -> Result<PathBuf> {
    let dir = tools_dir();
    let jobs = std::thread::available_parallelism().map_or(4, |n| n.get());
    for (name, version, sum) in GNU {
        let done = dir.join(format!(".{name}-{version}"));
        if done.exists() {
            continue;
        }
        println!("Building {name} {version}, which PHP extensions need (once)…");
        let src = home().join("build").join(format!("{name}-{version}"));
        let _ = fs::remove_dir_all(&src);
        let file = format!("{name}-{version}");
        fetch_tgz(
            &format!("https://ftp.gnu.org/gnu/{name}/{file}.tar.gz"),
            &src,
            true,
            Some(sum),
        )
        .with_context(|| format!("couldn't get {file} from ftp.gnu.org"))?;
        let prefix = format!("--prefix={}", dir.display());
        let steps: [(&str, Vec<String>); 3] = [
            ("./configure", vec!["--quiet".into(), prefix]),
            ("make", vec!["-s".into(), format!("-j{jobs}")]),
            ("make", vec!["-s".into(), "install".into()]),
        ];
        for (prog, args) in steps {
            let mut cmd = Command::new(prog);
            if name == "autoconf" {
                // autoconf remembers this m4, so the old one in /usr/bin is never used.
                cmd.env("M4", dir.join("bin/m4"));
            }
            run(cmd.args(args).current_dir(&src)).map_err(|e| tools_error(&file, e))?;
        }
        let _ = fs::remove_dir_all(&src);
        fs::write(&done, "")?;
    }
    Ok(dir.join("bin"))
}

/// The `-d` arguments that load `names` built for the installed PHP `minor` (a name
/// without a build adds nothing), then Xdebug's settings when it is one of them: the
/// user's `xdebug.*` overrides, and the defaults they don't set. Xdebug reads them when
/// PHP starts, so a pool's php_value would be too late.
pub fn ini_args(ini: &BTreeMap<String, String>, minor: &str, names: &[String]) -> Vec<String> {
    let Ok(full) = installed_full(minor) else {
        return vec![];
    };
    let mut args = vec![];
    for ext in CATALOG.iter().filter(|e| names.iter().any(|n| n == e.name)) {
        let so = so_path(minor, &full, ext.name);
        if !so.exists() {
            continue;
        }
        args.extend(["-d".into(), format!("{}={}", directive(ext), so.display())]);
        if ext.name == "xdebug" {
            let dev = XDEBUG.iter().filter(|(k, _)| !ini.contains_key(*k));
            let mine = ini.iter().filter(|(k, _)| k.starts_with("xdebug."));
            for (k, v) in dev
                .map(|(k, v)| (*k, *v))
                .chain(mine.map(|(k, v)| (k.as_str(), v.as_str())))
            {
                args.extend(["-d".into(), format!("{k}={v}")]);
            }
        }
    }
    args
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::ENV_LOCK;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    /// A `php` in a temp ANVIL_HOME that says it is `full`.
    pub(crate) fn fake_php(minor: &str, full: &str) {
        let dir = php_dir(minor);
        fs::create_dir_all(&dir).unwrap();
        let php = dir.join("php");
        fs::write(&php, format!("#!/bin/sh\nprintf {full}\n")).unwrap();
        fs::set_permissions(&php, fs::Permissions::from_mode(0o755)).unwrap();
    }

    pub(crate) fn fake_so(minor: &str, full: &str, name: &str) -> String {
        let so = so_path(minor, full, name);
        fs::create_dir_all(so.parent().unwrap()).unwrap();
        fs::write(&so, "").unwrap();
        so.display().to_string()
    }

    #[test]
    fn catalog_is_pinned() {
        let hex = |s: &str| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit());
        let names: Vec<_> = CATALOG.iter().map(|e| e.name).collect();
        assert_eq!(names, ["xdebug", "pcov", "apcu", "igbinary", "msgpack"]);
        for e in CATALOG {
            assert!(hex(e.sha256), "{}", e.name);
            assert!(!e.version.is_empty() && !e.pecl.is_empty(), "{}", e.name);
            assert_eq!(e.zend, e.name == "xdebug", "{}", e.name);
        }
        assert_eq!(
            GNU.map(|(n, v, _)| (n, v)),
            [("m4", "1.4.19"), ("autoconf", "2.72")]
        );
        assert!(GNU.iter().all(|(_, _, s)| hex(s)));
    }

    #[test]
    fn so_is_keyed_by_full_version() {
        let dir = tempfile::tempdir().unwrap();
        let _env = crate::lock(&ENV_LOCK);
        // SAFETY: every test that sets ANVIL_HOME holds ENV_LOCK.
        unsafe { std::env::set_var("ANVIL_HOME", dir.path()) };
        assert_ne!(
            so_path("8.4", "8.4.23", "apcu"),
            so_path("8.4", "8.4.24", "apcu")
        );
        assert_eq!(
            so_path("8.4", "8.4.23", "apcu"),
            php_dir("8.4").join("ext/8.4.23/apcu.so")
        );
        assert!(!built("8.4", "apcu"), "PHP isn't installed");
        fake_php("8.4", "8.4.23");
        assert!(!built("8.4", "apcu"));
        fake_so("8.4", "8.4.23", "apcu");
        assert!(built("8.4", "apcu"));
        // A reinstall brought 8.4.24: the 8.4.23 build is never loaded into it.
        fake_php("8.4", "8.4.24");
        assert!(!built("8.4", "apcu"));
        assert!(ini_args(&BTreeMap::new(), "8.4", &["apcu".into()]).is_empty());
    }

    #[test]
    fn ini_args_zend_and_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let _env = crate::lock(&ENV_LOCK);
        // SAFETY: every test that sets ANVIL_HOME holds ENV_LOCK.
        unsafe { std::env::set_var("ANVIL_HOME", dir.path()) };
        fake_php("8.4", "8.4.23");
        let xdebug = fake_so("8.4", "8.4.23", "xdebug");
        let apcu = fake_so("8.4", "8.4.23", "apcu");
        let names: Vec<String> = ["xdebug", "apcu", "pcov"].map(String::from).into();
        let mut ini = BTreeMap::new();
        let args = ini_args(&ini, "8.4", &names);
        let d = |v: &str| ["-d".to_string(), v.to_string()];
        assert_eq!(
            args,
            [
                d(&format!("zend_extension={xdebug}")),
                d("xdebug.mode=debug"),
                d("xdebug.start_with_request=trigger"),
                d(&format!("extension={apcu}")),
            ]
            .concat(),
            "pcov isn't built: no argument for it"
        );
        ini.insert("xdebug.mode".into(), "coverage".into());
        ini.insert("xdebug.client_port".into(), "9010".into());
        ini.insert("memory_limit".into(), "1G".into());
        let args = ini_args(&ini, "8.4", &names);
        assert!(args.contains(&"xdebug.mode=coverage".into()));
        assert!(!args.contains(&"xdebug.mode=debug".into()));
        assert!(args.contains(&"xdebug.start_with_request=trigger".into()));
        assert!(args.contains(&"xdebug.client_port=9010".into()));
        assert!(!args.iter().any(|a| a.starts_with("memory_limit")));
        // Without xdebug, its settings stay out.
        let args = ini_args(&ini, "8.4", &["apcu".into()]);
        assert_eq!(args, d(&format!("extension={apcu}")));
    }

    #[test]
    fn removed_ext_leaves_no_arg() {
        let dir = tempfile::tempdir().unwrap();
        let _env = crate::lock(&ENV_LOCK);
        // SAFETY: every test that sets ANVIL_HOME holds ENV_LOCK.
        unsafe { std::env::set_var("ANVIL_HOME", dir.path()) };
        fake_php("8.4", "8.4.23");
        fs::write(php_dir("8.4").join("php-fpm"), "").unwrap();
        let apcu = fake_so("8.4", "8.4.23", "apcu");
        let xdebug = fake_so("8.4", "8.4.23", "xdebug");
        crate::save(&Config::default()).unwrap();
        add("8.4", "xdebug").unwrap();
        let cfg = add("8.4", "apcu").unwrap();
        assert_eq!(cfg.php_ext["8.4"], ["xdebug", "apcu"]);
        let args = php_args(&cfg, "8.4");
        assert!(args.contains(&format!("extension={apcu}")), "{args:?}");
        assert!(
            args.contains(&format!("zend_extension={xdebug}")),
            "{args:?}"
        );
        assert!(php_args(&cfg, "8.3").is_empty(), "on for 8.4 only");

        let cfg = remove("8.4", "apcu").unwrap();
        assert!(!php_args(&cfg, "8.4").iter().any(|a| a.contains("apcu")));
        assert!(fs::metadata(&apcu).is_ok(), "the build stays for next time");
        let cfg = remove("8.4", "xdebug").unwrap();
        assert!(cfg.php_ext.is_empty(), "nothing left for 8.4: not written");
        assert!(!php_args(&cfg, "8.4").iter().any(|a| a.contains("xdebug")));

        // A `.so` deleted behind Anvil's back: no argument points at it.
        let cfg = add("8.4", "apcu").unwrap();
        fs::remove_file(&apcu).unwrap();
        assert!(!php_args(&cfg, "8.4").iter().any(|a| a.contains("apcu")));
        assert!(add("8.4", "swoole").is_err());
        let e = add("8.3", "apcu").err().unwrap();
        assert!(e.to_string().contains("not installed"), "{e}");
    }

    #[test]
    fn scan_dir_ini_and_the_users_own() {
        let dir = tempfile::tempdir().unwrap();
        let _env = crate::lock(&ENV_LOCK);
        // SAFETY: every test that sets ANVIL_HOME holds ENV_LOCK; so does PHP_INI_SCAN_DIR.
        unsafe { std::env::set_var("ANVIL_HOME", dir.path()) };
        let conf = dir.path().join("php/8.4/conf.d");
        let args: Vec<String> = ["-d", "zend_extension=/o.so", "-d", "opcache.enable=1"]
            .map(String::from)
            .into();
        assert_eq!(scan_env_in(&conf, &[]).unwrap(), None);
        assert!(!conf.exists(), "nothing to load: no file");
        let env = |theirs: Option<&str>| {
            match theirs {
                // SAFETY: as above.
                Some(t) => unsafe { std::env::set_var("PHP_INI_SCAN_DIR", t) },
                None => unsafe { std::env::remove_var("PHP_INI_SCAN_DIR") },
            }
            scan_env_in(&conf, &args).unwrap().unwrap().1
        };
        let ours = conf.display().to_string();
        assert_eq!(env(None), format!("{ours}:"), "then PHP's built-in folder");
        assert_eq!(
            fs::read_to_string(conf.join("anvil.ini")).unwrap(),
            "zend_extension=/o.so\nopcache.enable=1\n"
        );
        assert_eq!(env(Some("/etc/php.d")), format!("{ours}:/etc/php.d"));
        // From an app process (ours already there, maybe another version's): once.
        let other = dir.path().join("php/8.3/conf.d");
        assert_eq!(
            env(Some(&format!("{}::/etc/php.d", other.display()))),
            format!("{ours}::/etc/php.d")
        );
        unsafe { std::env::remove_var("PHP_INI_SCAN_DIR") };
    }

    #[test]
    fn second_build_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let _env = crate::lock(&ENV_LOCK);
        // SAFETY: every test that sets ANVIL_HOME holds ENV_LOCK.
        unsafe { std::env::set_var("ANVIL_HOME", dir.path()) };
        let first = build_lock("8.4", "xdebug", "xdebug").unwrap();
        let e = build("8.4", "xdebug").unwrap_err();
        assert_eq!(e.to_string(), "xdebug for PHP 8.4 is already being built");
        drop(first);
        let op = build_lock("8.4", "opcache", "OPcache").unwrap();
        let e = crate::opcache::build("8.4").unwrap_err();
        assert_eq!(e.to_string(), "OPcache for PHP 8.4 is already being built");
        drop(op);
        // Free again: the next one gets past the lock (to the PHP that isn't there).
        let e = build("8.4", "xdebug").unwrap_err();
        assert!(!e.to_string().contains("already"), "{e}");
        let e = crate::opcache::build("8.4").unwrap_err();
        assert!(!e.to_string().contains("already"), "{e}");
    }

    #[test]
    fn build_errors_are_clear() {
        let e = build("8.4", "swoole").unwrap_err();
        assert_eq!(e.to_string(), "swoole is not in Anvil's catalog");
        let e = tools_error(
            "xdebug",
            anyhow::anyhow!("\"make\" exited with exit status: 2"),
        );
        assert!(format!("{e:#}").contains("xcode-select --install"), "{e:#}");
        assert!(
            format!("{e:#}").starts_with("Building xdebug failed"),
            "{e:#}"
        );
    }
}
