//! Laravel Octane: which server an app asks for, and that server's binary, fetched once
//! into `~/.anvil/octane/<server>/<version>/` and checked against its SHA-256.

use crate::{Site, home};
use anyhow::{Result, bail};
use std::path::{Path, PathBuf};

/// The process id (and name) of an app's Octane row.
pub const ID: &str = "octane";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Server {
    RoadRunner,
    FrankenPhp,
    Swoole,
}

impl Server {
    /// As `octane:start --server=` takes it.
    pub fn name(self) -> &'static str {
        match self {
            Server::RoadRunner => "roadrunner",
            Server::FrankenPhp => "frankenphp",
            Server::Swoole => "swoole",
        }
    }
}

/// RoadRunner 2025.1.x: Octane 2.22 flags v3 as possibly incompatible.
const RR_VERSION: &str = "2025.1.15";
/// The release publishes no checksums file: these are the SHA-256 digests GitHub lists for
/// its macOS assets (`gh api repos/roadrunner-server/roadrunner/releases/tags/v2025.1.15`).
const RR_ARM64: &str = "cc0521bd31478cc39403c2f090fc73db45e87e33a66b6783e9bcf096f321729a";
const RR_AMD64: &str = "2059b1564de0352c8d010ffc9f630f9da2174e9430ee1a6aa83d6c2b38bdc6c6";

/// FrankenPHP, a single binary with its own PHP inside. No checksums file either: the
/// digests GitHub lists (`gh api repos/php/frankenphp/releases/tags/v1.13.1`).
const FP_VERSION: &str = "1.13.1";
const FP_ARM64: &str = "0cb3b88e0f18b51fb9d1d71977be2dd871199d5cdf986a6d20e32a841686a426";
const FP_AMD64: &str = "b9123b427cb357eac076b1f316f71589cfe5c3db4ad2449114e243563d794a89";
/// The PHP inside that release: `frankenphp version` says "FrankenPHP v1.13.1 PHP 8.5.11
/// Caddy v2.11.7". Checked against the binary when it is installed (`install_frankenphp`),
/// so a version bump can't leave this behind; known before the 180 MB download, so an app
/// on another PHP is told so without it.
const FP_PHP: &str = "8.5";

/// The server of an app with `laravel/octane`: `OCTANE_SERVER` in its `.env`, else
/// `config/octane.php`'s default, else RoadRunner. `None` without laravel/octane.
pub fn server(dir: &Path) -> Option<Server> {
    let composer: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("composer.json")).ok()?).ok()?;
    ["require", "require-dev"]
        .iter()
        .any(|k| composer[k].get("laravel/octane").is_some())
        .then_some(())?;
    let name = crate::resources::dotenv(dir)
        .remove("OCTANE_SERVER")
        .or_else(|| config_default(&std::fs::read_to_string(dir.join("config/octane.php")).ok()?));
    Some(match name.as_deref() {
        Some("frankenphp") => Server::FrankenPhp,
        Some("swoole" | "openswoole") => Server::Swoole,
        _ => Server::RoadRunner,
    })
}

/// `x` of `env('OCTANE_SERVER', 'x')` in config/octane.php.
fn config_default(php: &str) -> Option<String> {
    let rest = &php[php.find("env('OCTANE_SERVER'")? + "env('OCTANE_SERVER'".len()..];
    let rest = rest.trim_start().strip_prefix(',')?.trim_start();
    let quote = rest.chars().next().filter(|c| matches!(c, '\'' | '"'))?;
    let rest = &rest[1..];
    Some(rest[..rest.find(quote)?].to_string())
}

/// Where the server's binary goes: `~/.anvil/octane/<server>/<version>/`.
pub fn dir(server: Server) -> PathBuf {
    let version = match server {
        Server::FrankenPhp => FP_VERSION,
        _ => RR_VERSION,
    };
    home().join("octane").join(server.name()).join(version)
}

