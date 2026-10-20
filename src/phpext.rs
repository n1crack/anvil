//! PHP extensions from PECL (xdebug, pcov, apcu, igbinary, msgpack), built on this Mac
//! with phpize against the source tree OPcache configured for the same exact version.

use crate::opcache::{install_so, installed_full, tools_error};
use crate::{Config, fetch_tgz, home, php_dir, run};
use anyhow::{Context, Result, bail};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub struct Ext {
    pub name: &'static str,
    pub pecl: &'static str,
    pub version: &'static str,
    /// PECL publishes no checksums: these are of the tarballs as Anvil first saw them.
    pub sha256: &'static str,
    /// Loaded with `zend_extension=`, which PHP reads only when it starts.
    pub zend: bool,
    /// Not offered for a version or an app (`offered`): built and loaded for one process
    /// only (Swoole, for Octane's).
    pub hidden: bool,
    /// What the build needs beyond phpize's defaults, for the PHP `minor`.
    pub recipe: Option<fn(&str) -> Result<Recipe>>,
}

/// Environment and `./configure` flags an extension's build adds.
#[derive(Default)]
pub struct Recipe {
    pub env: Vec<(String, String)>,
    pub flags: Vec<String>,
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
        hidden: false,
        recipe: None,
    },
    Ext {
        name: "pcov",
        pecl: "pcov",
        version: "1.0.12",
        sha256: "23255c8c9335a9636ccb743f5302436a97a582a0bbde9869485be911bbc15da8",
        zend: false,
        hidden: false,
        recipe: None,
    },
    Ext {
        name: "apcu",
        pecl: "apcu",
        version: "5.1.28",
        sha256: "ca9c1820810a168786f8048a4c3f8c9e3fd941407ad1553259fb2e30b5f057bf",
        zend: false,
        hidden: false,
        recipe: None,
    },
    Ext {
        name: "igbinary",
        pecl: "igbinary",
        version: "3.2.17RC1",
        sha256: "91da821443db125282a6aea039f24588dd28ff5d71e8187f6ecc41165bceafbc",
        zend: false,
        hidden: false,
        recipe: None,
    },
    Ext {
        name: "msgpack",
        pecl: "msgpack",
        version: "3.0.1",
        sha256: "e30be355ab79aaea4568692fbd6073fd7c7f50ea8d3cf12edce40fc0c921d868",
        zend: false,
        hidden: false,
        recipe: None,
    },
    // Octane's server; loaded only into the Octane process (`octane::swoole_env`).
    Ext {
        name: "swoole",
        pecl: "swoole",
        version: "6.2.3",
        sha256: "21dfa1c4a4a09f4d1cb4b154f7defdc91fbabafc8d44a77f8f618b9742c67304",
        zend: false,
        hidden: true,
        recipe: Some(swoole_recipe),
    },
];

/// The OpenSSL releases Anvil's PHP builds link statically (`OPENSSL_VERSION_TEXT`), whose
/// headers Swoole compiles against: SHA-256 of GitHub's release tarballs, as the release's
/// `.sha256` file and GitHub's asset digest both list them.
const OPENSSL: [(&str, &str); 2] = [
    (
        "3.6.1",
        "b1bfedcd5b289ff22aee87c9d600f515767ebf45f77168cb6d64f231f518a82e",
    ),
    (
        "3.6.2",
        "aaf51a1fe064384f811daeaeb4ec4dce7340ec8bd893027eee676af31e83a04f",
    ),
];

/// The catalog the extensions UI, `anvil php ext` and `.anvil.toml` offer.
pub fn offered() -> impl Iterator<Item = &'static Ext> {
    CATALOG.iter().filter(|e| !e.hidden)
}

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
    offered()
        .find(|e| e.name == name)
        .with_context(|| format!("{name} is not in Anvil's catalog"))
}

/// Keyed by the exact version: a build is never loaded into another patch release.
pub fn so_path(minor: &str, full: &str, name: &str) -> PathBuf {
    php_dir(minor)
        .join("ext")
        .join(full)
        .join(if cfg!(windows) {
            format!("php_{name}.dll")
        } else {
            format!("{name}.so")
        })
}

/// Whether `name` is built for the PHP `minor` installed now.
pub fn built(minor: &str, name: &str) -> bool {
    installed_full(minor).is_ok_and(|full| so_path(minor, &full, name).exists())
}

