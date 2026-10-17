//! Language toolchains for apps that run their own server: Node.js, Go, Rust,
//! .NET and Python. Each lives inside `~/.anvil` and is only put on PATH for the
//! apps Anvil runs, so system installs are never touched.
//!
//! Python comes through uv, which installs Python versions and runs projects
//! with their dependencies, all inside Anvil's folder.

use crate::{download, fetch_text, fetch_tgz, home, platform, published_sum, replace_dir, run};
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const RUNTIMES: [&str; 5] = ["node", "go", "rust", "dotnet", "python"];

/// uv, pinned with the SHA-256 its release publishes per platform.
const UV_VERSION: &str = "0.13.1";
const UV_SHA256: [(&str, &str); 4] = [
    (
        "aarch64-apple-darwin",
        "cfe454573412ff7380d15fc7dae5997ab53ffce72fce0a78e91ddbc84001acd2",
    ),
    (
        "x86_64-apple-darwin",
        "bb7b805bdcf3d0bc8c74ceb94569e6b3e8aa35bd492acdbb9d46bf46d9d4ff6b",
    ),
    (
        "aarch64-unknown-linux-gnu",
        "9041c73c99fdc5892cf128140beb354eca9231d502068cc36e6cb53a8e59ad14",
    ),
    (
        "x86_64-unknown-linux-gnu",
        "eadd9d27c134ab5ac1ffd4473b97a741407f3ea238288f03121fc4deddf90220",
    ),
];

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
        "python" => "Python",
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
        // uv's folders: cpython-3.13.16-macos-aarch64-none (and a cpython-3.13-… link).
        "python" => subdirs(&root("python"))
            .into_iter()
            .filter_map(|d| {
                let v = d.strip_prefix("cpython-")?.split('-').next()?;
                Some(v.split('.').take(2).collect::<Vec<_>>().join("."))
            })
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
        "python" => ["3.14", "3.13", "3.12", "3.11"].map(String::from).to_vec(),
        _ => vec!["10.0".into(), "9.0".into(), "8.0".into()],
    })
}

fn uv() -> PathBuf {
    root("uv").join("uv")
}

