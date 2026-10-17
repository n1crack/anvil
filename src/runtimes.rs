//! Language toolchains for apps that run their own server: Node.js, Go, Rust and
//! .NET. Each lives inside `~/.anvil` and is only put on PATH for the apps Anvil
//! runs, so system installs are never touched.

use crate::{download, fetch_text, fetch_tgz, home, platform, published_sum, replace_dir, run};
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

pub const RUNTIMES: [&str; 4] = ["node", "go", "rust", "dotnet"];

#[derive(Serialize)]
pub struct RuntimeInfo {
    pub id: &'static str,
    pub name: &'static str,
    /// Versions installed in Anvil, newest first.
    pub installed: Vec<String>,
    /// Versions offered for install, newest first.
    pub available: Vec<String>,
}

pub fn name(id: &str) -> &'static str {
    match id {
        "node" => "Node.js",
        "go" => "Go",
        "rust" => "Rust",
        _ => ".NET",
    }
}

fn root(id: &str) -> PathBuf {
    home().join("runtimes").join(id)
}

fn check(id: &str) -> Result<()> {
    if !RUNTIMES.contains(&id) {
        bail!("Unknown runtime {id}");
    }
    Ok(())
}

/// Sort version strings numerically, newest first ("22" > "9", "1.27.2" > "1.9").
fn newest_first(v: &mut [String]) {
    let key = |s: &String| -> Vec<u64> {
        s.split(|c: char| !c.is_ascii_digit())
            .filter_map(|p| p.parse().ok())
            .collect()
    };
    v.sort_by_key(|s| std::cmp::Reverse(key(s)));
}

fn subdirs(dir: &PathBuf) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir() && !e.file_name().to_string_lossy().starts_with('.'))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    newest_first(&mut v);
    v
}

pub fn installed(id: &str) -> Vec<String> {
    match id {
        // rustup manages toolchains itself; one Anvil install serves every project.
        "rust" if cargo_bin().join("cargo").exists() => vec!["stable".into()],
        "rust" => vec![],
        // All .NET SDKs share one folder and `dotnet` picks per project (global.json).
        "dotnet" => subdirs(&root("dotnet").join("sdk"))
            .into_iter()
            .map(|v| v.split('.').take(2).collect::<Vec<_>>().join("."))
            .fold(Vec::new(), |mut acc, v| {
                if !acc.contains(&v) {
                    acc.push(v);
                }
                acc
            }),
        _ => subdirs(&root(id)),
    }
}

/// What can be installed. Node majors and Go releases come from the official indexes.
pub fn available(id: &str) -> Result<Vec<String>> {
    Ok(match id {
        "node" => {
            let out = Command::new("curl")
                .args(["-fsSL", "https://nodejs.org/dist/index.json"])
                .output()?;
            let list: Vec<serde_json::Value> = serde_json::from_slice(&out.stdout)?;
            let mut majors: Vec<String> = Vec::new();
            for v in list.iter().filter_map(|v| v["version"].as_str()) {
                let major = v.trim_start_matches('v').split('.').next().unwrap_or("");
                if major.parse::<u32>().is_ok_and(|m| m >= 18) && !majors.iter().any(|m| m == major)
                {
                    majors.push(major.into());
                }
            }
            majors
        }
        "go" => {
            let out = Command::new("curl")
                .args(["-fsSL", "https://go.dev/dl/?mode=json"])
                .output()?;
            let list: Vec<serde_json::Value> = serde_json::from_slice(&out.stdout)?;
            list.iter()
                .filter_map(|v| v["version"].as_str())
                .map(|v| v.trim_start_matches("go").to_string())
                .collect()
        }
        "rust" => vec!["stable".into()],
        _ => vec!["10.0".into(), "9.0".into(), "8.0".into()],
    })
}

fn cargo_bin() -> PathBuf {
    root("rust").join("cargo").join("bin")
}