/// The catalog extensions built for the PHP `minor` installed now (one PHP run): one
/// turned on without a build there loads nothing and shows as not built.
pub fn built_names(minor: &str) -> Vec<&'static str> {
    let Ok(full) = installed_full(minor) else {
        return vec![];
    };
    offered()
        .filter(|e| so_path(minor, &full, e.name).exists())
        .map(|e| e.name)
        .collect()
}

/// Build `name` for the installed PHP `minor` and put it in place once PHP loads it.
pub fn build(minor: &str, name: &str) -> Result<()> {
    let ext = CATALOG
        .iter()
        .find(|e| e.name == name)
        .with_context(|| format!("{name} is not in Anvil's catalog"))?;
    let _held = build_lock(minor, name, name)?;
    if cfg!(windows) {
        return fetch_dll(minor, ext);
    }
    // First: it may refuse before anything is downloaded.
    let recipe = ext.recipe.map_or(Ok(Recipe::default()), |r| r(minor))?;
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
    let ok = compile(&dir, &stage, &bin, name, &recipe).and_then(|so| {
        check_loads(minor, ext, &so)?;
        install_so(&so, &so_path(minor, &full, name))
    });
    let _ = fs::remove_dir_all(&dir);
    ok?;
    println!("{name} built for PHP {full}");
    Ok(())
}

/// Windows: PHP's own builds of the extension (windows.php.net's PECL folder) for the
/// installed PHP `minor`, thread-safety off, x64, in place once PHP loads it. Like the
/// tarballs, these come with no checksums; HTTPS is the check.
fn fetch_dll(minor: &str, ext: &Ext) -> Result<()> {
    let full = installed_full(minor)?;
    // The compiler PHP itself was built with: Visual Studio 2019 up to 8.3, 2022 after.
    let vs = if ["8.0", "8.1", "8.2", "8.3"].contains(&minor) {
        "vs16"
    } else {
        "vs17"
    };
    let (pecl, v) = (ext.pecl, ext.version);
    let url = format!(
        "https://downloads.php.net/~windows/pecl/releases/{pecl}/{v}/php_{pecl}-{}-{minor}-nts-{vs}-x64.zip",
        v.to_ascii_lowercase()
    );
    println!("Downloading {} {v} for PHP {full}…", ext.name);
    let dir = home()
        .join("build")
        .join(format!("{pecl}-{v}-php{full}-win"));
    let _ = fs::remove_dir_all(&dir);
    let dll = dir.join(format!("php_{pecl}.dll"));
    let ok = fetch_tgz(&url, &dir, false, None)
        .with_context(|| {
            format!(
                "couldn't get {} for PHP {minor} from windows.php.net",
                ext.name
            )
        })
        .and_then(|()| check_loads(minor, ext, &dll))
        .and_then(|()| install_so(&dll, &so_path(minor, &full, ext.name)));
    let _ = fs::remove_dir_all(&dir);
    ok?;
    println!("{} ready for PHP {full}", ext.name);
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
    set_site_ext(&site, name, true)
}

/// Turn one app's own `name` off; its `.so` stays.
pub fn remove_site(site: &str, name: &str) -> Result<Config> {
    find(name)?;
    set_site_ext(&crate::site_name(site), name, false)
}

