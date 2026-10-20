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
    /// Installed versions apps are pinned to, with those apps (`crate::pinned_to`).
    pub pinned_by: std::collections::BTreeMap<String, Vec<String>>,
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

pub(crate) fn root(id: &str) -> PathBuf {
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
            let list: Vec<serde_json::Value> =
                crate::fetch_json("https://nodejs.org/dist/index.json")?;
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
            let list: Vec<serde_json::Value> = crate::fetch_json("https://go.dev/dl/?mode=json")?;
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

/// Where a Node.js version's programs are: `bin`, or on Windows the folder itself
/// (node.exe, npm.cmd).
pub fn node_bin(version: &str) -> PathBuf {
    let dir = root("node").join(version);
    if cfg!(windows) { dir } else { dir.join("bin") }
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
    let windows = cfg!(windows);
    // ponytail: Rust needs Visual Studio's build tools and uv a pinned Windows checksum.
    if windows && matches!(id, "rust" | "python") {
        bail!("{} isn't available on Windows yet", name(id));
    }
    // Windows downloads are zips, which Windows' tar unpacks too (`fetch_tgz`).
    let archive = if windows { "zip" } else { "tar.gz" };
    match id {
        "node" => {
            let list: Vec<serde_json::Value> =
                crate::fetch_json("https://nodejs.org/dist/index.json")?;
            let full = list
                .iter()
                .filter_map(|v| v["version"].as_str())
                .find(|v| v.trim_start_matches('v').split('.').next() == Some(version))
                .with_context(|| format!("No Node.js {version} release"))?;
            let os = if windows {
                "win"
            } else if mac {
                "darwin"
            } else {
                "linux"
            };
            let arch = if arm { "arm64" } else { "x64" };
            let base = format!("https://nodejs.org/dist/{full}/");
            let file = format!("node-{full}-{os}-{arch}.{archive}");
            let sum = published_sum(&format!("{base}SHASUMS256.txt"), &file)?;
            replace_dir(&root("node").join(version), |d| {
                fetch_tgz(&format!("{base}{file}"), d, true, Some(&sum))
            })
        }
        "go" => {
            let os = if windows {
                "windows"
            } else if mac {
                "darwin"
            } else {
                "linux"
            };
            let arch = if arm { "arm64" } else { "amd64" };
            let file = format!("go{version}.{os}-{arch}.{archive}");
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
            run(crate::platform::command(uv())
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
            run(crate::platform::command("chmod").arg("+x").arg(&init))?;
            run(crate::platform::command(&init)
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
                _ if windows && arm => "win-arm64",
                _ if windows => "win-x64",
                (true, true) => "osx-arm64",
                (true, false) => "osx-x64",
                (false, true) => "linux-arm64",
                (false, false) => "linux-x64",
            };
            let name = format!("dotnet-sdk-{rid}.{archive}");
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
            return run(crate::platform::command(uv())
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
    let path = crate::env_path();
    let d = |p: PathBuf| p.display().to_string();
    let mut e: Vec<(String, String)> = Vec::new();
    let cache = home().join("cache");
    let bin = match id {
        "node" => {
            e.push(("npm_config_cache".into(), d(cache.join("npm"))));
            // Native modules: node-gyp keeps Node headers in ~/Library/Caches/node-gyp otherwise.
            // npm 11 warns about npm_config_devdir ("Unknown env config"); node-gyp also
            // reads its options from npm_package_config_node_gyp_*, which npm leaves alone.
            e.push((
                "npm_package_config_node_gyp_devdir".into(),
                d(cache.join("node-gyp")),
            ));
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
            node_bin(version)
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
            // Languages are installed only when asked for (Settings › Languages),
            // never by uv on its own because a project wants another version.
            e.push(("UV_PYTHON_DOWNLOADS".into(), "manual".into()));
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
    // Node apps also find the package managers installed from Settings.
    let path = if id == "node" { with_pms(&path) } else { path };
    e.push(("PATH".into(), crate::prepend_path(&bin, &path)));
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
        pinned_by: Default::default(),
    }
}

/// Where Anvil puts the package managers it installs: pnpm and yarn as npm
/// globals (`<pm>/bin`), bun as its standalone binary (`<bun>/bin`).
fn pm_dirs() -> [PathBuf; 2] {
    [root("pm").join("bin"), root("bun").join("bin")]
}

/// `path` with Anvil's package managers in front.
pub fn with_pms(path: &str) -> String {
    let [pm, bun] = pm_dirs();
    crate::prepend_path(&pm, &crate::prepend_path(&bun, path))
}

/// One package manager as found on a PATH.
#[derive(Serialize)]
pub struct Found {
    pub version: Option<String>,
    pub path: String,
    /// "Anvil", "Homebrew", "Vite+", "Corepack", "Bundled with Node.js 24",
    /// "Homebrew (Node.js 26)" or "System".
    pub source: String,
}

/// npm, pnpm, yarn or bun, for the apps Anvil runs and for a terminal.
#[derive(Serialize)]
pub struct PackageManager {
    pub name: &'static str,
    /// On the PATH apps get: the newest Anvil Node.js and Anvil's package
    /// managers first, then the login shell's.
    pub apps: Option<Found>,
    /// On the login shell's PATH alone; `None` when it is the same program.
    pub terminal: Option<Found>,
    /// Both find the same program.
    pub same: bool,
    /// Installed by Anvil, so Settings can uninstall it.
    pub anvil: bool,
}

/// The first `name` on `path`, its resolved file, and its `--version` run
/// with `envs`. Corepack may not download anything here.
fn find(name: &str, path: &str, envs: &[(String, String)]) -> Option<(Found, PathBuf)> {
    let bin = which(name, path)?;
    let version = crate::output_within(
        crate::platform::command(&bin)
            .arg("--version")
            .envs(envs.iter().cloned())
            .env("PATH", path)
            .env("COREPACK_ENABLE_NETWORK", "0")
            .env("COREPACK_ENABLE_DOWNLOAD_PROMPT", "0")
            .current_dir(std::env::temp_dir()),
        std::time::Duration::from_secs(3),
    )
    .and_then(|o| {
        o.lines()
            .next()
            .map(|l| l.trim().trim_start_matches('v').to_string())
    })
    .filter(|v| !v.is_empty());
    let real = crate::platform::canonicalize(&bin).unwrap_or_else(|_| bin.clone());
    let user = crate::user_home();
    let source = source_of(&real, &home(), &user);
    Some((
        Found {
            version,
            path: bin.display().to_string(),
            source,
        },
        real,
    ))
}

/// npm, pnpm, yarn and bun as an app sees them (the newest Anvil Node.js) and
/// as a terminal does (the login shell's PATH, without Anvil's runtimes).
pub fn package_managers() -> Vec<PackageManager> {
    let base = crate::env_path();
    let apps_env = match installed("node").first() {
        Some(v) => env("node", v),
        None => vec![("PATH".into(), with_pms(&base))],
    };
    let apps_path = apps_env
        .iter()
        .find(|(k, _)| k == "PATH")
        .map(|(_, v)| v.clone())
        .unwrap_or_default();
    let login = crate::login_path().unwrap_or(base);
    let mine = [root("pm"), root("bun")];
    ["npm", "pnpm", "yarn", "bun"]
        .into_iter()
        .map(|name| {
            let apps = find(name, &apps_path, &apps_env);
            let apps_real = apps.as_ref().map(|(_, r)| r.clone());
            let term_real =
                which(name, &login).map(|b| crate::platform::canonicalize(&b).unwrap_or(b));
            let same = term_real.is_some() && term_real == apps_real;
            PackageManager {
                name,
                anvil: apps_real.is_some_and(|r| mine.iter().any(|d| r.starts_with(d))),
                apps: apps.map(|(f, _)| f),
                terminal: if same {
                    None
                } else {
                    find(name, &login, &[]).map(|(f, _)| f)
                },
                same,
            }
        })
        .collect()
}

/// The first executable `name` in a PATH string.
fn which(name: &str, path: &str) -> Option<PathBuf> {
    path.split(crate::platform::PATH_SEP)
        .filter(|d| !d.is_empty())
        .map(|d| Path::new(d).join(crate::platform::exe(name)))
        .find(|p| fs::metadata(p).is_ok_and(|m| crate::platform::is_executable(p, &m)))
}

/// Where a program (its resolved path) comes from.
fn source_of(real: &Path, anvil: &Path, user_home: &Path) -> String {
    let s = real.to_string_lossy();
    if s.contains("/corepack/") {
        return "Corepack".into();
    }
    // npm itself, bundled with Anvil's Node.js or Homebrew's (node, node@22).
    if s.contains("/lib/node_modules/npm/") {
        if let Ok(rest) = real.strip_prefix(anvil.join("runtimes").join("node"))
            && let Some(v) = rest.iter().next()
        {
            return format!("Bundled with Node.js {}", v.to_string_lossy());
        }
        if let Some(v) = s
            .split_once("/Cellar/node")
            .and_then(|(_, r)| r.split('/').nth(1))
            .and_then(|v| v.split('.').next())
        {
            return format!("Homebrew (Node.js {v})");
        }
    }
    if real.starts_with(anvil) {
        "Anvil"
    } else if [
        "/opt/homebrew/",
        "/usr/local/Cellar/",
        "/usr/local/Homebrew/",
    ]
    .iter()
    .any(|p| s.starts_with(p))
    {
        "Homebrew"
    } else if real.starts_with(user_home.join(".local/share/vite-plus")) {
        "Vite+"
    } else {
        "System"
    }
    .into()
}

type Runner<'a> = &'a mut dyn FnMut(&mut Command) -> Result<()>;

/// Install a package manager apps lack into Anvil's folder.
pub fn install_pm(name: &str) -> Result<()> {
    // Errors read in a row of Settings: the tool's last word, not the whole command line.
    install_pm_in(name, &home(), &mut |c| {
        let out = c.output()?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            let last = err
                .lines()
                .rev()
                .find(|l| !l.trim().is_empty() && !l.contains("A complete log of this run"));
            bail!("{}", last.unwrap_or("The install failed").trim());
        }
        Ok(())
    })
}

/// pnpm and yarn with the newest Anvil Node.js's npm into `<anvil>/runtimes/pm`;
/// bun as its official release, checked against the published SHA-256.
fn install_pm_in(name: &str, anvil: &Path, runner: Runner) -> Result<()> {
    let runtimes = anvil.join("runtimes");
    match name {
        "pnpm" | "yarn" => {
            let node = runtimes.join("node");
            let v = subdirs(&node)
                .into_iter()
                .next()
                .with_context(|| format!("{name} is installed with npm: install Node.js first"))?;
            runner(
                crate::platform::command(node.join(&v).join("bin").join("npm"))
                    .args(["install", "-g", "--prefix"])
                    .arg(runtimes.join("pm"))
                    .arg(name)
                    .envs(env("node", &v)),
            )
        }
        "bun" => {
            let (_, plat) = platform()?;
            let asset = bun_asset(plat.starts_with("mac"), plat.ends_with("arm64"));
            // ponytail: "latest" for both files; a release landing in between fails the checksum, never installs.
            let base = "https://github.com/oven-sh/bun/releases/latest/download/";
            let sum = published_sum(&format!("{base}SHASUMS256.txt"), &format!("{asset}.zip"))?;
            replace_dir(&runtimes.join("bun"), |d| {
                let zip = download(&format!("{base}{asset}.zip"), Some(&sum))?;
                let bin = d.join("bin");
                let ok = fs::create_dir_all(&bin)
                    .map_err(Into::into)
                    .and_then(|_| runner(&mut unzip_bun(&zip, asset, &bin)));
                let _ = fs::remove_file(&zip);
                ok?;
                let bun = crate::platform::exe("bun");
                Ok(crate::platform::link_program(
                    &bun,
                    &bin.join(crate::platform::exe("bunx")),
                )?)
            })
        }
        _ => bail!("Anvil installs pnpm, yarn and bun, not {name}"),
    }
}

/// bun's release asset for a platform.
fn bun_asset(mac: bool, arm: bool) -> &'static str {
    match (mac, arm) {
        (true, true) => "bun-darwin-aarch64",
        (true, false) => "bun-darwin-x64",
        (false, true) => "bun-linux-aarch64",
        (false, false) => "bun-linux-x64",
    }
}

/// Only the `bun` binary out of the release zip, straight into `bin`.
fn unzip_bun(zip: &Path, asset: &str, bin: &Path) -> Command {
    let mut c = crate::platform::command("unzip");
    c.args(["-q", "-j"])
        .arg(zip)
        .arg(format!("{asset}/bun"))
        .arg("-d")
        .arg(bin);
    c
}

/// Remove a package manager Anvil installed.
pub fn uninstall_pm(name: &str) -> Result<()> {
    uninstall_pm_in(name, &home())
}

fn uninstall_pm_in(name: &str, anvil: &Path) -> Result<()> {
    let runtimes = anvil.join("runtimes");
    let missing = || format!("{name} isn't installed in Anvil");
    match name {
        "bun" => fs::remove_dir_all(runtimes.join("bun")).with_context(missing),
        "pnpm" | "yarn" => {
            let pm = runtimes.join("pm");
            let pkg = pm.join("lib").join("node_modules").join(name);
            if !pkg.is_dir() {
                bail!(missing());
            }
            // npm's links in bin point into the package: ../lib/node_modules/pnpm/…
            let inside = format!("node_modules/{name}/");
            for e in fs::read_dir(pm.join("bin")).into_iter().flatten().flatten() {
                if fs::read_link(e.path()).is_ok_and(|t| t.to_string_lossy().contains(&inside)) {
                    fs::remove_file(e.path())?;
                }
            }
            Ok(fs::remove_dir_all(pkg)?)
        }
        _ => bail!(missing()),
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
        for k in [
            "npm_config_cache",
            "npm_package_config_node_gyp_devdir",
            "COREPACK_HOME",
        ] {
            assert!(node.iter().any(|n| n == k), "node env lacks {k}");
        }
        // npm warns about any npm_config_ key that isn't its own config.
        for k in node.iter().filter(|k| k.starts_with("npm_config_")) {
            assert_eq!(k, "npm_config_cache", "npm warns about {k}");
        }
    }

    #[test]
    fn package_manager_sources() {
        let anvil = Path::new("/Users/x/.anvil");
        let src = |p: &str| source_of(Path::new(p), anvil, Path::new("/Users/x"));
        assert_eq!(
            src("/Users/x/.anvil/runtimes/node/24/lib/node_modules/npm/bin/npm-cli.js"),
            "Bundled with Node.js 24"
        );
        assert_eq!(
            src("/opt/homebrew/Cellar/node/26.12.0/lib/node_modules/npm/bin/npm-cli.js"),
            "Homebrew (Node.js 26)"
        );
        assert_eq!(
            src("/opt/homebrew/Cellar/node@22/22.20.0/lib/node_modules/npm/bin/npm-cli.js"),
            "Homebrew (Node.js 22)"
        );
        assert_eq!(src("/Users/x/.local/share/vite-plus/0.1.4/bin/vp"), "Vite+");
        assert_eq!(
            src("/Users/x/.anvil/runtimes/pm/lib/node_modules/yarn/bin/yarn.js"),
            "Anvil"
        );
        assert_eq!(src("/Users/x/.anvil/runtimes/bun/bin/bun"), "Anvil");
        assert_eq!(
            src("/Users/x/.nvm/versions/node/v20.1.0/lib/node_modules/npm/bin/npm-cli.js"),
            "System"
        );
        assert_eq!(
            src("/opt/homebrew/Cellar/pnpm/10.12.1/bin/pnpm"),
            "Homebrew"
        );
        assert_eq!(src("/usr/local/Cellar/yarn/1.22.22/bin/yarn"), "Homebrew");
        assert_eq!(
            src("/opt/homebrew/lib/node_modules/corepack/dist/pnpm.js"),
            "Corepack"
        );
        assert_eq!(
            src("/Users/x/.anvil/runtimes/node/24/lib/node_modules/corepack/dist/yarn.js"),
            "Corepack"
        );
        assert_eq!(
            src("/Users/x/.anvil/runtimes/node/24/lib/node_modules/pnpm/bin/pnpm.cjs"),
            "Anvil"
        );
        assert_eq!(src("/Users/x/.bun/bin/bun"), "System");
        let dir = std::env::temp_dir().join(format!("anvil-which-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("pnpm"), "").unwrap();
        // Not executable: skipped.
        assert_eq!(which("pnpm", &dir.display().to_string()), None);
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir.join("pnpm"), fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            which("pnpm", &format!("/nope::{}", dir.display())),
            Some(dir.join("pnpm"))
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn package_manager_install_and_uninstall() {
        let anvil = std::env::temp_dir().join(format!("anvil-pm-{}", std::process::id()));
        let _ = fs::remove_dir_all(&anvil);
        let mut ran: Vec<Vec<String>> = vec![];
        let mut fake = |c: &mut Command| {
            let mut argv = vec![c.get_program().to_string_lossy().into_owned()];
            argv.extend(c.get_args().map(|a| a.to_string_lossy().into_owned()));
            ran.push(argv);
            Ok(())
        };
        // pnpm and yarn need an Anvil Node.js for its npm.
        let err = install_pm_in("pnpm", &anvil, &mut fake).unwrap_err();
        assert!(err.to_string().contains("install Node.js first"), "{err}");
        for v in ["22", "24"] {
            fs::create_dir_all(anvil.join("runtimes/node").join(v)).unwrap();
        }
        for bad in ["npm", "../x", "deno", ""] {
            assert!(install_pm_in(bad, &anvil, &mut fake).is_err(), "{bad}");
        }
        install_pm_in("yarn", &anvil, &mut fake).unwrap();
        let a = |p: &str| anvil.join(p).display().to_string();
        // Only the yarn install ran: refusals run nothing.
        assert_eq!(
            ran,
            [vec![
                a("runtimes/node/24/bin/npm"),
                "install".into(),
                "-g".into(),
                "--prefix".into(),
                a("runtimes/pm"),
                "yarn".into(),
            ]]
        );
        assert_eq!(bun_asset(true, true), "bun-darwin-aarch64");
        let unzip = unzip_bun(
            Path::new("/b/x.zip"),
            "bun-darwin-aarch64",
            Path::new("/d/bin"),
        );
        assert_eq!(
            unzip.get_args().collect::<Vec<_>>(),
            [
                "-q",
                "-j",
                "/b/x.zip",
                "bun-darwin-aarch64/bun",
                "-d",
                "/d/bin"
            ]
        );

        // Uninstall removes the package and only its own links in bin.
        let pm = anvil.join("runtimes/pm");
        fs::create_dir_all(pm.join("lib/node_modules/yarn/bin")).unwrap();
        fs::create_dir_all(pm.join("bin")).unwrap();
        for (link, to) in [
            ("yarn", "../lib/node_modules/yarn/bin/yarn.js"),
            ("yarnpkg", "../lib/node_modules/yarn/bin/yarn.js"),
            ("pnpm", "../lib/node_modules/pnpm/bin/pnpm.cjs"),
        ] {
            std::os::unix::fs::symlink(to, pm.join("bin").join(link)).unwrap();
        }
        uninstall_pm_in("yarn", &anvil).unwrap();
        assert!(!pm.join("lib/node_modules/yarn").exists());
        assert!(pm.join("bin/pnpm").symlink_metadata().is_ok());
        assert!(pm.join("bin/yarn").symlink_metadata().is_err());
        assert!(uninstall_pm_in("yarn", &anvil).is_err());
        assert!(uninstall_pm_in("bun", &anvil).is_err());
        assert!(uninstall_pm_in("npm", &anvil).is_err());
        // Node apps find them, after Node.js itself.
        let path = env("node", "24")
            .into_iter()
            .find(|(k, _)| k == "PATH")
            .unwrap()
            .1;
        let dirs: Vec<&str> = path.split(':').collect();
        assert!(dirs[0].ends_with("runtimes/node/24/bin"), "{path}");
        assert!(dirs[1].ends_with("runtimes/pm/bin"), "{path}");
        assert!(dirs[2].ends_with("runtimes/bun/bin"), "{path}");
        fs::remove_dir_all(anvil).unwrap();
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