/// Install `version` (a Node major like "22", a Go release like "1.27.2",
/// "stable" for Rust, a .NET channel like "10.0").
pub fn install(id: &str, version: &str) -> Result<()> {
    check(id)?;
    if version.is_empty() || version.contains(['/', '\\']) || version.starts_with('.') {
        bail!("Invalid version {version}");
    }
    let (_, caddy_plat) = platform()?;
    let mac = caddy_plat.starts_with("mac");
    let arm = caddy_plat.ends_with("arm64");
    match id {
        "node" => {
            let out = Command::new("curl")
                .args(["-fsSL", "https://nodejs.org/dist/index.json"])
                .output()?;
            let list: Vec<serde_json::Value> = serde_json::from_slice(&out.stdout)?;
            let full = list
                .iter()
                .filter_map(|v| v["version"].as_str())
                .find(|v| v.trim_start_matches('v').split('.').next() == Some(version))
                .with_context(|| format!("No Node.js {version} release"))?;
            let os = if mac { "darwin" } else { "linux" };
            let arch = if arm { "arm64" } else { "x64" };
            let base = format!("https://nodejs.org/dist/{full}/");
            let file = format!("node-{full}-{os}-{arch}.tar.gz");
            let sum = published_sum(&format!("{base}SHASUMS256.txt"), &file)?;
            replace_dir(&root("node").join(version), |d| {
                fetch_tgz(&format!("{base}{file}"), d, true, Some(&sum))
            })
        }
        "go" => {
            let os = if mac { "darwin" } else { "linux" };
            let arch = if arm { "arm64" } else { "amd64" };
            let file = format!("go{version}.{os}-{arch}.tar.gz");
            let list: Vec<serde_json::Value> =
                serde_json::from_str(&fetch_text("https://go.dev/dl/?mode=json&include=all")?)?;
            let sum = list
                .iter()
                .filter_map(|r| r["files"].as_array())
                .flatten()
                .find(|f| f["filename"] == file.as_str())
                .and_then(|f| f["sha256"].as_str())
                .with_context(|| format!("go.dev lists no checksum for {file}"))?
                .to_string();
            replace_dir(&root("go").join(version), |d| {
                fetch_tgz(&format!("https://go.dev/dl/{file}"), d, true, Some(&sum))
            })
        }
        "rust" => {
            let target = match (mac, arm) {
                (true, true) => "aarch64-apple-darwin",
                (true, false) => "x86_64-apple-darwin",
                (false, true) => "aarch64-unknown-linux-gnu",
                (false, false) => "x86_64-unknown-linux-gnu",
            };
            let url = format!("https://static.rust-lang.org/rustup/dist/{target}/rustup-init");
            let sum = published_sum(&format!("{url}.sha256"), "rustup-init")?;
            let init = download(&url, Some(&sum))?;
            run(Command::new("chmod").arg("+x").arg(&init))?;
            run(Command::new(&init)
                .args(["-y", "--no-modify-path", "--profile", "minimal"])
                .args(["--default-toolchain", version])
                .envs(env("rust", version)))?;
            let _ = fs::remove_file(init);
            Ok(())
        }
        _ => {
            // Microsoft publishes no checksum for dotnet-install.sh; HTTPS is the only check.
            let script = download("https://dot.net/v1/dotnet-install.sh", None)?;
            run(Command::new("bash")
                .arg(&script)
                .args(["--channel", version, "--install-dir"])
                .arg(root("dotnet"))
                .arg("--no-path"))?;
            let _ = fs::remove_file(script);
            Ok(())
        }
    }
}

pub fn uninstall(id: &str, version: &str) -> Result<()> {
    check(id)?;
    // Only a version that is actually installed: the name becomes a path to delete.
    if !installed(id).iter().any(|v| v == version) {
        bail!("{} {version} is not installed", name(id));
    }
    match id {
        "rust" => fs::remove_dir_all(root("rust")),
        "dotnet" => {
            // SDK folders are full versions (10.0.402); remove those of this channel.
            let sdk = root("dotnet").join("sdk");
            for v in subdirs(&sdk) {
                if v.starts_with(&format!("{version}.")) {
                    fs::remove_dir_all(sdk.join(v))?;
                }
            }
            Ok(())
        }
        _ => fs::remove_dir_all(root(id).join(version)),
    }
    .with_context(|| format!("{} {version} is not installed", name(id)))
}

