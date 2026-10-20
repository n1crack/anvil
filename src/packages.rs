//! What an app depends on: the packages its manifests ask for and the versions
//! its lock files pin, for Composer, Node, Cargo, Go and .NET projects.

use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::SystemTime;

#[derive(Serialize, Clone, Default, Debug)]
pub struct Packages {
    /// Frameworks and notable tools with their locked versions, e.g. ("Laravel", "12.1.0").
    pub highlights: Vec<(String, String)>,
    /// The Packages tab's default badges as (key, label): the frameworks above by package
    /// name, then the platform facts by their label, e.g. ("laravel/framework", "Laravel").
    pub defaults: Vec<(String, String)>,
    pub ecosystems: Vec<Ecosystem>,
}

#[derive(Serialize, Clone, Default, Debug)]
pub struct Ecosystem {
    /// composer, npm, cargo, go or nuget; also picks the registry.
    pub id: &'static str,
    pub label: &'static str,
    /// The package manager and its version when known, e.g. "pnpm 9.15.0".
    pub manager: Option<String>,
    pub lock: Option<String>,
    /// Platform requirements, e.g. ("PHP", "^8.2") or ("Node", ">=20").
    pub facts: Vec<(String, String)>,
    /// Direct dependencies, dev ones included.
    pub packages: Vec<Package>,
    /// Every package in the lock file as (name, version).
    pub locked: Vec<(String, String)>,
    pub scripts: Vec<(String, String)>,
}

#[derive(Serialize, Clone, Default, Debug, PartialEq)]
pub struct Package {
    pub name: String,
    pub constraint: String,
    pub version: Option<String>,
    pub dev: bool,
    /// Release date from the lock file (Composer only).
    pub released: Option<String>,
}

const FILES: &[&str] = &[
    "composer.json",
    "composer.lock",
    "package.json",
    "package-lock.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "bun.lock",
    "bun.lockb",
    "Cargo.toml",
    "Cargo.lock",
    "go.mod",
];

const FRAMEWORKS: &[(&str, &str)] = &[
    ("laravel/framework", "Laravel"),
    ("symfony/framework-bundle", "Symfony"),
    ("statamic/cms", "Statamic"),
    ("livewire/livewire", "Livewire"),
    ("filament/filament", "Filament"),
    ("inertiajs/inertia-laravel", "Inertia"),
    ("next", "Next.js"),
    ("nuxt", "Nuxt"),
    ("@sveltejs/kit", "SvelteKit"),
    ("astro", "Astro"),
    ("react", "React"),
    ("vue", "Vue"),
    ("svelte", "Svelte"),
    ("@inertiajs/react", "Inertia"),
    ("@inertiajs/vue3", "Inertia"),
    ("@inertiajs/svelte", "Inertia"),
    ("tailwindcss", "Tailwind"),
    ("vite", "Vite"),
    ("electron", "Electron"),
    ("tauri", "Tauri"),
    ("axum", "Axum"),
    ("actix-web", "Actix Web"),
    ("github.com/gin-gonic/gin", "Gin"),
    ("github.com/gofiber/fiber/v2", "Fiber"),
    ("github.com/labstack/echo/v4", "Echo"),
];

/// Everything found in `dir`. Ecosystems without a manifest are left out.
pub fn read(dir: &Path) -> Packages {
    let ecosystems: Vec<Ecosystem> = [composer(dir), node(dir), cargo(dir), go(dir), dotnet(dir)]
        .into_iter()
        .flatten()
        .collect();
    let mut highlights: Vec<(String, String)> = Vec::new();
    let mut defaults: Vec<(String, String)> = Vec::new();
    for (name, label) in FRAMEWORKS {
        if highlights.iter().any(|(l, _)| l == label) {
            continue;
        }
        let found = ecosystems
            .iter()
            .flat_map(|e| &e.packages)
            .find(|p| p.name == *name);
        if let Some(p) = found {
            let v = p.version.clone().unwrap_or_else(|| p.constraint.clone());
            highlights.push((label.to_string(), v));
            defaults.push((name.to_string(), label.to_string()));
        }
    }
    for (label, _) in ecosystems.iter().flat_map(|e| &e.facts) {
        defaults.push((label.clone(), label.clone()));
    }
    Packages {
        highlights,
        defaults,
        ecosystems,
    }
}

/// Modification times of the files `read` looks at, to tell when it must look again.
type Stamp = Vec<Option<SystemTime>>;

static CACHE: Mutex<BTreeMap<PathBuf, (Stamp, Packages)>> = Mutex::new(BTreeMap::new());

/// `read`, kept until a manifest or lock file in `dir` changes.
pub fn cached(dir: &Path) -> Packages {
    let mut files: Vec<PathBuf> = FILES.iter().map(|f| dir.join(f)).collect();
    files.extend(csprojs(dir));
    let stamp: Vec<_> = files
        .iter()
        .map(|f| fs::metadata(f).and_then(|m| m.modified()).ok())
        .collect();
    if let Some((s, p)) = crate::lock(&CACHE).get(dir)
        && *s == stamp
    {
        return p.clone();
    }
    let p = read(dir);
    crate::lock(&CACHE).insert(dir.to_path_buf(), (stamp, p.clone()));
    p
}

fn json(path: &Path) -> Option<Value> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

fn str_map(v: &Value) -> Vec<(String, String)> {
    v.as_object()
        .map(|m| {
            m.iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string()))
                .collect()
        })
        .unwrap_or_default()
}

/* ---------- Composer ---------- */

fn composer(dir: &Path) -> Option<Ecosystem> {
    let manifest = json(&dir.join("composer.json"))?;
    let mut e = Ecosystem {
        id: "composer",
        label: "Composer",
        ..Default::default()
    };
    let mut locked = HashMap::new();
    if let Some(lock) = json(&dir.join("composer.lock")) {
        e.lock = Some("composer.lock".into());
        for p in ["packages", "packages-dev"]
            .iter()
            .filter_map(|k| lock[k].as_array())
            .flatten()
        {
            let name = p["name"].as_str().unwrap_or_default().to_string();
            let version = p["version"].as_str().unwrap_or_default();
            let version = version.strip_prefix('v').unwrap_or(version).to_string();
            let time = p["time"].as_str().map(|t| t.chars().take(10).collect());
            e.locked.push((name.clone(), version.clone()));
            locked.insert(name, (version, time));
        }
    }
    for (key, dev) in [("require", false), ("require-dev", true)] {
        for (name, constraint) in str_map(&manifest[key]) {
            if name == "php" {
                e.facts.push(("PHP".into(), constraint));
                continue;
            }
            // Platform packages: extensions, libraries and Composer itself.
            if name.starts_with("ext-")
                || name.starts_with("lib-")
                || name.starts_with("composer-")
                || name.starts_with("php-")
            {
                continue;
            }
            let (version, released) = locked.get(&name).cloned().unzip();
            e.packages.push(Package {
                name,
                constraint,
                version,
                dev,
                released: released.flatten(),
            });
        }
    }
    if let Some(scripts) = manifest["scripts"].as_object() {
        for (name, cmd) in scripts {
            let cmd = match cmd {
                Value::Array(a) => a
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" && "),
                v => v.as_str().unwrap_or_default().to_string(),
            };
            e.scripts.push((name.clone(), cmd));
        }
    }
    Some(e)
}

