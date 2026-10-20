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
    home().join("octane").join(server.name()).join(RR_VERSION)
}

/// The server's binary, downloaded, verified and unpacked the first time.
pub fn ensure(server: Server) -> Result<PathBuf> {
    if server != Server::RoadRunner {
        bail!(
            "Anvil runs Octane with RoadRunner for now, not {}",
            server.name()
        );
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
            let mut c = std::process::Command::new("unzip");
            c.args(["-q", "-j"]).arg(&file).arg("*/rr").arg("-d").arg(d);
            c
        } else {
            let mut c = std::process::Command::new("tar");
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

/// The command of the app's Octane row on `port`; `admin` is RoadRunner's RPC port.
/// Err with the reason when Anvil can't run that server for the app.
///
/// RoadRunner: Octane writes an empty `.rr.yaml` into the project unless it's given one,
/// so it gets the project's own when there is one, else Anvil's empty one; `rr` itself is
/// found on PATH (`apps::process_env`), never in the project.
pub fn command(site: &Site, server: Server, port: u16, admin: Option<u16>) -> Result<String> {
    let mut c = format!(
        "php artisan octane:start --server={} --host=127.0.0.1 --port={port}",
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
        _ => bail!(
            "Anvil runs Octane with RoadRunner for now, not {}",
            server.name()
        ),
    }
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
            std::process::Command::new("tar")
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
        let out = std::process::Command::new("openssl")
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