/// `name` on or off in the app's own list: the extensions it has now (a worktree's are
/// its app's until it has its own), changed, as its own. A worktree's own list is kept
/// even empty, so one its app has stays off for it; an app's empty list isn't stored.
fn set_site_ext(site: &str, name: &str, on: bool) -> Result<Config> {
    let (cfg, ()) = crate::update(|c| {
        let found = crate::sites(c).into_iter().find(|s| s.name == site);
        let mut names = found.as_ref().map(|s| s.ext.clone()).unwrap_or_default();
        let worktree = found.is_some_and(|s| s.worktree_of.is_some());
        if names.iter().any(|n| n == name) == on {
            return Ok(());
        }
        names.retain(|n| n != name);
        if on {
            names.push(name.into());
        }
        c.sites.entry(site.into()).or_default().ext =
            (worktree || !names.is_empty()).then_some(names);
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
    php_args_with(cfg, minor, None)
}

/// `php_args` for one app's PHP (its PHP-FPM master of its own, its CLI): its own
/// extensions (`Site::ext`) too, each loaded once, and its own `opcache.*` and
/// `xdebug.*` values over the global ones, which PHP reads only when it starts.
pub fn php_args_with(cfg: &Config, minor: &str, site: Option<&crate::Site>) -> Vec<String> {
    let mut ini = cfg.ini.clone();
    ini.extend(site.map(|s| s.ini.clone()).unwrap_or_default());
    let mut args = crate::opcache::fpm_args(&ini, minor);
    let names = names_for(cfg, minor, site);
    if !names.is_empty() {
        args.extend(ini_args(&ini, minor, &names));
    }
    args
}

/// The extensions turned on for an app on `minor`: its version's, then its own.
fn names_for(cfg: &Config, minor: &str, site: Option<&crate::Site>) -> Vec<String> {
    let mut names = cfg.php_ext.get(minor).cloned().unwrap_or_default();
    for n in site.map_or(&[][..], |s| &s.ext) {
        if !names.contains(n) {
            names.push(n.clone());
        }
    }
    names
}

/// Build what is turned on for PHP `minor` (for the version, or by an app on it) but
/// not built for its installed patch release: after `anvil php install` brings a new
/// one, or an app moves to `minor`. One that fails is said; it shows as not built.
pub fn build_missing(cfg: &Config, minor: &str) {
    for name in missing(cfg, minor) {
        if let Err(e) = build(minor, name) {
            println!("PHP {minor} works without {name} for now: {e:#}");
        }
    }
}

/// What `build_missing` builds, in catalog order.
fn missing(cfg: &Config, minor: &str) -> Vec<&'static str> {
    let mut names = cfg.php_ext.get(minor).cloned().unwrap_or_default();
    for s in crate::sites(cfg) {
        if s.php.as_deref() == Some(minor) {
            names.extend(s.ext);
        }
    }
    let built = built_names(minor);
    offered()
        .map(|e| e.name)
        .filter(|n| names.iter().any(|x| x == n) && !built.contains(n))
        .collect()
}

/// `build_missing` for the PHP `site` is on now: after it moved to another version.
pub fn build_missing_for(cfg: &Config, site: &str) {
    let name = crate::site_name(site);
    if let Some(v) = crate::sites(cfg)
        .into_iter()
        .find(|s| s.name == name)
        .and_then(|s| s.php)
    {
        build_missing(cfg, &v);
    }
}

/// `php_args_with` and then the php.ini values PHP-FPM's pool gives `site` (or, without
/// one, the shared pool): the global ones, the app's own over them. What a PHP Anvil
/// starts outside PHP-FPM reads. The CLI keeps its own `max_execution_time=0` and
/// `max_input_time=-1`: PHP sets them over any ini file.
pub fn cli_args(cfg: &Config, minor: &str, site: Option<&crate::Site>) -> Vec<String> {
    let mut args = php_args_with(cfg, minor, site);
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
/// from an app process) is left out: never two of Anvil's at once. With nothing to load,
/// `None`, unless the user's names Anvil's folders: then the user's own entries only, so
/// another project's extensions don't come along.
pub(crate) fn scan_env_in(dir: &Path, args: &[String]) -> Result<Option<(String, String)>> {
    let home = home();
    let sep = crate::platform::PATH_SEP;
    let theirs = std::env::var("PHP_INI_SCAN_DIR").unwrap_or_default();
    let own: Vec<&str> = theirs
        .split(sep)
        .filter(|p| !Path::new(p).starts_with(&home))
        .collect();
    if args.is_empty() {
        let had_ours = own.len() != theirs.split(sep).count();
        return Ok(had_ours.then(|| ("PHP_INI_SCAN_DIR".into(), own.join(&sep.to_string()))));
    }
    let text: String = args.chunks(2).map(|c| format!("{}\n", c[1])).collect();
    let file = dir.join("anvil.ini");
    if fs::read_to_string(&file).ok().as_deref() != Some(text.as_str()) {
        fs::create_dir_all(dir)?;
        let tmp = dir.join(format!(".anvil.ini.{}", std::process::id()));
        fs::write(&tmp, &text)?;
        fs::rename(&tmp, &file)?;
    }
    let mut dirs = vec![dir.display().to_string()];
    if theirs.is_empty() {
        dirs.push(String::new());
    } else {
        dirs.extend(own.into_iter().map(String::from));
    }
    Ok(Some((
        "PHP_INI_SCAN_DIR".into(),
        dirs.join(&sep.to_string()),
    )))
}

/// phpize, configure and make in `dir` against the staged PHP; the built `.so`.
fn compile(dir: &Path, stage: &Path, bin: &Path, name: &str, recipe: &Recipe) -> Result<PathBuf> {
    let jobs = std::thread::available_parallelism().map_or(4, |n| n.get());
    let mut configure = vec![
        "--quiet".into(),
        format!(
            "--with-php-config={}",
            stage.join("bin/php-config").display()
        ),
    ];
    configure.extend(recipe.flags.iter().cloned());
    let steps: [(&Path, Vec<String>); 3] = [
        (&stage.join("bin/phpize"), vec![]),
        (Path::new("./configure"), configure),
        (Path::new("make"), vec!["-s".into(), format!("-j{jobs}")]),
    ];
    for (prog, args) in steps {
        run(crate::platform::command(prog)
            .args(args)
            .env("PHP_AUTOCONF", bin.join("autoconf"))
            .env("PHP_AUTOHEADER", bin.join("autoheader"))
            .envs(recipe.env.iter().cloned())
            .current_dir(dir))
        .map_err(|e| tools_error(name, e))?;
    }
    Ok(dir.join("modules").join(format!("{name}.so")))
}

/// PHP lists the module with the `.so` loaded: a build it rejects would only print a
/// warning on every start.
fn check_loads(minor: &str, ext: &Ext, so: &Path) -> Result<()> {
    let out = crate::platform::command(crate::php_bin(minor))
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

/// Swoole against the PHP binary's own OpenSSL: the headers of that exact release, no
/// libraries (`SSL_LIBS=" "`: the symbols come from the binary when the `.so` loads), no
/// Homebrew on PATH (its pkg-config would bring brotli or another OpenSSL). Sockets
/// support stays off: it needs PHP's sockets headers, which the staged tree lacks, and
/// Octane doesn't use it. 6.2.3 lacks `sw_usleep` on macOS (fixed upstream after it).
fn swoole_recipe(minor: &str) -> Result<Recipe> {
    let out = crate::platform::command(crate::php_bin(minor))
        .args(["-n", "-r", "echo OPENSSL_VERSION_TEXT;"])
        .output()
        .with_context(|| format!("PHP {minor} is not installed"))?;
    let inc = openssl_headers(&String::from_utf8_lossy(&out.stdout))?;
    let inc = format!("-I{}", inc.display());
    let path = format!(
        "{}:/usr/bin:/bin:/usr/sbin:/sbin",
        tools_dir().join("bin").display()
    );
    let env = [
        ("PATH", path.as_str()),
        ("SSL_CFLAGS", &inc),
        ("SSL_LIBS", " "),
        ("CRYPTO_CFLAGS", &inc),
        ("CRYPTO_LIBS", " "),
        ("CXXFLAGS", "-g -O2 -Dsw_usleep=usleep"),
    ];
    Ok(Recipe {
        env: env.map(|(k, v)| (k.to_string(), v.to_string())).into(),
        flags: vec!["--disable-brotli".into()],
    })
}

/// The pinned OpenSSL release `text` (`OPENSSL_VERSION_TEXT`) names and its SHA-256.
fn openssl_pin(text: &str) -> Result<(&'static str, &'static str)> {
    let version = text.split_whitespace().nth(1).unwrap_or_default();
    OPENSSL
        .iter()
        .find(|(v, _)| *v == version)
        .copied()
        .with_context(|| {
            format!(
                "Swoole needs the headers of this PHP's OpenSSL ({}), which Anvil has no checksum for",
                text.trim()
            )
        })
}

/// The `include` folder with OpenSSL's headers for the release `text` names, generated
/// once from its verified source into `cache/openssl/<version>`; the 55 MB source goes.
fn openssl_headers(text: &str) -> Result<PathBuf> {
    let (version, sum) = openssl_pin(text)?;
    let dir = home().join("cache").join("openssl").join(version);
    if dir.join("include/openssl/opensslv.h").is_file() {
        return Ok(dir.join("include"));
    }
    println!("Getting OpenSSL {version}'s headers, which Swoole needs (once)…");
    let src = home().join("build").join(format!("openssl-{version}"));
    let _ = fs::remove_dir_all(&src);
    let file = format!("openssl-{version}");
    let target = match std::env::consts::ARCH {
        "aarch64" => "darwin64-arm64-cc",
        _ => "darwin64-x86_64-cc",
    };
    let ok = fetch_tgz(
        &format!("https://github.com/openssl/openssl/releases/download/{file}/{file}.tar.gz"),
        &src,
        true,
        Some(sum),
    )
    .with_context(|| format!("couldn't get {file} from GitHub"))
    .and_then(|()| {
        for args in [
            vec!["./Configure", target, "no-shared", "no-tests"],
            vec!["make", "-s", "build_generated"],
        ] {
            run(crate::platform::command(args[0])
                .args(&args[1..])
                .current_dir(&src))
            .map_err(|e| tools_error("OpenSSL's headers", e))?;
        }
        crate::replace_dir(&dir, |d| {
            fs::create_dir_all(d.join("include"))?;
            Ok(fs::rename(
                src.join("include/openssl"),
                d.join("include/openssl"),
            )?)
        })
    });
    let _ = fs::remove_dir_all(&src);
    ok?;
    Ok(dir.join("include"))
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
            let mut cmd = crate::platform::command(prog);
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
    for ext in offered().filter(|e| names.iter().any(|n| n == e.name)) {
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
        assert_eq!(
            names,
            ["xdebug", "pcov", "apcu", "igbinary", "msgpack", "swoole"]
        );
        for e in CATALOG {
            assert!(hex(e.sha256), "{}", e.name);
            assert!(!e.version.is_empty() && !e.pecl.is_empty(), "{}", e.name);
            assert_eq!(e.zend, e.name == "xdebug", "{}", e.name);
        }
        assert!(OPENSSL.iter().all(|(_, s)| hex(s)));
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
    fn missing_counts_apps_on_the_version() {
        let dir = tempfile::tempdir().unwrap();
        let _env = crate::lock(&ENV_LOCK);
        // SAFETY: every test that sets ANVIL_HOME holds ENV_LOCK.
        unsafe { std::env::set_var("ANVIL_HOME", dir.path()) };
        for v in ["8.3", "8.4"] {
            fake_php(v, &format!("{v}.9"));
            fs::write(php_dir(v).join(crate::PHP_SERVER), "").unwrap();
        }
        let app = |n: &str| {
            let d = dir.path().join(n);
            fs::create_dir_all(&d).unwrap();
            fs::write(d.join("index.php"), "").unwrap();
            d
        };
        let mut cfg = Config {
            php: Some("8.4".into()),
            ..Default::default()
        };
        cfg.links.insert("shop".into(), app("shop"));
        cfg.links.insert("blog".into(), app("blog"));
        cfg.php_ext.insert("8.4".into(), vec!["apcu".into()]);
        cfg.sites.entry("shop".into()).or_default().ext = Some(vec!["xdebug".into()]);
        let blog = cfg.sites.entry("blog".into()).or_default();
        blog.ext = Some(vec!["pcov".into()]);
        blog.php = Some("8.3".into());
        // A new 8.4.9: the version's apcu and shop's own xdebug; blog's pcov is 8.3's.
        assert_eq!(missing(&cfg, "8.4"), ["xdebug", "apcu"]);
        fake_so("8.4", "8.4.9", "xdebug");
        assert_eq!(missing(&cfg, "8.4"), ["apcu"]);
        assert_eq!(missing(&cfg, "8.3"), ["pcov"]);
        assert_eq!(built_names("8.4"), ["xdebug"]);
    }

    #[test]
    fn an_apps_own_ini_reaches_its_master() {
        let dir = tempfile::tempdir().unwrap();
        let _env = crate::lock(&ENV_LOCK);
        // SAFETY: every test that sets ANVIL_HOME holds ENV_LOCK.
        unsafe { std::env::set_var("ANVIL_HOME", dir.path()) };
        fake_php("8.5", "8.5.1");
        fake_so("8.5", "8.5.1", "xdebug");
        let mut cfg = Config::default();
        cfg.ini.insert("xdebug.client_port".into(), "9010".into());
        cfg.ini
            .insert("opcache.memory_consumption".into(), "512".into());
        let mut s = crate::tests::site("shop", "/s/shop", Some("8.5"));
        s.ext = vec!["xdebug".into()];
        s.ini.insert("xdebug.mode".into(), "coverage".into());
        s.ini
            .insert("opcache.memory_consumption".into(), "1024".into());
        let own = php_args_with(&cfg, "8.5", Some(&s));
        for (want, not) in [
            ("xdebug.mode=coverage", "xdebug.mode=debug"),
            (
                "opcache.memory_consumption=1024",
                "opcache.memory_consumption=512",
            ),
        ] {
            assert!(
                own.contains(&want.into()) && !own.contains(&not.into()),
                "{own:?}"
            );
        }
        assert!(
            own.contains(&"xdebug.client_port=9010".into()),
            "the global one too"
        );
        // The version's master: the global values only, and no xdebug at all.
        let shared = php_args_with(&cfg, "8.5", None);
        assert!(shared.contains(&"opcache.memory_consumption=512".into()));
        assert!(!shared.iter().any(|a| a.contains("xdebug")), "{shared:?}");
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
        // Nothing to load here, from an app process: its folder doesn't come along.
        let none = |theirs: &str| {
            // SAFETY: as above.
            unsafe { std::env::set_var("PHP_INI_SCAN_DIR", theirs) };
            scan_env_in(&conf, &[]).unwrap().map(|(_, v)| v)
        };
        let theirs = format!("{}:/etc/php.d", other.display());
        assert_eq!(none(&theirs).as_deref(), Some("/etc/php.d"));
        assert_eq!(none(&format!("{}:", other.display())).as_deref(), Some(""));
        assert_eq!(none("/etc/php.d"), None, "the user's own stays as it is");
        unsafe { std::env::remove_var("PHP_INI_SCAN_DIR") };
        assert_eq!(scan_env_in(&conf, &[]).unwrap(), None);
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
        let e = build("8.4", "redis").unwrap_err();
        assert_eq!(e.to_string(), "redis is not in Anvil's catalog");
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

    #[test]
    fn swoole_is_hidden() {
        let dir = tempfile::tempdir().unwrap();
        let _env = crate::lock(&ENV_LOCK);
        // SAFETY: every test that sets ANVIL_HOME holds ENV_LOCK.
        unsafe { std::env::set_var("ANVIL_HOME", dir.path()) };
        let names: Vec<_> = offered().map(|e| e.name).collect();
        assert_eq!(names, ["xdebug", "pcov", "apcu", "igbinary", "msgpack"]);
        fake_php("8.4", "8.4.23");
        fs::write(php_dir("8.4").join("php-fpm"), "").unwrap();
        fake_so("8.4", "8.4.23", "swoole");
        crate::save(&Config::default()).unwrap();
        let e = add("8.4", "swoole").err().unwrap();
        assert_eq!(e.to_string(), "swoole is not in Anvil's catalog");
        assert!(add_site("shop", "swoole").is_err());
        // Built, and in the config by hand: still never loaded for the version.
        let mut cfg = Config::default();
        cfg.php_ext.insert("8.4".into(), vec!["swoole".into()]);
        assert!(php_args(&cfg, "8.4").is_empty());
        assert!(ini_args(&cfg.ini, "8.4", &["swoole".into()]).is_empty());
    }

    #[test]
    fn unpinned_openssl_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let _env = crate::lock(&ENV_LOCK);
        // SAFETY: every test that sets ANVIL_HOME holds ENV_LOCK.
        unsafe { std::env::set_var("ANVIL_HOME", dir.path()) };
        assert_eq!(openssl_pin("OpenSSL 3.6.2 7 Apr 2026").unwrap().0, "3.6.2");
        assert_eq!(
            openssl_pin("OpenSSL 3.5.0 8 Apr 2025")
                .unwrap_err()
                .to_string(),
            "Swoole needs the headers of this PHP's OpenSSL (OpenSSL 3.5.0 8 Apr 2025), which Anvil has no checksum for"
        );
        // A PHP on an OpenSSL Anvil has no pin for: refused before any download.
        fake_php("8.4", "'OpenSSL 3.5.0 8 Apr 2025'");
        let e = build("8.4", "swoole").unwrap_err();
        assert!(e.to_string().contains("no checksum"), "{e}");
        assert!(!dir.path().join("cache").exists(), "nothing downloaded");
        assert!(!dir.path().join("build").exists());
    }
}