/* ---------- Node ---------- */

/// What a lock file pins: versions by name (or by "name@range" for yarn) and
/// every package in it.
#[derive(Default, Debug)]
struct Lock {
    versions: HashMap<String, String>,
    all: Vec<(String, String)>,
}

fn node(dir: &Path) -> Option<Ecosystem> {
    let manifest = json(&dir.join("package.json"))?;
    let mut e = Ecosystem {
        id: "npm",
        label: "Node",
        ..Default::default()
    };
    // The newest lock wins when a project has more than one.
    let lock = [
        "pnpm-lock.yaml",
        "package-lock.json",
        "yarn.lock",
        "bun.lock",
        "bun.lockb",
    ]
    .into_iter()
    .filter_map(|f| Some((fs::metadata(dir.join(f)).ok()?.modified().ok()?, f)))
    .max()
    .map(|(_, f)| f);
    let parsed = match lock {
        Some(f @ "pnpm-lock.yaml") => fs::read_to_string(dir.join(f)).ok().map(|t| pnpm_lock(&t)),
        Some(f @ "yarn.lock") => fs::read_to_string(dir.join(f)).ok().map(|t| yarn_lock(&t)),
        Some(f @ "package-lock.json") => json(&dir.join(f)).map(|v| npm_lock(&v)),
        _ => None,
    }
    .unwrap_or_default();
    e.lock = lock.map(String::from);
    e.manager = match manifest["packageManager"].as_str() {
        // "pnpm@9.15.0+sha512.abc" -> "pnpm 9.15.0"
        Some(pm) => Some(pm.split('+').next().unwrap_or(pm).replacen('@', " ", 1)),
        None => lock.map(|f| {
            match f {
                "pnpm-lock.yaml" => "pnpm",
                "yarn.lock" => "yarn",
                "package-lock.json" => "npm",
                _ => "bun",
            }
            .to_string()
        }),
    };
    for (name, v) in str_map(&manifest["engines"]) {
        let label = if name == "node" { "Node".into() } else { name };
        e.facts.push((label, v));
    }
    for (key, dev) in [("dependencies", false), ("devDependencies", true)] {
        for (name, constraint) in str_map(&manifest[key]) {
            if ["workspace:", "link:", "file:"]
                .iter()
                .any(|p| constraint.starts_with(p))
            {
                continue;
            }
            let version = parsed
                .versions
                .get(&name)
                .or_else(|| parsed.versions.get(&format!("{name}@{constraint}")))
                .cloned();
            e.packages.push(Package {
                name,
                constraint,
                version,
                dev,
                released: None,
            });
        }
    }
    e.locked = parsed.all;
    e.scripts = str_map(&manifest["scripts"]);
    Some(e)
}

/// package-lock.json: v2 and v3 list every install path under `packages`,
/// v1 only has the nested `dependencies` tree (its top level is enough here).
fn npm_lock(v: &Value) -> Lock {
    let mut lock = Lock::default();
    if let Some(pkgs) = v["packages"].as_object() {
        for (path, p) in pkgs {
            let Some(i) = path.rfind("node_modules/") else {
                continue;
            };
            let name = &path[i + 13..];
            let version = p["version"].as_str().unwrap_or_default().to_string();
            if path.len() == name.len() + 13 {
                lock.versions.insert(name.to_string(), version.clone());
            }
            lock.all.push((name.to_string(), version));
        }
    } else if let Some(deps) = v["dependencies"].as_object() {
        for (name, p) in deps {
            let version = p["version"].as_str().unwrap_or_default().to_string();
            lock.versions.insert(name.clone(), version.clone());
            lock.all.push((name.clone(), version));
        }
    }
    lock
}

fn unquote(s: &str) -> &str {
    s.trim().trim_matches(|c| c == '\'' || c == '"')
}

/// "1.2.3(react@19.0.0)" (v6+) or "1.2.3_react@19.0.0" (v5) -> "1.2.3".
fn pnpm_version(v: &str) -> String {
    let v = unquote(v);
    let v = v.split('(').next().unwrap_or(v);
    if v.contains(':') {
        return v.to_string(); // link:../x, file:x
    }
    let v = v.rsplit_once('@').map_or(v, |(_, v)| v); // npm aliases: name@1.2.3
    v.split('_').next().unwrap_or(v).to_string()
}

/// "'@scope/name@1.2.3(peer)'" (v9), "/name@1.2.3" (v6) or "/name/1.2.3_peer" (v5).
fn pnpm_key(key: &str) -> Option<(String, String)> {
    let k = unquote(key.trim_end_matches(':'));
    let k = k.strip_prefix('/').unwrap_or(k);
    let k = k.split('(').next().unwrap_or(k);
    let (name, version) = match k.rsplit_once('/') {
        Some((n, v)) if v.starts_with(|c: char| c.is_ascii_digit()) => (n, v),
        _ => {
            let i = k.rfind('@').filter(|&i| i > 0)?;
            (&k[..i], &k[i + 1..])
        }
    };
    Some((name.to_string(), pnpm_version(version)))
}

/// Just enough of pnpm-lock.yaml: the root project's versions and the
/// `packages` keys. Without `importers` (single-project v5 and v6 locks) the
/// dependency sections sit at the top level.
fn pnpm_lock(text: &str) -> Lock {
    // pnpm 10+ can put its own install in a first YAML document; the project is the last.
    let doc = text.rsplit("\n---\n").next().unwrap_or(text);
    let has_importers = doc.lines().any(|l| l.trim_end() == "importers:");
    let mut lock = Lock::default();
    let mut top = "";
    let mut base: Option<usize> = (!has_importers).then_some(0);
    let mut section = false;
    let mut name = String::new();
    for line in doc.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') || t == "---" {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let (key, value) = match t.split_once(": ") {
            Some((k, v)) => (unquote(k), v.trim()),
            None => (unquote(t.trim_end_matches(':')), ""),
        };
        if indent == 0 {
            top = key;
        }
        if top == "packages" {
            if indent == 2
                && t.ends_with(':')
                && let Some(p) = pnpm_key(t)
            {
                lock.all.push(p);
            }
            continue;
        }
        if has_importers {
            if top != "importers" {
                continue;
            }
            if indent == 2 {
                base = (key == "." && value.is_empty()).then_some(4);
                continue;
            }
        }
        let Some(b) = base else { continue };
        if indent == b {
            section = matches!(
                key,
                "dependencies" | "devDependencies" | "optionalDependencies"
            );
        } else if section && indent == b + 2 {
            name = key.to_string();
            if !value.is_empty() {
                lock.versions.insert(name.clone(), pnpm_version(value));
            }
        } else if section && indent == b + 4 && key == "version" {
            lock.versions.insert(name.clone(), pnpm_version(value));
        }
    }
    lock
}