/// The server's binary, downloaded, verified and unpacked the first time.
pub fn ensure(server: Server) -> Result<PathBuf> {
    match server {
        Server::RoadRunner => {}
        Server::FrankenPhp => {
            let (arch, sum) = match crate::platform()?.1 {
                "mac_arm64" => ("arm64", FP_ARM64),
                "mac_amd64" => ("x86_64", FP_AMD64),
                p => bail!("Anvil has no FrankenPHP for {p}"),
            };
            let url = format!(
                "https://github.com/php/frankenphp/releases/download/v{FP_VERSION}/frankenphp-mac-{arch}"
            );
            return install_frankenphp(&url, sum, &dir(server));
        }
        Server::Swoole => bail!("Swoole is built for the app's PHP, not downloaded"),
    }
    let (arch, sum, zip) = match crate::platform()?.1 {
        "mac_arm64" => ("darwin-arm64", RR_ARM64, false),
        "mac_amd64" => ("darwin-amd64", RR_AMD64, true),
        p => bail!("Anvil has no RoadRunner for {p}"),
    };
    let ext = if zip { "zip" } else { "tar.gz" };
    let url = format!(
        "https://github.com/roadrunner-server/roadrunner/releases/download/v{RR_VERSION}/roadrunner-{RR_VERSION}-{arch}.{ext}"
    );
    install(&url, sum, &dir(server), zip)
}

/// `rr` out of the release archive at `url` into `dir`, once it matches `sum`, with an
/// empty `rr.yaml` beside it (see `command`).
fn install(url: &str, sum: &str, dir: &Path, zip: bool) -> Result<PathBuf> {
    let rr = dir.join("rr");
    if rr.is_file() {
        return Ok(rr);
    }
    crate::replace_dir(dir, |d| {
        let file = crate::download(url, Some(sum))?;
        // Just the binary; the archive keeps it in a folder of its own.
        let mut unpack = if zip {
            let mut c = crate::platform::command("unzip");
            c.args(["-q", "-j"]).arg(&file).arg("*/rr").arg("-d").arg(d);
            c
        } else {
            let mut c = crate::platform::command("tar");
            c.arg("-xzf")
                .arg(&file)
                .arg("-C")
                .arg(d)
                .args(["--strip-components=1", "*/rr"]);
            c
        };
        let ok = crate::run(&mut unpack);
        let _ = std::fs::remove_file(&file);
        ok?;
        anyhow::ensure!(d.join("rr").is_file(), "{url} has no rr in it");
        Ok(std::fs::write(d.join("rr.yaml"), "")?)
    })?;
    Ok(rr)
}

/// The FrankenPHP binary at `url` as `dir/frankenphp`, once it matches `sum` and runs the
/// PHP `FP_PHP` says.
fn install_frankenphp(url: &str, sum: &str, dir: &Path) -> Result<PathBuf> {
    let bin = dir.join("frankenphp");
    if bin.is_file() {
        return Ok(bin);
    }
    crate::replace_dir(dir, |d| {
        let file = crate::download(url, Some(sum))?;
        let to = d.join("frankenphp");
        let moved = std::fs::rename(&file, &to);
        let _ = std::fs::remove_file(&file);
        moved?;
        crate::platform::set_mode(&to, 0o755)?;
        let out = crate::platform::command(&to).arg("version").output()?;
        let version = String::from_utf8_lossy(&out.stdout);
        anyhow::ensure!(
            version.contains(&format!(" PHP {FP_PHP}.")),
            "FrankenPHP {FP_VERSION} doesn't run PHP {FP_PHP}: {}",
            version.trim()
        );
        Ok(())
    })?;
    Ok(bin)
}

/// The Caddyfile Anvil gives FrankenPHP: the app's own Octane's, listening on 127.0.0.1
/// only. Octane's binds every interface (`http://:<port>`), whatever `--host` says.
fn caddyfile(site: &Site) -> PathBuf {
    home()
        .join("run")
        .join(format!("octane-{}.Caddyfile", crate::pool(site)))
}

/// Write `caddyfile(site)` from the app's vendor/laravel/octane.
fn write_caddyfile(site: &Site) -> Result<()> {
    let stub = site
        .path
        .join("vendor/laravel/octane/src/Commands/stubs/Caddyfile");
    let Ok(text) = std::fs::read_to_string(&stub) else {
        bail!("laravel/octane isn't in vendor yet: run composer install");
    };
    let Some(rest) = text.trim_start().strip_prefix('{') else {
        bail!("{} doesn't start with its global options", stub.display());
    };
    let file = caddyfile(site);
    std::fs::create_dir_all(file.parent().unwrap_or(Path::new("/")))?;
    Ok(std::fs::write(
        file,
        format!("{{\n\tdefault_bind 127.0.0.1\n{rest}"),
    )?)
}