/// Environment that puts one runtime version first on PATH and keeps its
/// caches inside Anvil.
pub fn env(id: &str, version: &str) -> Vec<(String, String)> {
    let path = std::env::var("PATH").unwrap_or_default();
    let d = |p: PathBuf| p.display().to_string();
    let mut e: Vec<(String, String)> = Vec::new();
    let cache = home().join("cache");
    let bin = match id {
        "node" => {
            e.push(("npm_config_cache".into(), d(cache.join("npm"))));
            // Native modules: node-gyp keeps Node headers in ~/Library/Caches/node-gyp otherwise.
            e.push(("npm_config_devdir".into(), d(cache.join("node-gyp"))));
            // giget (Nuxt's template download) and others use ~/.cache otherwise.
            e.push(("XDG_CACHE_HOME".into(), d(cache.join("xdg"))));
            e.push(("COREPACK_HOME".into(), d(cache.join("corepack"))));
            // Their telemetry keeps consent and ids in the user's home folder.
            e.push(("NEXT_TELEMETRY_DISABLED".into(), "1".into()));
            e.push(("NUXT_TELEMETRY_DISABLED".into(), "1".into()));
            root("node").join(version).join("bin")
        }
        "go" => {
            let goroot = root("go").join(version);
            e.push(("GOROOT".into(), d(goroot.clone())));
            e.push(("GOPATH".into(), d(cache.join("go"))));
            e.push(("GOCACHE".into(), d(cache.join("go-build"))));
            e.push(("GOTOOLCHAIN".into(), "local".into()));
            goroot.join("bin")
        }
        "rust" => {
            e.push(("RUSTUP_HOME".into(), d(root("rust").join("rustup"))));
            e.push(("CARGO_HOME".into(), d(root("rust").join("cargo"))));
            cargo_bin()
        }
        _ => {
            e.push(("DOTNET_ROOT".into(), d(root("dotnet"))));
            e.push(("DOTNET_CLI_TELEMETRY_OPTOUT".into(), "1".into()));
            e.push(("DOTNET_NOLOGO".into(), "1".into()));
            // The CLI otherwise writes first-run files to ~/.dotnet and packages to ~/.nuget.
            e.push(("DOTNET_CLI_HOME".into(), d(cache.join("dotnet"))));
            e.push(("NUGET_PACKAGES".into(), d(cache.join("nuget"))));
            root("dotnet")
        }
    };
    e.push(("PATH".into(), format!("{}:{path}", bin.display())));
    e
}

pub fn info(id: &'static str, with_available: bool) -> RuntimeInfo {
    RuntimeInfo {
        id,
        name: name(id),
        installed: installed(id),
        available: if with_available {
            available(id).unwrap_or_default()
        } else {
            vec![]
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_folders_stay_in_anvil_home() {
        for id in RUNTIMES {
            for (k, v) in env(id, "1") {
                if k != "PATH" && v.starts_with('/') {
                    assert!(v.starts_with(&*home().to_string_lossy()), "{id}: {k}={v}");
                }
            }
        }
        let node: Vec<String> = env("node", "22").into_iter().map(|(k, _)| k).collect();
        for k in [
            "npm_config_cache",
            "npm_config_devdir",
            "XDG_CACHE_HOME",
            "COREPACK_HOME",
        ] {
            assert!(node.iter().any(|n| n == k), "node env lacks {k}");
        }
    }

    #[test]
    fn versions_sort_numerically() {
        let mut v = vec!["9".to_string(), "22".into(), "1.9".into(), "1.27.2".into()];
        newest_first(&mut v);
        assert_eq!(v, ["22", "9", "1.27.2", "1.9"]);
        assert!(install("node", "../x").is_err());
        assert!(install("cobol", "1").is_err());
        for bad in ["../../..", "", "..", "22/../.."] {
            assert!(uninstall("node", bad).is_err(), "{bad}");
        }
        let path = env("go", "1.27.2");
        assert!(path.iter().any(|(k, v)| k == "GOTOOLCHAIN" && v == "local"));
    }
}