/// yarn.lock v1 (`version "1.2.3"`) and berry (`version: 1.2.3`), keyed by
/// each "name@range" a block lists.
fn yarn_lock(text: &str) -> Lock {
    let mut lock = Lock::default();
    let mut specs: Vec<String> = Vec::new();
    for line in text.lines() {
        if !line.starts_with([' ', '#']) && line.ends_with(':') {
            specs = line
                .trim_end_matches(':')
                .split(", ")
                .map(|s| unquote(s).replacen("@npm:", "@", 1))
                .collect();
        } else if let Some(v) = line
            .trim_start()
            .strip_prefix("version ")
            .or_else(|| line.trim_start().strip_prefix("version: "))
            && line.starts_with("  ")
            && !line.starts_with("    ")
        {
            let v = unquote(v).to_string();
            let Some(first) = specs.first() else { continue };
            if first == "__metadata" {
                continue;
            }
            let name = first
                .rfind('@')
                .filter(|&i| i > 0)
                .map_or(&first[..], |i| &first[..i]);
            lock.all.push((name.to_string(), v.clone()));
            for s in specs.drain(..) {
                lock.versions.insert(s, v.clone());
            }
        }
    }
    lock
}

/* ---------- Cargo ---------- */

fn cargo(dir: &Path) -> Option<Ecosystem> {
    let root: toml::Table = fs::read_to_string(dir.join("Cargo.toml"))
        .ok()?
        .parse()
        .ok()?;
    let mut e = Ecosystem {
        id: "cargo",
        label: "Cargo",
        manager: Some("cargo".into()),
        ..Default::default()
    };
    let ws = root.get("workspace").and_then(|w| w.as_table());
    let ws_deps = ws
        .and_then(|w| w.get("dependencies"))
        .and_then(|d| d.as_table());
    let pkg = root
        .get("package")
        .or_else(|| ws.and_then(|w| w.get("package")))
        .and_then(|p| p.as_table());
    for (key, label) in [("edition", "Edition"), ("rust-version", "Rust")] {
        if let Some(v) = pkg.and_then(|p| p.get(key)).and_then(|v| v.as_str()) {
            e.facts.push((label.into(), v.into()));
        }
    }
    let mut manifests = vec![root.clone()];
    let members = ws
        .and_then(|w| w.get("members"))
        .and_then(|m| m.as_array())
        .into_iter()
        .flatten()
        .filter_map(|m| m.as_str());
    for m in members {
        let dirs: Vec<PathBuf> = match m.strip_suffix("/*") {
            Some(parent) => fs::read_dir(dir.join(parent))
                .into_iter()
                .flatten()
                .flatten()
                .map(|d| d.path())
                .collect(),
            None => vec![dir.join(m)],
        };
        for d in dirs {
            if let Some(t) = fs::read_to_string(d.join("Cargo.toml"))
                .ok()
                .and_then(|s| s.parse::<toml::Table>().ok())
            {
                manifests.push(t);
            }
        }
    }
    let lock = fs::read_to_string(dir.join("Cargo.lock"))
        .map(|t| cargo_lock(&t))
        .unwrap_or_default();
    if !lock.is_empty() {
        e.lock = Some("Cargo.lock".into());
    }
    for m in &manifests {
        for (key, dev) in [
            ("dependencies", false),
            ("dev-dependencies", true),
            ("build-dependencies", true),
        ] {
            for (name, spec) in m.get(key).and_then(|d| d.as_table()).into_iter().flatten() {
                let Some((name, constraint)) = cargo_dep(name, spec, ws_deps) else {
                    continue;
                };
                if e.packages.iter().any(|p| p.name == name && p.dev == dev) {
                    continue;
                }
                let version = pick(&lock, &name, &constraint);
                e.packages.push(Package {
                    name,
                    constraint,
                    version,
                    dev,
                    released: None,
                });
            }
        }
    }
    e.locked = lock;
    Some(e)
}

/// (crate name, version requirement); None for path dependencies.
fn cargo_dep(name: &str, spec: &toml::Value, ws: Option<&toml::Table>) -> Option<(String, String)> {
    let Some(t) = spec.as_table() else {
        return Some((name.into(), spec.as_str()?.into()));
    };
    if t.get("workspace").and_then(|w| w.as_bool()) == Some(true) {
        return cargo_dep(name, ws?.get(name)?, None);
    }
    if t.contains_key("path") {
        return None;
    }
    let real = t.get("package").and_then(|p| p.as_str()).unwrap_or(name);
    let constraint = match t.get("version").and_then(|v| v.as_str()) {
        Some(v) => v,
        None if t.contains_key("git") => "git",
        None => "*",
    };
    Some((real.into(), constraint.into()))
}

fn cargo_lock(text: &str) -> Vec<(String, String)> {
    let mut all = Vec::new();
    let mut name = None;
    for line in text.lines() {
        if line == "[[package]]" {
            name = None;
        } else if let Some(n) = line.strip_prefix("name = ") {
            name = Some(unquote(n).to_string());
        } else if let Some(v) = line.strip_prefix("version = ")
            && let Some(n) = name.take()
        {
            all.push((n, unquote(v).to_string()));
        }
    }
    all
}

/// The locked version of `name`; with several, the first one whose major
/// version matches the requirement.
fn pick(lock: &[(String, String)], name: &str, constraint: &str) -> Option<String> {
    let mut found = lock.iter().filter(|(n, _)| n == name).map(|(_, v)| v);
    let first = found.next()?;
    let want = nums(constraint);
    let same = |v: &String| {
        let n = nums(v);
        n[0] == want[0] && (want[0] > 0 || n[1] == want[1])
    };
    if same(first) {
        return Some(first.clone());
    }
    Some(found.find(|v| same(v)).unwrap_or(first).clone())
}

/* ---------- Go ---------- */

fn go(dir: &Path) -> Option<Ecosystem> {
    let text = fs::read_to_string(dir.join("go.mod")).ok()?;
    let mut e = Ecosystem {
        id: "go",
        label: "Go",
        manager: Some("go".into()),
        lock: Some("go.mod".into()),
        ..Default::default()
    };
    let mut block = false;
    for line in text.lines() {
        let t = line.trim();
        if let Some(v) = t.strip_prefix("go ") {
            e.facts.push(("Go".into(), v.trim().into()));
            continue;
        }
        let req = if block {
            if t == ")" {
                block = false;
                continue;
            }
            t
        } else if t == "require (" {
            block = true;
            continue;
        } else if let Some(r) = t.strip_prefix("require ") {
            r
        } else {
            continue;
        };
        let (spec, comment) = req.split_once("//").unwrap_or((req, ""));
        let mut parts = spec.split_whitespace();
        let (Some(name), Some(version)) = (parts.next(), parts.next()) else {
            continue;
        };
        e.locked.push((name.into(), version.into()));
        if comment.trim() != "indirect" {
            e.packages.push(Package {
                name: name.into(),
                constraint: version.into(),
                version: Some(version.into()),
                ..Default::default()
            });
        }
    }
    Some(e)
}

/* ---------- .NET ---------- */

fn csprojs(dir: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|d| d.path())
        .filter(|p| p.extension().is_some_and(|x| x == "csproj"))
        .collect();
    found.sort();
    found
}

/// The value of `attr="…"` inside one XML tag.
fn attr<'a>(tag: &'a str, attr: &str) -> Option<&'a str> {
    let i = tag.find(&format!(" {attr}=\""))? + attr.len() + 3;
    tag[i..].split('"').next()
}

/// The text of the first `<name>…</name>`.
fn element<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let i = xml.find(&format!("<{name}>"))? + name.len() + 2;
    xml[i..].split('<').next().map(str::trim)
}