/// uv itself, downloaded once and checked against its pinned checksum.
fn ensure_uv(mac: bool, arm: bool) -> Result<()> {
    // A newer pinned uv replaces an older one.
    let marker = root("uv").join("VERSION");
    if uv().exists() && fs::read_to_string(&marker).is_ok_and(|v| v.trim() == UV_VERSION) {
        return Ok(());
    }
    let target = match (mac, arm) {
        (true, true) => "aarch64-apple-darwin",
        (true, false) => "x86_64-apple-darwin",
        (false, true) => "aarch64-unknown-linux-gnu",
        (false, false) => "x86_64-unknown-linux-gnu",
    };
    let sum = UV_SHA256
        .iter()
        .find(|(t, _)| *t == target)
        .map(|(_, s)| *s)
        .with_context(|| format!("No pinned checksum for uv on {target}"))?;
    replace_dir(&root("uv"), |d| {
        fetch_tgz(
            &format!(
                "https://github.com/astral-sh/uv/releases/download/{UV_VERSION}/uv-{target}.tar.gz"
            ),
            d,
            true,
            Some(sum),
        )?;
        Ok(fs::write(d.join("VERSION"), UV_VERSION)?)
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
        // uv downloads Python (python-build-standalone) and checks it against
        // the checksums built into uv.
        "python" => {
            // 3.13 style only: anything else would reach uv as a flag or another interpreter.
            let mut parts = version.split('.');
            let ok = matches!((parts.next(), parts.next(), parts.next()), (Some(a), Some(b), None)
                if !a.is_empty() && !b.is_empty() && a.chars().chain(b.chars()).all(|c| c.is_ascii_digit()));
            if !ok {
                bail!("Python versions look like 3.13");
            }
            ensure_uv(mac, arm)?;
            run(Command::new(uv())
                .args(["python", "install", version])
                .envs(env("python", version)))
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
            // The SDK archive from Microsoft's release list, which carries its SHA-512.
            // (dotnet-install.sh has no published checksum and would run as a script.)
            let meta: serde_json::Value = serde_json::from_str(&fetch_text(&format!(
                "https://builds.dotnet.microsoft.com/dotnet/release-metadata/{version}/releases.json"
            ))?)
            .with_context(|| format!("No .NET {version} release list"))?;
            let latest = meta["latest-sdk"]
                .as_str()
                .with_context(|| format!("No .NET {version} SDK"))?;
            let rid = match (mac, arm) {
                (true, true) => "osx-arm64",
                (true, false) => "osx-x64",
                (false, true) => "linux-arm64",
                (false, false) => "linux-x64",
            };
            let name = format!("dotnet-sdk-{rid}.tar.gz");
            let file = sdk_file(&meta, latest, &name)
                .with_context(|| format!("No {name} for .NET SDK {latest}"))?;
            // Unpack aside, then move in what isn't there yet: an interrupted
            // install leaves no half SDK, and other channels' files stay as they are.
            let tmp = home().join("build").join("dotnet-sdk");
            let _ = fs::remove_dir_all(&tmp);
            let mut channels = installed("dotnet");
            channels.push(version.to_string());
            newest_first(&mut channels);
            let ok = fetch_tgz(&file.0, &tmp, false, Some(&file.1))
                .and_then(|_| merge_new(&tmp, &root("dotnet"), channels[0] == version));
            let _ = fs::remove_dir_all(&tmp);
            ok
        }
    }
}

/// Move what `src` has and `dst` lacks into `dst`, folder by folder; files
/// at the top (the `dotnet` host, licences) are replaced only by the newest
/// channel. `sdk` goes last, so a found SDK always has its runtime.
fn merge_new(src: &Path, dst: &Path, newest: bool) -> Result<()> {
    fs::create_dir_all(dst)?;
    let mut entries: Vec<_> = fs::read_dir(src)?.flatten().map(|e| e.path()).collect();
    entries.sort_by_key(|p| p.file_name().is_some_and(|n| n == "sdk"));
    for from in entries {
        let to = dst.join(from.file_name().context("no file name")?);
        let dir = from.is_dir();
        if to.symlink_metadata().is_err() || (newest && !dir) {
            fs::rename(&from, &to)?;
        } else if dir && to.is_dir() {
            merge_new(&from, &to, false)?;
        }
    }
    Ok(())
}

/// URL and SHA-512 of `name` for SDK `version` in a .NET releases.json. Only
/// Microsoft's download host is accepted.
fn sdk_file(meta: &serde_json::Value, version: &str, name: &str) -> Option<(String, String)> {
    let releases = meta["releases"].as_array()?;
    let sdk = releases
        .iter()
        .flat_map(|r| {
            r["sdks"]
                .as_array()
                .cloned()
                .unwrap_or_else(|| vec![r["sdk"].clone()])
        })
        .find(|s| s["version"] == version)?;
    let file = sdk["files"]
        .as_array()?
        .iter()
        .find(|f| f["name"] == name)?;
    let url = file["url"].as_str()?;
    let hash = file["hash"].as_str()?;
    (url.starts_with("https://builds.dotnet.microsoft.com/") && hash.len() == 128)
        .then(|| (url.to_string(), hash.to_string()))
}

pub fn uninstall(id: &str, version: &str) -> Result<()> {
    check(id)?;
    // Only a version that is actually installed: the name becomes a path to delete.
    if !installed(id).iter().any(|v| v == version) {
        bail!("{} {version} is not installed", name(id));
    }
    match id {
        "rust" => fs::remove_dir_all(root("rust")),
        "python" => {
            return run(Command::new(uv())
                .args(["python", "uninstall", version])
                .envs(env("python", version)));
        }
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
            // giget (Nuxt's templates) and yarn use ~/.cache, pnpm ~/.local/state,
            // unless the user points them elsewhere; their choice stands.
            for (var, dir) in [("XDG_CACHE_HOME", "xdg"), ("XDG_STATE_HOME", "xdg-state")] {
                if std::env::var_os(var).is_none() {
                    e.push((var.into(), d(cache.join(dir))));
                }
            }
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
        "python" => {
            // Every uv folder in Anvil's; only Pythons uv installed, never the system's.
            for (k, v) in [
                ("UV_PYTHON_INSTALL_DIR", root("python")),
                ("UV_CACHE_DIR", cache.join("uv")),
                ("UV_PYTHON_CACHE_DIR", cache.join("uv-python")),
                ("UV_TOOL_DIR", cache.join("uv-tools")),
                ("UV_TOOL_BIN_DIR", cache.join("uv-tools").join("bin")),
                ("UV_PYTHON_BIN_DIR", cache.join("uv-python-bin")),
            ] {
                e.push((k.into(), d(v)));
            }
            e.push(("UV_MANAGED_PYTHON".into(), "1".into()));
            e.push(("UV_PYTHON_INSTALL_BIN".into(), "0".into()));
            e.push(("UV_PYTHON".into(), version.into()));
            root("uv")
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
    fn dotnet_merge_keeps_what_is_there() {
        let base = std::env::temp_dir().join(format!("anvil-merge-{}", std::process::id()));
        let (src, dst) = (base.join("new"), base.join("dotnet"));
        for d in ["sdk/10.0.402", "shared/App/10.0.13"] {
            fs::create_dir_all(src.join(d)).unwrap();
        }
        fs::write(src.join("dotnet"), "new host").unwrap();
        fs::write(src.join("shared/App/10.0.13/a"), "new").unwrap();
        fs::create_dir_all(dst.join("shared/App/9.0.1")).unwrap();
        fs::write(dst.join("dotnet"), "old host").unwrap();
        // An older channel keeps the host; existing versions are left alone.
        merge_new(&src, &dst, false).unwrap();
        assert_eq!(fs::read_to_string(dst.join("dotnet")).unwrap(), "old host");
        assert!(dst.join("sdk/10.0.402").is_dir());
        assert!(dst.join("shared/App/9.0.1").is_dir());
        assert_eq!(
            fs::read_to_string(dst.join("shared/App/10.0.13/a")).unwrap(),
            "new"
        );
        fs::write(src.join("dotnet"), "newest host").unwrap();
        merge_new(&src, &dst, true).unwrap();
        assert_eq!(
            fs::read_to_string(dst.join("dotnet")).unwrap(),
            "newest host"
        );
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn dotnet_sdk_from_release_list() {
        let hash = "a".repeat(128);
        let meta = serde_json::json!({ "releases": [
            { "sdk": { "version": "9.0.100", "files": [] } },
            { "sdk": { "version": "x" }, "sdks": [{ "version": "10.0.402", "files": [
                { "name": "dotnet-sdk-osx-arm64.tar.gz",
                  "url": "https://builds.dotnet.microsoft.com/dotnet/Sdk/10.0.402/dotnet-sdk-10.0.402-osx-arm64.tar.gz",
                  "hash": hash },
                { "name": "dotnet-sdk-osx-x64.tar.gz", "url": "https://evil.example/x.tar.gz", "hash": hash },
            ]}]},
        ]});
        let (url, sum) = sdk_file(&meta, "10.0.402", "dotnet-sdk-osx-arm64.tar.gz").unwrap();
        assert!(url.ends_with("dotnet-sdk-10.0.402-osx-arm64.tar.gz"));
        assert_eq!(sum, hash);
        // Another host, or a file the list doesn't have: refused.
        assert!(sdk_file(&meta, "10.0.402", "dotnet-sdk-osx-x64.tar.gz").is_none());
        assert!(sdk_file(&meta, "10.0.402", "dotnet-sdk-linux-x64.tar.gz").is_none());
    }

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
        for k in ["npm_config_cache", "npm_config_devdir", "COREPACK_HOME"] {
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