/// The extensions FrankenPHP's PHP has built in, lowercased: read from the binary once
/// (`php-cli -m` refuses below PHP 8.6, `-r` works) and kept beside it as `extensions`.
fn builtin() -> Result<Vec<String>> {
    let d = dir(Server::FrankenPhp);
    let file = d.join("extensions");
    let text = match std::fs::read_to_string(&file) {
        Ok(t) => t,
        Err(_) => {
            let data = home().join("octane/frankenphp/data");
            let out = crate::platform::command(d.join("frankenphp"))
                .args(["php-cli", "-n", "-r"])
                .arg(r#"echo implode(",", array_merge(get_loaded_extensions(), get_loaded_extensions(true)));"#)
                .env("XDG_CONFIG_HOME", &data)
                .env("XDG_DATA_HOME", &data)
                .output()?;
            anyhow::ensure!(
                out.status.success(),
                "FrankenPHP didn't list its extensions: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
            let t = String::from_utf8_lossy(&out.stdout)
                .to_lowercase()
                .replace(',', "\n");
            std::fs::write(&file, &t)?;
            t
        }
    };
    Ok(text.lines().map(|l| l.trim().to_string()).collect())
}

/// Err unless FrankenPHP has every extension Anvil loads for the app (its PHP version's
/// and its own, `phpext::php_args_with`): Anvil's builds can't load into it, so starting
/// without them would drop them silently.
fn lacking(cfg: &crate::Config, site: &Site) -> Result<()> {
    let version = cfg.php_ext.get(FP_PHP).cloned().unwrap_or_default();
    let mut names: Vec<&String> = vec![];
    for n in version.iter().chain(&site.ext) {
        if !names.contains(&n) && crate::phpext::built(FP_PHP, n) {
            names.push(n);
        }
    }
    if names.is_empty() {
        return Ok(());
    }
    let have = builtin()?;
    let missing: Vec<&String> = names.into_iter().filter(|n| !have.contains(n)).collect();
    let Some((last, rest)) = missing.split_last() else {
        return Ok(());
    };
    let list = match rest {
        [] => last.to_string(),
        _ => format!(
            "{} or {last}",
            rest.iter()
                .map(|n| n.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    };
    let (app, ver) = (
        missing.iter().any(|n| site.ext.contains(n)),
        missing.iter().any(|n| version.contains(n)),
    );
    let place = match (app, ver) {
        (true, false) => "for this app".to_string(),
        (false, _) => format!("for PHP {FP_PHP}"),
        (true, true) => format!("for this app and PHP {FP_PHP}"),
    };
    let it = if rest.is_empty() { "it" } else { "them" };
    bail!("FrankenPHP has no {list}; turn {it} off {place} or use RoadRunner")
}

/// FrankenPHP's environment over an app process's:Anvil's extensions are built for
/// Anvil's PHP (not thread-safe) and can't load into FrankenPHP's own (thread-safe, with
/// its own extensions), so its `PHP_INI_SCAN_DIR` has the app's php.ini values (PHP-FPM's,
/// and OPcache's and Xdebug's settings) but no extension lines, and an app that has one
/// FrankenPHP lacks doesn't start (`lacking`). Its Caddy keeps its state
/// under Anvil's folder, not the user's own Caddy's.
pub(crate) fn frankenphp_env(
    cfg: &crate::Config,
    site: &Site,
    env: &mut Vec<(String, String)>,
) -> Result<()> {
    lacking(cfg, site)?;
    env.retain(|(k, _)| k != "PHP_INI_SCAN_DIR");
    let args: Vec<String> = crate::phpext::cli_args(cfg, FP_PHP, Some(site))
        .chunks(2)
        .filter(|d| !d[1].starts_with("extension=") && !d[1].starts_with("zend_extension="))
        .flatten()
        .cloned()
        .collect();
    let dir = home()
        .join("run")
        .join(format!("frankenphp-{}.d", crate::pool(site)));
    env.extend(crate::phpext::scan_env_in(&dir, &args)?);
    let data = home().join("octane/frankenphp/data");
    env.push(("XDG_CONFIG_HOME".into(), data.display().to_string()));
    env.push(("XDG_DATA_HOME".into(), data.display().to_string()));
    Ok(())
}

/// The command of the app's Octane row on `port`; `admin` is RoadRunner's RPC port,
/// FrankenPHP's admin port (Octane's default, 2019, would clash between apps).
/// Err with the reason when Anvil can't run that server for the app.
///
/// RoadRunner: Octane writes an empty `.rr.yaml` into the project unless it's given one,
/// so it gets the project's own when there is one, else Anvil's empty one; `rr` itself is
/// found on PATH (`apps::process_env`), never in the project. Two workers, not one per
/// core: measured on a 12-core Mac, 140 MB against 455 MB idle, and no slower in dev.
pub fn command(site: &Site, server: Server, port: u16, admin: Option<u16>) -> Result<String> {
    let mut c = format!(
        "php artisan octane:start --server={} --host=127.0.0.1 --port={port} --workers=2",
        server.name()
    );
    match server {
        Server::RoadRunner => {
            let own = site.path.join(".rr.yaml");
            let config = if own.is_file() {
                own
            } else {
                dir(server).join("rr.yaml")
            };
            if let Some(rpc) = admin {
                c += &format!(" --rpc-port={rpc}");
            }
            c += &format!(
                " --rr-config={}",
                crate::shims::sh_quote(&config.display().to_string())
            );
            Ok(c)
        }
        // `frankenphp` is found on PATH too. Octane copies public/frankenphp-worker.php
        // into the project if it isn't there (as `octane:install` does): no flag avoids it.
        Server::FrankenPhp => {
            let php = site.php.as_deref().unwrap_or("no PHP version");
            if php != FP_PHP {
                bail!("FrankenPHP runs PHP {FP_PHP}; this app is on {php}");
            }
            if let Some(admin) = admin {
                c += &format!(" --admin-port={admin}");
            }
            c += &format!(
                " --caddyfile={}",
                crate::shims::sh_quote(&caddyfile(site).display().to_string())
            );
            Ok(c)
        }
        // Two workers, one task worker: Octane reads `--task-workers=0` as one per core
        // (17 processes, 533 MB on a 12-core Mac, against 6 and 200 MB).
        Server::Swoole => {
            match site.php.as_deref() {
                None => {
                    bail!("Swoole is built for Anvil's PHP; this app runs on none of its versions")
                }
                // Swoole 6 doesn't build for 8.1 ("require PHP version 8.2 or later").
                Some(v @ "8.1") => bail!("Swoole needs PHP 8.2 or later; this app is on {v}"),
                Some(_) => {}
            }
            Ok(c + " --task-workers=1")
        }
    }
}

/// Swoole's environment over an app process's: the app's usual `PHP_INI_SCAN_DIR` lines
/// and Swoole's, in a folder of the Octane row's own. `-d` wouldn't do: Octane starts its
/// server as a PHP of its own, which only the scan dir reaches. Never PHP-FPM's or any
/// other process's.
pub(crate) fn swoole_env(
    cfg: &crate::Config,
    site: &Site,
    env: &mut Vec<(String, String)>,
) -> Result<()> {
    let Some(minor) = site.php.as_deref() else {
        return Ok(());
    };
    let full = crate::opcache::installed_full(minor)?;
    let mut args = crate::phpext::cli_args(cfg, minor, Some(site));
    args.extend([
        "-d".into(),
        format!(
            "extension={}",
            crate::phpext::so_path(minor, &full, "swoole").display()
        ),
    ]);
    env.retain(|(k, _)| k != "PHP_INI_SCAN_DIR");
    let dir = home()
        .join("run")
        .join(format!("swoole-{}.d", crate::pool(site)));
    env.extend(crate::phpext::scan_env_in(&dir, &args)?);
    Ok(())
}

/// Before the Octane row `p` of `site` starts: the reason it can't, or its server's
/// binary in place (the download happens here, outside the app's process lock).
pub fn prepare(site: &Site, p: &crate::processes::Process) -> Result<()> {
    if p.id != ID {
        return Ok(());
    }
    let Some(server) = server(&site.path) else {
        return Ok(());
    };
    command(site, server, p.port.unwrap_or_default(), None)?;
    match (server, site.php.as_deref()) {
        (Server::FrankenPhp, _) => write_caddyfile(site)?,
        // Built on this Mac for the app's PHP, the first time (about a minute).
        (Server::Swoole, Some(minor)) => {
            if !crate::phpext::built(minor, "swoole") {
                crate::phpext::build(minor, "swoole")?;
            }
            return Ok(());
        }
        _ => {}
    }
    ensure(server).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn detects_server_from_env_config_default() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        assert_eq!(server(d), None, "no composer.json");
        fs::write(
            d.join("composer.json"),
            r#"{"require": {"laravel/framework": "^12.0"}}"#,
        )
        .unwrap();
        assert_eq!(server(d), None, "no laravel/octane");
        // require-dev counts too; nothing else says which: RoadRunner.
        fs::write(
            d.join("composer.json"),
            r#"{"require-dev": {"laravel/octane": "^2.22"}}"#,
        )
        .unwrap();
        assert_eq!(server(d), Some(Server::RoadRunner));
        // config/octane.php's default.
        fs::create_dir(d.join("config")).unwrap();
        fs::write(
            d.join("config/octane.php"),
            "<?php\nreturn [\n    'server' => env('OCTANE_SERVER', 'frankenphp'),\n];\n",
        )
        .unwrap();
        assert_eq!(server(d), Some(Server::FrankenPhp));
        // The .env wins over the config.
        fs::write(d.join(".env"), "APP_NAME=shop\nOCTANE_SERVER=\"swoole\"\n").unwrap();
        assert_eq!(server(d), Some(Server::Swoole));
        fs::write(d.join(".env"), "OCTANE_SERVER=roadrunner\n").unwrap();
        assert_eq!(server(d), Some(Server::RoadRunner));
    }

    /// A Laravel app on `php` in `dir` asking for FrankenPHP.
    fn frankenphp_app(dir: &Path, php: &str) -> Site {
        fs::write(
            dir.join("composer.json"),
            r#"{"require": {"laravel/octane": "^2.22"}}"#,
        )
        .unwrap();
        fs::write(dir.join(".env"), "OCTANE_SERVER=frankenphp\n").unwrap();
        let mut s = crate::tests::site("shop", dir.to_str().unwrap(), Some(php));
        s.kind = "laravel".into();
        s
    }

    #[test]
    fn frankenphp_runs_only_its_own_php() {
        let _env = crate::lock(&crate::ENV_LOCK);
        let home = tempfile::tempdir().unwrap();
        // SAFETY: every test that sets ANVIL_HOME holds ENV_LOCK.
        unsafe { std::env::set_var("ANVIL_HOME", home.path()) };
        let app = tempfile::tempdir().unwrap();
        let mut s = frankenphp_app(app.path(), "8.4");
        let err = command(&s, Server::FrankenPhp, 49001, Some(49002)).unwrap_err();
        assert_eq!(
            err.to_string(),
            "FrankenPHP runs PHP 8.5; this app is on 8.4"
        );
        // The row says so when started, before any download.
        let cfg = crate::Config::default();
        let row = crate::processes::octane_row(&cfg, &s).unwrap();
        assert_eq!(
            prepare(&s, &row).unwrap_err().to_string(),
            "FrankenPHP runs PHP 8.5; this app is on 8.4"
        );
        assert!(!dir(Server::FrankenPhp).exists(), "nothing downloaded");
        s.php = None;
        assert!(command(&s, Server::FrankenPhp, 49001, None).is_err());
        s.php = Some("8.5".into());
        assert!(command(&s, Server::FrankenPhp, 49001, None).is_ok());
        unsafe { std::env::remove_var("ANVIL_HOME") };
    }

    #[test]
    fn frankenphp_gets_admin_port() {
        let _env = crate::lock(&crate::ENV_LOCK);
        let home = tempfile::tempdir().unwrap();
        // SAFETY: every test that sets ANVIL_HOME holds ENV_LOCK.
        unsafe { std::env::set_var("ANVIL_HOME", home.path()) };
        let app = tempfile::tempdir().unwrap();
        let mut s = frankenphp_app(app.path(), "8.5");
        let file = caddyfile(&s);
        assert_eq!(
            command(&s, Server::FrankenPhp, 49001, Some(49002)).unwrap(),
            format!(
                "php artisan octane:start --server=frankenphp --host=127.0.0.1 --port=49001 --workers=2 --admin-port=49002 --caddyfile={}",
                crate::shims::sh_quote(&file.display().to_string())
            )
        );
        let mut cfg = crate::Config::default();
        let row = crate::processes::octane_row(&cfg, &s).unwrap();
        let (port, admin) = crate::processes::octane_ports(&cfg, &s);
        assert!(row.command.contains(&format!("--port={port} ")));
        assert!(row.command.contains(&format!("--admin-port={admin} ")));

        // Octane's own Caddyfile, bound to 127.0.0.1 rather than every interface.
        assert_eq!(
            write_caddyfile(&s).unwrap_err().to_string(),
            "laravel/octane isn't in vendor yet: run composer install"
        );
        let stubs = app.path().join("vendor/laravel/octane/src/Commands/stubs");
        fs::create_dir_all(&stubs).unwrap();
        fs::write(
            stubs.join("Caddyfile"),
            "{\n\t{$CADDY_GLOBAL_OPTIONS}\n}\n\n{$CADDY_SERVER_SERVER_NAME} {\n}\n",
        )
        .unwrap();
        write_caddyfile(&s).unwrap();
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "{\n\tdefault_bind 127.0.0.1\n\n\t{$CADDY_GLOBAL_OPTIONS}\n}\n\n{$CADDY_SERVER_SERVER_NAME} {\n}\n"
        );

        // php.ini values reach FrankenPHP's PHP; Anvil's extensions, built for its own, don't.
        crate::phpext::tests::fake_php("8.5", "8.5.1");
        crate::phpext::tests::fake_so("8.5", "8.5.1", "apcu");
        cfg.php_ext.insert("8.5".into(), vec!["apcu".into()]);
        fake_frankenphp("Core,apcu");
        cfg.ini.insert("memory_limit".into(), "1G".into());
        s.ini.insert("upload_max_filesize".into(), "64M".into());
        let env = crate::apps::process_env(&s, &row, &cfg).unwrap();
        let get = |k: &str| env.iter().find(|(x, _)| x == k).map(|(_, v)| v.clone());
        let scan = get("PHP_INI_SCAN_DIR").unwrap();
        let ini = fs::read_to_string(Path::new(scan.split(':').next().unwrap()).join("anvil.ini"))
            .unwrap();
        assert!(ini.contains("memory_limit=1G\n"), "{ini}");
        assert!(ini.contains("upload_max_filesize=64M\n"), "{ini}");
        assert!(!ini.contains("extension="), "{ini}");
        assert_eq!(
            env.iter().filter(|(k, _)| k == "PHP_INI_SCAN_DIR").count(),
            1
        );
        let data = home
            .path()
            .join("octane/frankenphp/data")
            .display()
            .to_string();
        assert_eq!(
            get("XDG_CONFIG_HOME"),
            Some(data.clone()),
            "not the user's Caddy's"
        );
        assert_eq!(get("XDG_DATA_HOME"), Some(data));
        let path = get("PATH").unwrap();
        assert!(path.starts_with(&format!("{}:", dir(Server::FrankenPhp).display())));
        unsafe { std::env::remove_var("ANVIL_HOME") };
    }

    #[test]
    fn swoole_only_on_octane_process() {
        let _env = crate::lock(&crate::ENV_LOCK);
        let home = tempfile::tempdir().unwrap();
        // SAFETY: every test that sets ANVIL_HOME holds ENV_LOCK.
        unsafe { std::env::set_var("ANVIL_HOME", home.path()) };
        let app = tempfile::tempdir().unwrap();
        let mut s = frankenphp_app(app.path(), "8.4");
        fs::write(app.path().join(".env"), "OCTANE_SERVER=swoole\n").unwrap();
        crate::phpext::tests::fake_php("8.4", "8.4.23");
        let swoole = crate::phpext::tests::fake_so("8.4", "8.4.23", "swoole");
        let apcu = crate::phpext::tests::fake_so("8.4", "8.4.23", "apcu");
        let mut cfg = crate::Config::default();
        cfg.php_ext.insert("8.4".into(), vec!["apcu".into()]);
        cfg.ini.insert("memory_limit".into(), "1G".into());
        s.ini.insert("upload_max_filesize".into(), "64M".into());
        let row = crate::processes::octane_row(&cfg, &s).unwrap();
        let (port, _) = crate::processes::octane_ports(&cfg, &s);
        // Two workers and one task worker: Octane takes 0 for "one per core".
        assert_eq!(
            row.command,
            format!(
                "php artisan octane:start --server=swoole --host=127.0.0.1 --port={port} --workers=2 --task-workers=1"
            )
        );
        let scan_ini = |env: &[(String, String)]| {
            let scans: Vec<_> = env
                .iter()
                .filter(|(k, _)| k == "PHP_INI_SCAN_DIR")
                .collect();
            assert_eq!(scans.len(), 1, "{env:?}");
            let dir = scans[0].1.split(':').next().unwrap().to_string();
            fs::read_to_string(Path::new(&dir).join("anvil.ini")).unwrap()
        };
        // The Octane process (and the server PHP it starts): the app's usual lines and Swoole.
        let ini = scan_ini(&crate::apps::process_env(&s, &row, &cfg).unwrap());
        assert!(ini.contains(&format!("extension={swoole}\n")), "{ini}");
        assert!(ini.contains(&format!("extension={apcu}\n")), "{ini}");
        assert!(ini.contains("memory_limit=1G\n"), "{ini}");
        assert!(ini.contains("upload_max_filesize=64M\n"), "{ini}");
        // Every other process of the app, its terminal and PHP-FPM: no Swoole.
        let queue = crate::processes::Process {
            id: "queue".into(),
            ..row.clone()
        };
        let ini = scan_ini(&crate::apps::process_env(&s, &queue, &cfg).unwrap());
        assert!(ini.contains(&format!("extension={apcu}\n")), "{ini}");
        assert!(!ini.contains("swoole"), "{ini}");
        let fpm = crate::phpext::php_args_with(&cfg, "8.4", Some(&s));
        assert!(!fpm.iter().any(|a| a.contains("swoole")), "{fpm:?}");
        let cli = crate::phpext::cli_args(&cfg, "8.4", Some(&s));
        assert!(!cli.iter().any(|a| a.contains("swoole")), "{cli:?}");
        // Built already: prepare has nothing to fetch.
        prepare(&s, &row).unwrap();
        s.php = None;
        assert_eq!(
            command(&s, Server::Swoole, port, None)
                .unwrap_err()
                .to_string(),
            "Swoole is built for Anvil's PHP; this app runs on none of its versions"
        );
        // Told on its row before anything is built.
        s.php = Some("8.1".into());
        let row = crate::processes::octane_row(&cfg, &s).unwrap();
        assert_eq!(
            prepare(&s, &row).unwrap_err().to_string(),
            "Swoole needs PHP 8.2 or later; this app is on 8.1"
        );
        unsafe { std::env::remove_var("ANVIL_HOME") };
    }

    /// A `frankenphp` in `dir(FrankenPhp)` whose PHP lists `exts`.
    fn fake_frankenphp(exts: &str) {
        use std::os::unix::fs::PermissionsExt;
        let d = dir(Server::FrankenPhp);
        fs::create_dir_all(&d).unwrap();
        let bin = d.join("frankenphp");
        fs::write(&bin, format!("#!/bin/sh\nprintf '{exts}'\n")).unwrap();
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn frankenphp_refuses_extensions_it_lacks() {
        let _env = crate::lock(&crate::ENV_LOCK);
        let home = tempfile::tempdir().unwrap();
        // SAFETY: every test that sets ANVIL_HOME holds ENV_LOCK.
        unsafe { std::env::set_var("ANVIL_HOME", home.path()) };
        let app = tempfile::tempdir().unwrap();
        let mut s = frankenphp_app(app.path(), "8.5");
        let mut cfg = crate::Config::default();
        let row = crate::processes::octane_row(&cfg, &s).unwrap();
        crate::phpext::tests::fake_php("8.5", "8.5.1");
        for e in ["xdebug", "pcov", "apcu"] {
            crate::phpext::tests::fake_so("8.5", "8.5.1", e);
        }
        fake_frankenphp("Core,APCu,Zend OPcache");
        // Read once from the binary, then from the list kept beside it.
        assert_eq!(builtin().unwrap(), ["core", "apcu", "zend opcache"]);
        fs::remove_file(dir(Server::FrankenPhp).join("frankenphp")).unwrap();
        assert_eq!(builtin().unwrap(), ["core", "apcu", "zend opcache"]);

        cfg.php_ext.insert("8.5".into(), vec!["apcu".into()]);
        assert!(
            crate::apps::process_env(&s, &row, &cfg).is_ok(),
            "it has apcu"
        );
        s.ext = vec!["xdebug".into()];
        let err = |cfg: &crate::Config, s: &Site| {
            crate::apps::process_env(s, &row, cfg)
                .unwrap_err()
                .to_string()
        };
        assert_eq!(
            err(&cfg, &s),
            "FrankenPHP has no xdebug; turn it off for this app or use RoadRunner"
        );
        cfg.php_ext.insert("8.5".into(), vec!["pcov".into()]);
        assert_eq!(
            err(&cfg, &s),
            "FrankenPHP has no pcov or xdebug; turn them off for this app and PHP 8.5 or use RoadRunner"
        );
        s.ext.clear();
        assert_eq!(
            err(&cfg, &s),
            "FrankenPHP has no pcov; turn it off for PHP 8.5 or use RoadRunner"
        );
        // On but not built for this PHP: Anvil wouldn't load it, nothing to refuse.
        cfg.php_ext.insert("8.5".into(), vec!["msgpack".into()]);
        assert!(crate::apps::process_env(&s, &row, &cfg).is_ok());
        unsafe { std::env::remove_var("ANVIL_HOME") };
    }

    #[test]
    fn binary_sum_mismatch_refuses() {
        let _env = crate::lock(&crate::ENV_LOCK);
        let home = tempfile::tempdir().unwrap();
        // SAFETY: every test that sets ANVIL_HOME holds ENV_LOCK.
        unsafe { std::env::set_var("ANVIL_HOME", home.path()) };
        // A release archive as RoadRunner ships it: rr inside a folder of its own.
        let src = home.path().join("src");
        fs::create_dir_all(src.join("roadrunner-x")).unwrap();
        fs::write(src.join("roadrunner-x/rr"), "#!/bin/sh\n").unwrap();
        let tgz = home.path().join("rr.tar.gz");
        crate::run(
            crate::platform::command("tar")
                .arg("-czf")
                .arg(&tgz)
                .arg("-C")
                .arg(&src)
                .arg("roadrunner-x"),
        )
        .unwrap();
        let url = format!("file://{}", tgz.display());
        let dir = home.path().join("octane/roadrunner/x");
        let bad = "0".repeat(64);
        let err = install(&url, &bad, &dir, false).unwrap_err().to_string();
        assert_eq!(
            err,
            "The download of rr.tar.gz didn't match its published checksum"
        );
        assert!(!dir.exists(), "nothing of it is kept");
        let out = crate::platform::command("openssl")
            .args(["dgst", "-sha256", "-r"])
            .arg(&tgz)
            .output()
            .unwrap();
        let sum = String::from_utf8_lossy(&out.stdout)
            .split_whitespace()
            .next()
            .unwrap()
            .to_string();
        let rr = install(&url, &sum, &dir, false).unwrap();
        assert_eq!(rr, dir.join("rr"));
        assert!(rr.is_file());
        // Octane's own .rr.yaml would land in the project: Anvil's empty one is used instead.
        assert!(dir.join("rr.yaml").is_file());
        // Once there, it isn't fetched again.
        fs::remove_file(&tgz).unwrap();
        assert_eq!(install(&url, &sum, &dir, false).unwrap(), rr);
        unsafe { std::env::remove_var("ANVIL_HOME") };
    }
}