fn csproj(xml: &str, e: &mut Ecosystem) {
    for key in ["TargetFramework", "TargetFrameworks"] {
        if let Some(v) = element(xml, key)
            && !e.facts.iter().any(|(_, f)| f == v)
        {
            e.facts.push(("Target".into(), v.into()));
        }
    }
    for (i, _) in xml.match_indices("<PackageReference") {
        let rest = &xml[i..];
        let tag = &rest[..rest.find('>').unwrap_or(rest.len())];
        let Some(name) = attr(tag, "Include") else {
            continue;
        };
        let version = attr(tag, "Version").or_else(|| {
            let body = &rest[..rest.find("</PackageReference>").unwrap_or(0)];
            element(body, "Version")
        });
        if e.packages.iter().any(|p| p.name == name) {
            continue;
        }
        e.packages.push(Package {
            name: name.into(),
            constraint: version.unwrap_or("*").into(),
            version: version.map(String::from),
            ..Default::default()
        });
    }
}

fn dotnet(dir: &Path) -> Option<Ecosystem> {
    let projects = csprojs(dir);
    if projects.is_empty() {
        return None;
    }
    let mut e = Ecosystem {
        id: "nuget",
        label: ".NET",
        manager: Some("dotnet".into()),
        ..Default::default()
    };
    for p in projects {
        if let Ok(xml) = fs::read_to_string(p) {
            csproj(&xml, &mut e);
        }
    }
    Some(e)
}

/* ---------- Updates ---------- */

#[derive(Serialize, Clone, Debug)]
pub struct Update {
    pub ecosystem: &'static str,
    pub name: String,
    pub latest: String,
    /// major, minor or patch
    pub level: &'static str,
}

/// The first three numbers in a version or constraint: "^v1.2" -> [1, 2, 0].
fn nums(v: &str) -> [u64; 3] {
    let start = v.find(|c: char| c.is_ascii_digit()).unwrap_or(v.len());
    let mut out = [0; 3];
    let core = v[start..]
        .split(['-', '+', ' ', ',', '|'])
        .next()
        .unwrap_or("");
    for (o, n) in out.iter_mut().zip(core.split('.')) {
        *o = n.parse().unwrap_or(0);
    }
    out
}

/// How far `latest` is ahead of `current`, if at all. Below 1.0 a minor bump counts as major.
fn level(current: &str, latest: &str) -> Option<&'static str> {
    let (c, l) = (nums(current), nums(latest));
    if l <= c {
        None
    } else if l[0] != c[0] || (c[0] == 0 && l[1] != c[1]) {
        Some("major")
    } else if l[1] != c[1] {
        Some("minor")
    } else {
        Some("patch")
    }
}

fn stable(v: &str) -> bool {
    !v.contains('-') && !v.starts_with("dev-") && !v.ends_with("-dev")
}

fn fetch(url: &str) -> Option<Value> {
    let args = [
        "-sfL",
        "--compressed",
        "--max-time",
        "15",
        "-A",
        "anvil (local dev environment)",
    ];
    serde_json::from_slice(&crate::curl(&args, url).ok()?).ok()
}

/// The newest stable version of one package, from its registry.
fn latest(ecosystem: &str, name: &str) -> Option<String> {
    let highest = |vs: Vec<&str>| {
        vs.into_iter()
            .filter(|v| stable(v))
            .max_by_key(|v| nums(v))
            .map(String::from)
    };
    match ecosystem {
        "composer" => {
            let v = fetch(&format!("https://repo.packagist.org/p2/{name}.json"))?;
            let list = v["packages"][name].as_array()?;
            highest(list.iter().filter_map(|p| p["version"].as_str()).collect())
                .map(|v| v.trim_start_matches('v').to_string())
        }
        "npm" => fetch(&format!("https://registry.npmjs.org/{name}/latest"))?["version"]
            .as_str()
            .map(String::from),
        "cargo" => fetch(&format!("https://crates.io/api/v1/crates/{name}"))?["crate"]
            ["max_stable_version"]
            .as_str()
            .map(String::from),
        "go" => {
            // The module proxy wants capitals escaped as "!" + lowercase.
            let path: String = name
                .chars()
                .flat_map(|c| match c.is_ascii_uppercase() {
                    true => vec!['!', c.to_ascii_lowercase()],
                    false => vec![c],
                })
                .collect();
            fetch(&format!("https://proxy.golang.org/{path}/@latest"))?["Version"]
                .as_str()
                .map(String::from)
        }
        "nuget" => {
            let id = name.to_lowercase();
            let v = fetch(&format!("https://api.nuget.org/v3-flatcontainer/{id}/index.json"))?;
            highest(v["versions"].as_array()?.iter().filter_map(Value::as_str).collect())
        }
        _ => None,
    }
}

/// Ask each registry for the newest version of every direct dependency, eight
/// at a time, and return the ones that are behind.
pub fn updates(p: &Packages) -> Vec<Update> {
    let jobs: Vec<(&'static str, &Package)> = p
        .ecosystems
        .iter()
        .flat_map(|e| e.packages.iter().map(move |p| (e.id, p)))
        // Aliases (npm:, catalog:) and git sources may not be the registry package of that name.
        .filter(|(_, p)| !p.constraint.contains(':') && p.constraint != "git")
        .collect();
    let next = AtomicUsize::new(0);
    let found = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..8.min(jobs.len()) {
            s.spawn(|| {
                while let Some((eco, pkg)) = jobs.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let Some(latest) = latest(eco, &pkg.name) else {
                        continue;
                    };
                    let current = pkg.version.as_deref().unwrap_or(&pkg.constraint);
                    if let Some(level) = level(current, &latest) {
                        crate::lock(&found).push(Update {
                            ecosystem: eco,
                            name: pkg.name.clone(),
                            latest,
                            level,
                        });
                    }
                }
            });
        }
    });
    found.into_inner().unwrap()
}

/* ---------- Security ---------- */

/// One known vulnerability in an installed package, from the package manager's audit.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Advisory {
    pub package: String,
    /// The installed version, from the audit or else the lock file.
    pub version: Option<String>,
    /// As the tool names it: info, low, moderate (medium for Composer), high or critical.
    pub severity: String,
    pub title: String,
    pub url: Option<String>,
    /// Where it is fixed when the tool says, e.g. ">=1.2.6" (a range) or "1.2.8".
    pub fixed: Option<String>,
    /// The CVE id when the tool gives one, e.g. "CVE-2021-44906".
    pub cve: Option<String>,
    /// The affected versions as the tool writes them, e.g. ">=1.0.0 <1.2.6".
    pub range: Option<String>,
}

fn advisory(package: &str, version: Option<&str>, a: &Value) -> Advisory {
    let s = |k: &str| a[k].as_str().filter(|v| !v.is_empty()).map(String::from);
    Advisory {
        package: package.into(),
        version: version.map(String::from),
        severity: s("severity").unwrap_or_else(|| "unknown".into()),
        title: s("title").or_else(|| s("Issue")).unwrap_or_default(),
        url: s("url").or_else(|| s("link")).or_else(|| s("URL")),
        // "<0.0.0" is the registry's "no fix".
        fixed: s("patched_versions").filter(|v| v != "<0.0.0"),
        cve: s("cve").or_else(|| a["cves"][0].as_str().map(String::from)),
        range: s("affectedVersions")
            .or_else(|| s("vulnerable_versions"))
            .or_else(|| s("range"))
            .or_else(|| s("Vulnerable Versions")),
    }
}

/// `composer audit --format=json`: advisories by package, a list (or an object
/// when Composer filtered some out) each.
fn composer_audit(v: &Value) -> Option<Vec<Advisory>> {
    let by_pkg = v.get("advisories")?;
    let mut out = Vec::new();
    for (name, list) in by_pkg.as_object().into_iter().flatten() {
        let items: Vec<&Value> = match list {
            Value::Array(a) => a.iter().collect(),
            Value::Object(o) => o.values().collect(),
            _ => continue,
        };
        out.extend(items.into_iter().map(|a| advisory(name, None, a)));
    }
    Some(out)
}

/// `npm audit --json` (report v2): per vulnerable package, `via` holds its own
/// advisories as objects and the packages it is vulnerable through as names.
fn npm_audit(v: &Value) -> Option<Vec<Advisory>> {
    let mut out = Vec::new();
    for (name, vuln) in v.get("vulnerabilities")?.as_object()? {
        // A fix that is a newer version of this package, not of a parent.
        let fix = &vuln["fixAvailable"];
        let fixed = (fix["name"].as_str() == Some(name))
            .then(|| fix["version"].as_str().map(String::from))
            .flatten();
        for via in vuln["via"].as_array().into_iter().flatten() {
            if via.is_object() {
                out.push(Advisory {
                    fixed: fixed.clone(),
                    ..advisory(name, None, via)
                });
            }
        }
    }
    Some(out)
}

/// npm's advisory objects as pnpm (`advisories` by id) and yarn classic print
/// them: one per installed version in `findings`.
fn npm_advisory(a: &Value, out: &mut Vec<Advisory>) {
    let name = a["module_name"].as_str().unwrap_or_default();
    let versions: Vec<&str> = a["findings"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| f["version"].as_str())
        .collect();
    if versions.is_empty() {
        out.push(advisory(name, None, a));
    }
    for v in versions {
        out.push(advisory(name, Some(v), a));
    }
}

fn pnpm_audit(v: &Value) -> Option<Vec<Advisory>> {
    let mut out = Vec::new();
    for a in v.get("advisories")?.as_object()?.values() {
        npm_advisory(a, &mut out);
    }
    Some(out)
}

/// `yarn audit --json` (classic): one JSON object per line, an `auditAdvisory`
/// for each path to a vulnerable package.
fn yarn_classic_audit(text: &str) -> Option<Vec<Advisory>> {
    let mut out = Vec::new();
    let mut summary = false;
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let v: Value = serde_json::from_str(line).ok()?;
        match v["type"].as_str() {
            Some("auditAdvisory") => npm_advisory(&v["data"]["advisory"], &mut out),
            Some("auditSummary") => summary = true,
            _ => {}
        }
    }
    (summary || !out.is_empty()).then_some(out)
}

/// `yarn npm audit --json` (berry): one `{"value": name, "children": {…}}` per
/// line, nothing at all when it finds nothing.
fn yarn_berry_audit(text: &str) -> Option<Vec<Advisory>> {
    let mut out = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let v: Value = serde_json::from_str(line).ok()?;
        let c = &v["children"];
        let name = v["value"].as_str()?;
        let severity = c["Severity"].as_str().map(str::to_lowercase);
        let versions: Vec<&str> = c["Tree Versions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        let one = |version: Option<&str>| Advisory {
            severity: severity.clone().unwrap_or_else(|| "unknown".into()),
            ..advisory(name, version, c)
        };
        if versions.is_empty() {
            out.push(one(None));
        }
        out.extend(versions.into_iter().map(|v| one(Some(v))));
    }
    Some(out)
}

/// `bun audit --json`: advisories by package name, without installed or fixed versions.
fn bun_audit(v: &Value) -> Option<Vec<Advisory>> {
    let mut out = Vec::new();
    for (name, list) in v.as_object()? {
        for a in list.as_array().into_iter().flatten() {
            out.push(advisory(name, None, a));
        }
    }
    Some(out)
}

fn parse_audit(kind: &str, out: &str) -> Option<Vec<Advisory>> {
    let json = || serde_json::from_str::<Value>(out).ok();
    match kind {
        "composer" => composer_audit(&json()?),
        "npm" => npm_audit(&json()?),
        "pnpm" => pnpm_audit(&json()?),
        "yarn" => yarn_classic_audit(out),
        "yarn-berry" => yarn_berry_audit(out),
        "bun" => bun_audit(&json()?),
        _ => None,
    }
}

/// The audit for an ecosystem: (parser, program, arguments), the program
/// picked by the lock file.
fn audit_command(
    e: &Ecosystem,
    dir: &Path,
) -> anyhow::Result<(&'static str, &'static str, &'static [&'static str])> {
    Ok(match (e.id, e.lock.as_deref()) {
        // The lock file, so it works before `composer install`; abandoned packages aren't vulnerabilities.
        ("composer", Some(_)) => (
            "composer",
            "composer",
            &[
                "audit",
                "--locked",
                "--format=json",
                "--abandoned=ignore",
                "--no-interaction",
            ],
        ),
        ("npm", Some("package-lock.json")) => ("npm", "npm", &["audit", "--json"]),
        ("npm", Some("pnpm-lock.yaml")) => ("pnpm", "pnpm", &["audit", "--json"]),
        ("npm", Some("yarn.lock")) => {
            let berry = fs::read_to_string(dir.join("yarn.lock"))
                .is_ok_and(|t| t.lines().any(|l| l == "__metadata:"));
            if berry {
                let args = &["npm", "audit", "--all", "--recursive", "--json"];
                ("yarn-berry", "yarn", args)
            } else {
                ("yarn", "yarn", &["audit", "--json"])
            }
        }
        ("npm", Some(_)) => ("bun", "bun", &["audit", "--json"]),
        ("composer" | "npm", None) => {
            anyhow::bail!("No lock file to audit yet: install the packages first")
        }
        _ => anyhow::bail!("Anvil can't check {} packages for vulnerabilities", e.label),
    })
}

/// The most severe first, then by package.
fn severity_rank(s: &str) -> u8 {
    match s {
        "critical" => 0,
        "high" => 1,
        "moderate" | "medium" => 2,
        "low" => 3,
        _ => 4,
    }
}

/// Run the package manager's own audit for ecosystem `eco` in `dir`, with the
/// PHP and Node versions Anvil gives the app.
pub fn audit(dir: &Path, eco: &str, cfg: &crate::Config) -> anyhow::Result<Vec<Advisory>> {
    use anyhow::Context;
    let p = cached(dir);
    let e = p
        .ecosystems
        .iter()
        .find(|e| e.id == eco)
        .with_context(|| format!("No {eco} packages in {}", dir.display()))?;
    let (kind, tool, args) = audit_command(e, dir)?;
    let (program, pre, env) = match tool {
        "composer" | "npm" => crate::shims::resolve(tool, dir, cfg)?,
        // bun needs no Node; Anvil's package managers come first like for the apps.
        "bun" => (
            tool.into(),
            vec![],
            vec![("PATH".into(), crate::runtimes::with_pms(&crate::env_path()))],
        ),
        // pnpm and yarn run on the app's Node, whose PATH has Anvil's package managers.
        _ => (
            tool.into(),
            vec![],
            crate::shims::resolve("node", dir, cfg)?.2,
        ),
    };
    // ponytail: no time limit, the tools time out on the network themselves.
    let out = crate::platform::command(&program)
        .args(pre)
        .args(args)
        .envs(env)
        .env("COREPACK_ENABLE_DOWNLOAD_PROMPT", "0")
        .env("NO_COLOR", "1")
        .env("FORCE_COLOR", "0")
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .output()
        .with_context(|| format!("couldn't run {tool}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let parsed = parse_audit(kind, &stdout);
    let mut list = match parsed {
        // An empty answer is only believed from a run that succeeded.
        Some(l) if !l.is_empty() || out.status.success() => l,
        _ => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let npm_error = serde_json::from_str::<Value>(&stdout)
                .ok()
                .and_then(|v| v["error"]["summary"].as_str().map(String::from));
            let why = npm_error
                .or_else(|| {
                    // Yarn berry reports its errors on stdout.
                    stderr
                        .lines()
                        .chain(stdout.lines())
                        .map(str::trim)
                        // Composer's "In AuditCommand.php line 120:" header.
                        .find(|l| !l.is_empty() && !(l.starts_with("In ") && l.ends_with(':')))
                        .map(String::from)
                })
                .unwrap_or_else(|| format!("exited with {}", out.status));
            anyhow::bail!("{tool} audit failed: {why}");
        }
    };
    for a in list.iter_mut().filter(|a| a.version.is_none()) {
        a.version = e
            .packages
            .iter()
            .find(|p| p.name == a.package)
            .and_then(|p| p.version.clone())
            .or_else(|| {
                e.locked
                    .iter()
                    .find(|(n, _)| *n == a.package)
                    .map(|(_, v)| v.clone())
            });
    }
    // Yarn classic repeats an advisory for each path to the package.
    list.sort_by(|a, b| {
        (severity_rank(&a.severity), &a.package, &a.version, &a.title).cmp(&(
            severity_rank(&b.severity),
            &b.package,
            &b.version,
            &b.title,
        ))
    });
    list.dedup();
    Ok(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pnpm_v9_with_importers_and_two_documents() {
        let text = "---
lockfileVersion: '9.0'

importers:

  .:
    packageManagerDependencies:
      pnpm:
        specifier: 11.0.0
        version: 11.0.0

packages:

  '@pnpm/exe@11.0.0':
    resolution: {integrity: x}

---
lockfileVersion: '9.0'

settings:
  autoInstallPeers: true

catalogs:
  default:
    vite:
      specifier: 1.1.0
      version: 1.1.0

importers:

  .:
    dependencies:
      '@inertiajs/react':
        specifier: ^2.0.0
        version: 2.3.26(react-dom@19.2.7(react@19.2.7))(react@19.2.7)
      ui:
        specifier: workspace:*
        version: link:../ui
    devDependencies:
      vite:
        specifier: ^7.0.0
        version: 7.3.5(jiti@2.7.0)

  apps/web:
    dependencies:
      react:
        specifier: ^19.0.0
        version: 19.2.7

packages:

  '@inertiajs/react@2.3.26':
    resolution: {integrity: x}

  vite@7.3.5:
    resolution: {integrity: x}

snapshots:

  vite@7.3.5(jiti@2.7.0):
    dependencies:
      jiti: 2.7.0
";
        let lock = pnpm_lock(text);
        assert_eq!(lock.versions["@inertiajs/react"], "2.3.26");
        assert_eq!(lock.versions["vite"], "7.3.5");
        assert_eq!(lock.versions["ui"], "link:../ui");
        assert!(!lock.versions.contains_key("react"));
        assert!(!lock.versions.contains_key("pnpm"));
        assert_eq!(
            lock.all,
            [
                ("@inertiajs/react".into(), "2.3.26".into()),
                ("vite".into(), "7.3.5".into())
            ]
        );
    }

    #[test]
    fn pnpm_v6_and_v5_without_importers() {
        let v6 = "lockfileVersion: '6.0'

dependencies:
  vue:
    specifier: ^3.2.0
    version: 3.3.4

devDependencies:
  '@vitejs/plugin-vue':
    specifier: ^4.0.0
    version: 4.2.3(vite@4.4.9)(vue@3.3.4)

packages:

  /@vitejs/plugin-vue@4.2.3(vite@4.4.9)(vue@3.3.4):
    resolution: {integrity: x}

  /vue@3.3.4:
    resolution: {integrity: x}
";
        let lock = pnpm_lock(v6);
        assert_eq!(lock.versions["vue"], "3.3.4");
        assert_eq!(lock.versions["@vitejs/plugin-vue"], "4.2.3");
        assert_eq!(lock.all[0], ("@vitejs/plugin-vue".into(), "4.2.3".into()));

        let v5 = "lockfileVersion: 5.4

specifiers:
  react: ^18.2.0
  react-dom: ^18.2.0

dependencies:
  react: 18.2.0
  react-dom: 18.2.0_react@18.2.0

packages:

  /react-dom/18.2.0_react@18.2.0:
    resolution: {integrity: x}

  /@babel/core/7.22.0:
    resolution: {integrity: x}
";
        let lock = pnpm_lock(v5);
        assert_eq!(lock.versions["react"], "18.2.0");
        assert_eq!(lock.versions["react-dom"], "18.2.0");
        assert!(!lock.versions.contains_key("specifiers"));
        assert_eq!(
            lock.all,
            [
                ("react-dom".into(), "18.2.0".into()),
                ("@babel/core".into(), "7.22.0".into())
            ]
        );
    }

    #[test]
    fn package_lock_v3_and_v1() {
        let v3: Value = serde_json::from_str(
            r#"{"lockfileVersion":3,"packages":{
                "":{"name":"x","dependencies":{"react":"^18.2.0"}},
                "node_modules/react":{"version":"18.2.0"},
                "node_modules/@babel/core":{"version":"7.22.0","dev":true},
                "node_modules/foo/node_modules/react":{"version":"17.0.2"}}}"#,
        )
        .unwrap();
        let lock = npm_lock(&v3);
        assert_eq!(lock.versions["react"], "18.2.0");
        assert_eq!(lock.versions["@babel/core"], "7.22.0");
        assert_eq!(lock.all.len(), 3);

        let v1: Value = serde_json::from_str(
            r#"{"lockfileVersion":1,"dependencies":{"lodash":{"version":"4.17.21"}}}"#,
        )
        .unwrap();
        assert_eq!(npm_lock(&v1).versions["lodash"], "4.17.21");
    }

    #[test]
    fn yarn_v1_and_berry() {
        let v1 = r#"# yarn lockfile v1


"@babel/parser@^7.18.4", "@babel/parser@^7.20.0":
  version "7.23.6"
  resolved "https://registry.npmjs.org/@babel/parser/-/parser-7.23.6.tgz"
  dependencies:
    foo "^1.0.0"

lodash@^4.17.21:
  version "4.17.21"
"#;
        let lock = yarn_lock(v1);
        assert_eq!(lock.versions["@babel/parser@^7.20.0"], "7.23.6");
        assert_eq!(lock.versions["lodash@^4.17.21"], "4.17.21");
        assert_eq!(lock.all[0], ("@babel/parser".into(), "7.23.6".into()));

        let berry = r#"__metadata:
  version: 8

"react@npm:^18.2.0":
  version: 18.2.0
  resolution: "react@npm:18.2.0"
"#;
        let lock = yarn_lock(berry);
        assert_eq!(lock.versions["react@^18.2.0"], "18.2.0");
        assert_eq!(lock.all, [("react".into(), "18.2.0".into())]);
    }

    #[test]
    fn cargo_lock_and_pick() {
        let text = r#"version = 4

[[package]]
name = "toml"
version = "0.8.23"
source = "registry+https://github.com/rust-lang/crates.io-index"

[[package]]
name = "toml"
version = "1.1.8"
dependencies = [
 "serde",
]
"#;
        let lock = cargo_lock(text);
        assert_eq!(lock.len(), 2);
        assert_eq!(pick(&lock, "toml", "1.1").as_deref(), Some("1.1.8"));
        assert_eq!(pick(&lock, "toml", "0.8").as_deref(), Some("0.8.23"));
        assert_eq!(pick(&lock, "serde", "1"), None);
    }

    #[test]
    fn cargo_dependency_forms() {
        let t: toml::Table = r#"
[workspace.dependencies]
serde = { version = "1.0", features = ["derive"] }
[dependencies]
anyhow = "1"
serde = { workspace = true }
local = { path = "../local" }
gitdep = { git = "https://example.com/x" }
renamed = { package = "real-name", version = "0.3" }
"#
        .parse()
        .unwrap();
        let ws = t["workspace"]["dependencies"].as_table();
        let deps = t["dependencies"].as_table().unwrap();
        let got: Vec<_> = deps
            .iter()
            .filter_map(|(n, s)| cargo_dep(n, s, ws))
            .collect();
        let want = [
            ("anyhow", "1"),
            ("gitdep", "git"),
            ("real-name", "0.3"),
            ("serde", "1.0"),
        ]
        .map(|(a, b)| (a.to_string(), b.to_string()));
        assert_eq!(got, want);
    }

    #[test]
    fn go_mod() {
        let dir = std::env::temp_dir().join(format!("anvil-pkg-go-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("go.mod"),
            "module example.com/x\n\ngo 1.22\n\nrequire github.com/google/uuid v1.6.0\n\nrequire (\n\tgithub.com/gin-gonic/gin v1.9.1\n\tgolang.org/x/net v0.20.0 // indirect\n)\n",
        )
        .unwrap();
        let e = go(&dir).unwrap();
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(e.facts, [("Go".into(), "1.22".into())]);
        let names: Vec<_> = e.packages.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            ["github.com/google/uuid", "github.com/gin-gonic/gin"]
        );
        assert_eq!(e.locked.len(), 3);
    }

    #[test]
    fn csproj_references() {
        let xml = r#"<Project Sdk="Microsoft.NET.Sdk.Web">
  <PropertyGroup>
    <TargetFramework>net10.0</TargetFramework>
  </PropertyGroup>
  <ItemGroup>
    <PackageReference Include="DotNetEnv" Version="3.1.1" />
    <PackageReference Include="Serilog">
      <Version>4.0.0</Version>
    </PackageReference>
  </ItemGroup>
</Project>"#;
        let mut e = Ecosystem::default();
        csproj(xml, &mut e);
        assert_eq!(e.facts, [("Target".into(), "net10.0".into())]);
        assert_eq!(e.packages[0].name, "DotNetEnv");
        assert_eq!(e.packages[0].version.as_deref(), Some("3.1.1"));
        assert_eq!(e.packages[1].version.as_deref(), Some("4.0.0"));
    }

    #[test]
    fn versions_compare() {
        assert_eq!(nums("^v1.2"), [1, 2, 0]);
        assert_eq!(nums(">=8.2 <9"), [8, 2, 0]);
        assert_eq!(level("12.1.0", "13.0.0"), Some("major"));
        assert_eq!(level("0.8.1", "0.9.0"), Some("major"));
        assert_eq!(level("1.2.3", "1.4.0"), Some("minor"));
        assert_eq!(level("1.2.3", "1.2.4"), Some("patch"));
        assert_eq!(level("1.2.3", "1.2.3"), None);
        assert!(!stable("2.0.0-beta.1"));
    }

    /// A Laravel app with Composer and pnpm, trimmed from a real project.
    #[test]
    fn reads_fixture_project() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/laravel-pnpm");
        let p = read(&dir);
        let ids: Vec<_> = p.ecosystems.iter().map(|e| e.id).collect();
        assert_eq!(ids, ["composer", "npm"]);
        let composer = &p.ecosystems[0];
        assert_eq!(composer.facts, [("PHP".into(), "^8.2".into())]);
        let fw = composer
            .packages
            .iter()
            .find(|x| x.name == "laravel/framework")
            .unwrap();
        assert_eq!(fw.version.as_deref(), Some("12.20.0"));
        assert_eq!(fw.released.as_deref(), Some("2025-07-08"));
        assert!(
            composer
                .packages
                .iter()
                .any(|x| x.dev && x.name == "pestphp/pest")
        );
        assert_eq!(composer.locked.len(), 3);
        assert_eq!(composer.scripts[0].0, "dev");

        let node = &p.ecosystems[1];
        assert_eq!(node.manager.as_deref(), Some("pnpm 10.12.1"));
        assert_eq!(node.lock.as_deref(), Some("pnpm-lock.yaml"));
        let vite = node.packages.iter().find(|x| x.name == "vite").unwrap();
        assert!(vite.dev);
        assert_eq!(vite.version.as_deref(), Some("7.0.4"));
        assert_eq!(node.locked.len(), 4);
        assert_eq!(
            p.highlights,
            [
                ("Laravel".into(), "12.20.0".into()),
                ("Inertia".into(), "2.0.3".into()),
                ("React".into(), "19.1.0".into()),
                ("Tailwind".into(), "4.1.11".into()),
                ("Vite".into(), "7.0.4".into()),
            ]
        );
        // The badges: the same frameworks by package, then the platform facts.
        let keys: Vec<_> = p.defaults.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            keys,
            [
                "laravel/framework",
                "inertiajs/inertia-laravel",
                "react",
                "tailwindcss",
                "vite",
                "PHP"
            ]
        );
        assert_eq!(p.defaults[0].1, "Laravel");
        assert_eq!(p.defaults[5].1, "PHP");
    }

    /// Real outputs (trimmed) of each tool on a throwaway project pinning
    /// lodash 4.17.15, minimist 1.2.0 or guzzlehttp/guzzle 7.4.0.
    fn audit_sample(kind: &str, file: &str) -> Vec<Advisory> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/audit")
            .join(file);
        parse_audit(kind, &fs::read_to_string(path).unwrap()).unwrap()
    }

    fn brief(list: &[Advisory]) -> Vec<(&str, Option<&str>, &str, Option<&str>)> {
        list.iter()
            .map(|a| {
                (
                    a.package.as_str(),
                    a.version.as_deref(),
                    a.severity.as_str(),
                    a.fixed.as_deref(),
                )
            })
            .collect()
    }

    #[test]
    fn composer_audit_json() {
        let list = audit_sample("composer", "composer.json");
        assert_eq!(
            brief(&list),
            [
                ("guzzlehttp/guzzle", None, "high", None),
                ("guzzlehttp/guzzle", None, "medium", None)
            ]
        );
        assert_eq!(
            list[0].title,
            "Guzzle: Noncanonical host can bypass host-based checks"
        );
        assert_eq!(
            list[0].url.as_deref(),
            Some("https://github.com/advisories/GHSA-v5mv-p594-2x33")
        );
        assert_eq!(list[0].cve.as_deref(), Some("CVE-2026-69246"));
        assert_eq!(list[0].range.as_deref(), Some(">=8.0.0,<8.0.1|<7.15.2"));
        // Filtered output keeps its numeric keys, so the list becomes an object.
        let v: Value = serde_json::from_str(
            r#"{"advisories":{"a/b":{"1":{"title":"T","link":"L","severity":null}}}}"#,
        )
        .unwrap();
        assert_eq!(
            composer_audit(&v).unwrap()[0],
            Advisory {
                package: "a/b".into(),
                version: None,
                severity: "unknown".into(),
                title: "T".into(),
                url: Some("L".into()),
                fixed: None,
                cve: None,
                range: None
            }
        );
        assert!(parse_audit("composer", "Composer could not find a composer.json").is_none());
    }

    #[test]
    fn npm_audit_json() {
        let list = audit_sample("npm", "npm.json");
        assert_eq!(
            brief(&list),
            [
                ("lodash", None, "high", Some("4.18.1")),
                ("lodash", None, "high", Some("4.18.1")),
                ("minimist", None, "moderate", Some("1.2.8")),
                ("minimist", None, "critical", Some("1.2.8"))
            ]
        );
        assert_eq!(list[0].title, "Command Injection in lodash");
        assert_eq!(list[0].range.as_deref(), Some("<4.17.21"));
        assert_eq!(
            list[0].url.as_deref(),
            Some("https://github.com/advisories/GHSA-35jh-r3h4-6jhm")
        );
        // A fix through a parent package isn't this package's fixed version.
        let v: Value = serde_json::from_str(
            r#"{"vulnerabilities":{"a":{"via":["b",{"title":"T","severity":"low","url":"U"}],
                "fixAvailable":{"name":"parent","version":"2.0.0"}},"b":{"via":[],"fixAvailable":true}}}"#,
        )
        .unwrap();
        assert_eq!(brief(&npm_audit(&v).unwrap()), [("a", None, "low", None)]);
        assert!(parse_audit("npm", "").is_none());
    }

    #[test]
    fn pnpm_audit_json() {
        let list = audit_sample("pnpm", "pnpm.json");
        assert_eq!(
            brief(&list),
            [
                ("minimist", Some("1.2.0"), "critical", Some(">=1.2.6")),
                ("lodash", Some("4.17.15"), "high", Some(">=4.17.21"))
            ]
        );
        assert_eq!(list[1].title, "Command Injection in lodash");
    }

    #[test]
    fn yarn_audits() {
        let classic = audit_sample("yarn", "yarn-classic.ndjson");
        assert_eq!(
            brief(&classic),
            [
                ("minimist", Some("1.2.0"), "moderate", Some(">=1.2.3")),
                ("minimist", Some("1.2.0"), "critical", Some(">=1.2.6"))
            ]
        );
        assert_eq!(
            classic[0].url.as_deref(),
            Some("https://github.com/advisories/GHSA-vh95-rmgr-6w4m")
        );
        assert_eq!(classic[0].cve.as_deref(), Some("CVE-2020-7598"));
        assert_eq!(classic[0].range.as_deref(), Some(">=1.0.0 <1.2.3"));
        // Nothing found: only the summary line.
        let clean = r#"{"type":"auditSummary","data":{"vulnerabilities":{"info":0}}}"#;
        assert_eq!(parse_audit("yarn", clean), Some(vec![]));
        assert_eq!(parse_audit("yarn", ""), None);

        let berry = audit_sample("yarn-berry", "yarn-berry.ndjson");
        assert_eq!(
            brief(&berry),
            [
                ("lodash", Some("4.17.15"), "high", None),
                ("minimist", Some("1.2.0"), "critical", None)
            ]
        );
        assert_eq!(berry[0].title, "Command Injection in lodash");
        assert_eq!(berry[0].range.as_deref(), Some("<4.17.21"));
        assert_eq!(
            berry[1].url.as_deref(),
            Some("https://github.com/advisories/GHSA-xvch-5gv4-984h")
        );
        // Berry prints nothing when it finds nothing.
        assert_eq!(parse_audit("yarn-berry", ""), Some(vec![]));
        assert_eq!(parse_audit("yarn-berry", "Usage Error: x"), None);
    }

    #[test]
    fn bun_audit_json() {
        let list = audit_sample("bun", "bun.json");
        assert_eq!(
            brief(&list),
            [
                ("lodash", None, "high", None),
                ("minimist", None, "critical", None),
                ("minimist", None, "moderate", None)
            ]
        );
        assert_eq!(list[0].title, "Command Injection in lodash");
        assert_eq!(parse_audit("bun", "{}"), Some(vec![]));
    }

    #[test]
    fn audit_follows_the_lock_file() {
        let dir = std::env::temp_dir().join(format!("anvil-pkg-audit-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let eco = |lock: Option<&str>| Ecosystem {
            id: "npm",
            label: "Node",
            lock: lock.map(String::from),
            ..Default::default()
        };
        let kind = |lock| {
            audit_command(&eco(lock), &dir)
                .map(|c| c.0)
                .map_err(|e| e.to_string())
        };
        assert_eq!(kind(Some("package-lock.json")), Ok("npm"));
        assert_eq!(kind(Some("pnpm-lock.yaml")), Ok("pnpm"));
        assert_eq!(kind(Some("bun.lock")), Ok("bun"));
        fs::write(
            dir.join("yarn.lock"),
            "# yarn lockfile v1\n\nms@^2:\n  version \"2.1.3\"\n",
        )
        .unwrap();
        assert_eq!(kind(Some("yarn.lock")), Ok("yarn"));
        fs::write(dir.join("yarn.lock"), "__metadata:\n  version: 8\n").unwrap();
        assert_eq!(kind(Some("yarn.lock")), Ok("yarn-berry"));
        assert!(kind(None).unwrap_err().contains("No lock file"));
        let cargo = Ecosystem {
            id: "cargo",
            label: "Cargo",
            ..Default::default()
        };
        assert!(audit_command(&cargo, &dir).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn cache_reuses_until_a_file_changes() {
        let dir = std::env::temp_dir().join(format!("anvil-pkg-cache-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("package.json"), r#"{"dependencies":{"a":"1"}}"#).unwrap();
        assert_eq!(cached(&dir).ecosystems[0].packages.len(), 1);
        // A newer mtime than the first write, even on coarse clocks.
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(
            dir.join("package.json"),
            r#"{"dependencies":{"a":"1","b":"2"}}"#,
        )
        .unwrap();
        assert_eq!(cached(&dir).ecosystems[0].packages.len(), 2);
        fs::remove_dir_all(&dir).unwrap();
    }
}
