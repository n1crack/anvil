//! Create a new application from a template, using Anvil's own PHP, Composer and
//! language toolchains so nothing is needed on the system.

use crate::{Config, apps, download, home, published_sum, runtimes, services};
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::cell::Cell;
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Kinds that can be created, in the order the dialog shows them.
pub const TEMPLATES: [&str; 15] = [
    "laravel", "statamic", "symfony", "php", "static", "next", "nuxt", "vite", "node", "rust",
    "go", "dotnet", "django", "fastapi", "flask",
];

const FASTAPI_MAIN: &str = r#"from fastapi import FastAPI

app = FastAPI()


@app.get("/")
def root():
    return {"hello": "FastAPI"}
"#;

const FLASK_APP: &str = r#"from flask import Flask

app = Flask(__name__)


@app.get("/")
def root():
    return "Hello from Flask"
"#;

/// Appended to a new Django project's settings.py: what Django needs behind
/// Anvil's HTTPS proxy at `.<tld>`, and only in debug.
fn django_settings(tld: &str) -> String {
    format!(
        "\n\n# Anvil serves this app over HTTPS at its local address.\nif DEBUG:\n    ALLOWED_HOSTS += [\".{tld}\", \"localhost\", \"127.0.0.1\"]\n    SECURE_PROXY_SSL_HEADER = (\"HTTP_X_FORWARDED_PROTO\", \"https\")\n"
    )
}

/// What each template needs installed first: a PHP version or a runtime id.
pub fn needs(kind: &str) -> Option<&'static str> {
    match kind {
        "laravel" | "statamic" | "symfony" => Some("php"),
        k => apps::runtime(k),
    }
}

pub(crate) fn composer() -> Result<PathBuf> {
    let phar = home().join("bin").join("composer.phar");
    if !phar.exists() {
        let url = "https://getcomposer.org/download/latest-stable/composer.phar";
        let sum = published_sum(&format!("{url}.sha256"), "composer.phar")?;
        let tmp = download(url, Some(&sum))?;
        fs::create_dir_all(phar.parent().unwrap())?;
        fs::rename(tmp, &phar)?;
    }
    Ok(phar)
}

const RUST_MAIN: &str = r#"use std::io::{Read, Write};
use std::net::TcpListener;

// Anvil passes the port in $PORT and serves this app at its .test address.
fn main() {
    let port = std::env::var("PORT").unwrap_or_else(|_| "8000".into());
    let listener = TcpListener::bind(format!("127.0.0.1:{port}")).expect("port in use");
    println!("Listening on http://127.0.0.1:{port}");
    for stream in listener.incoming().flatten() {
        let mut stream = stream;
        let mut buf = [0; 1024];
        let _ = stream.read(&mut buf);
        let body = "Hello from Rust";
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
    }
}
"#;

const GO_MAIN: &str = r#"package main

import (
	"fmt"
	"log"
	"net/http"
	"os"
)

// Anvil passes the port in $PORT and serves this app at its .test address.
func main() {
	port := os.Getenv("PORT")
	if port == "" {
		port = "8000"
	}
	http.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
		fmt.Fprintln(w, "Hello from Go")
	})
	log.Printf("Listening on http://127.0.0.1:%s", port)
	log.Fatal(http.ListenAndServe("127.0.0.1:"+port, nil))
}
"#;

const NODE_SERVER: &str = r#"// Anvil passes the port in $PORT and serves this app at its .test address.
const http = require("node:http");
const port = process.env.PORT || 8000;
http
  .createServer((req, res) => res.end("Hello from Node.js"))
  .listen(port, "127.0.0.1", () => console.log(`Listening on http://127.0.0.1:${port}`));
"#;

pub fn log_file(name: &str) -> PathBuf {
    home().join("log").join(format!("create-{name}.log"))
}

/// A template's choices, by option key: `{"kit": "livewire", "git": "on"}`.
pub type Options = BTreeMap<String, String>;

/// One choice the New application sheet offers for a template. `choices` are
/// (value, label); on/off options are exactly `on` and `off`.
#[derive(Serialize, Debug)]
pub struct Opt {
    pub key: &'static str,
    pub label: &'static str,
    pub choices: Vec<(String, String)>,
    pub default: String,
}

fn pick(key: &'static str, label: &'static str, list: &[(&str, &str)]) -> Opt {
    Opt {
        key,
        label,
        choices: list
            .iter()
            .map(|(v, l)| (v.to_string(), l.to_string()))
            .collect(),
        default: list[0].0.into(),
    }
}

fn on_off(key: &'static str, label: &'static str) -> Opt {
    pick(key, label, &[("on", label), ("off", "")])
}

/// What can be chosen when creating a `kind` app, each with its default. This
/// is also the allowlist `resolve` checks against.
pub fn options(kind: &str, cfg: &Config) -> Vec<Opt> {
    let mut v = vec![];
    match kind {
        "laravel" => {
            v.push(pick(
                "kit",
                "Starter kit",
                &[
                    ("none", "No starter kit"),
                    ("react", "React"),
                    ("vue", "Vue"),
                    ("svelte", "Svelte"),
                    ("livewire", "Livewire"),
                ],
            ));
            // SQLite, or a database in one of Anvil's MySQL or PostgreSQL services.
            let mut db = pick("db", "Database", &[("sqlite", "SQLite")]);
            for id in services::instances(cfg) {
                let kind = services::kind_of(cfg, &id).unwrap_or("");
                if matches!(kind, "mysql" | "postgres") {
                    let name = services::name(kind);
                    let label = if id == kind {
                        name.to_string()
                    } else {
                        format!("{name} · {id}")
                    };
                    db.choices.push((format!("service:{id}"), label));
                }
            }
            v.push(db);
        }
        "next" => {
            v.push(on_off("ts", "TypeScript"));
            v.push(on_off("tailwind", "Tailwind"));
        }
        "vite" => {
            v.push(pick(
                "framework",
                "Framework",
                &[
                    ("react", "React"),
                    ("vue", "Vue"),
                    ("svelte", "Svelte"),
                    ("vanilla", "Vanilla"),
                ],
            ));
            v.push(on_off("ts", "TypeScript"));
        }
        _ => {}
    }
    if matches!(kind, "laravel" | "statamic" | "next" | "nuxt" | "vite") {
        v.push(pick(
            "js",
            "Packages",
            &[("npm", "npm"), ("pnpm", "pnpm"), ("bun", "bun")],
        ));
    }
    let versions = |key, label: &str, list: Vec<String>, default: Option<String>| {
        let default = default
            .filter(|d| list.contains(d))
            .or_else(|| list.first().cloned())?;
        Some(Opt {
            key,
            label: if key == "php" { "PHP" } else { "Node.js" },
            choices: list
                .into_iter()
                .map(|v| (v.clone(), format!("{label} {v}")))
                .collect(),
            default,
        })
    };
    if needs(kind) == Some("php") || kind == "php" {
        let mut list = crate::installed_php();
        list.reverse(); // newest first
        v.extend(versions("php", "PHP", list, cfg.php.clone()));
    }
    if apps::runtime(kind) == Some("node") {
        v.extend(versions("node", "Node", runtimes::installed("node"), None));
    }
    let mut git = on_off("git", "Git");
    if !crate::git_installed() {
        git.default = "off".into();
    }
    v.push(git);
    // Every template is served; Settings › "HTTPS for new applications" sets the default.
    let mut https = on_off("https", "HTTPS");
    if cfg.https_default != Some(true) {
        https.default = "off".into();
    }
    v.push(https);
    v
}

/// The new app's HTTPS as `opts` chose it; nothing when they don't say.
fn choose_https(cfg: &mut Config, name: &str, opts: &Options) {
    if let Some(v) = opts.get("https") {
        cfg.sites.entry(crate::site_name(name)).or_default().https = Some(v == "on");
    }
}

/// `given` checked against `options(kind)`, with every option's default filled
/// in. Unknown keys and values are refused, so nothing raw reaches a command.
pub fn resolve(kind: &str, cfg: &Config, given: &Options) -> Result<Options> {
    let opts = options(kind, cfg);
    for (k, v) in given {
        let o = opts
            .iter()
            .find(|o| o.key == k)
            .with_context(|| format!("{} apps have no option {k}", apps::label(kind)))?;
        if !o.choices.iter().any(|c| c.0 == *v) {
            let list: Vec<&str> = o.choices.iter().map(|c| c.0.as_str()).collect();
            bail!(
                "{v} isn't a choice for {}; use one of: {}",
                o.label,
                list.join(", ")
            );
        }
    }
    Ok(opts
        .into_iter()
        .map(|o| (o.key.into(), given.get(o.key).cloned().unwrap_or(o.default)))
        .collect())
}

fn composer_package(kind: &str, opts: &Options) -> String {
    match (kind, opts.get("kit").map(String::as_str)) {
        ("laravel", Some(kit @ ("react" | "vue" | "svelte" | "livewire"))) => {
            format!("laravel/{kit}-starter-kit")
        }
        ("laravel", _) => "laravel/laravel".into(),
        ("statamic", _) => "statamic/statamic".into(),
        _ => "symfony/skeleton".into(),
    }
}

fn on(opts: &Options, key: &str) -> bool {
    opts.get(key).is_some_and(|v| v == "on")
}

fn js(opts: &Options) -> &str {
    opts.get("js").map_or("npm", String::as_str)
}

/// npx arguments creating a JS template's project without installing it;
/// Anvil installs with the chosen manager afterwards.
fn npx_args(kind: &str, name: &str, opts: &Options) -> Vec<String> {
    let pm = js(opts);
    let args: Vec<&str> = match kind {
        "next" => vec![
            "create-next-app@latest",
            name,
            "--yes",
            if on(opts, "ts") { "--ts" } else { "--js" },
            if on(opts, "tailwind") {
                "--tailwind"
            } else {
                "--no-tailwind"
            },
            "--app",
            "--skip-install",
            "--disable-git",
        ],
        "nuxt" => vec![
            "nuxi@latest",
            "init",
            name,
            "--packageManager",
            pm,
            "--gitInit",
            "false",
            "--no-install",
            "--template",
            "minimal",
        ],
        _ => vec!["create-vite@latest", name, "--no-interactive"],
    };
    let mut args: Vec<String> = std::iter::once("--yes")
        .chain(args)
        .map(String::from)
        .collect();
    match kind {
        "next" => args.push(format!("--use-{pm}")),
        "vite" => {
            let framework = opts.get("framework").map_or("react", String::as_str);
            let ts = if on(opts, "ts") { "-ts" } else { "" };
            args.extend(["--template".into(), format!("{framework}{ts}")]);
        }
        _ => {}
    }
    args
}

/// `text` (a .env) with each pair set: an existing or commented-out line for
/// the key is replaced, a missing one appended.
fn set_env(text: &str, pairs: &[(String, String)]) -> String {
    let mut lines: Vec<String> = text.lines().map(Into::into).collect();
    for (k, v) in pairs {
        let line = format!("{k}={v}");
        let key = format!("{k}=");
        match lines
            .iter()
            .position(|l| l.trim_start_matches(['#', ' ']).starts_with(&key))
        {
            Some(i) => lines[i] = line,
            None => lines.push(line),
        }
    }
    lines.join("\n") + "\n"
}

/// The log's size in characters and lines when nothing measured it.
pub const LOG_SIZE: (u16, u16) = (100, 30);

/// What every step's process (composer, npm, artisan…) gets: color, and the log
/// pane's size, so lines like artisan's dotted `… 2.13ms DONE` fit it unwrapped.
fn step_env((columns, lines): (u16, u16)) -> Vec<(String, String)> {
    vec![
        ("FORCE_COLOR".into(), "1".into()),
        ("COLUMNS".into(), columns.clamp(20, 500).to_string()),
        ("LINES".into(), lines.clamp(5, 500).to_string()),
    ]
}

/// Create `<parent>/<name>` as a `kind` project with `opts` (see `resolve`),
/// its tools told the log is `size` (columns, lines) big.
/// Output goes to `log_file(name)`: each step starts with a `==> <step>` line
/// (Download, PHP dependencies, JS dependencies, Set up), and a failure ends it.
pub fn create(
    kind: &str,
    name: &str,
    parent: &Path,
    opts: &Options,
    size: (u16, u16),
) -> Result<PathBuf> {
    crate::validate::new_app_name(name)?;
    // An earlier run's log goes, so a refusal below leaves none to read.
    let _ = fs::remove_file(log_file(name));
    if !TEMPLATES.contains(&kind) {
        bail!("Anvil can't create {kind} applications");
    }
    if !parent.is_dir() {
        bail!("{} doesn't exist", parent.display());
    }
    let dir = parent.join(name);
    if dir.exists() {
        bail!("{} already exists", dir.display());
    }
    fs::create_dir_all(home().join("log"))?;
    let log = fs::File::create(log_file(name))?;
    let result = build(kind, name, parent, &dir, opts, size, &log);
    if let Err(e) = &result {
        let _ = writeln!(&log, "\n{e:#}");
        // Leave nothing half-made behind.
        let _ = fs::remove_dir_all(&dir);
    }
    result.map(|_| dir)
}

fn build(
    kind: &str,
    name: &str,
    parent: &Path,
    dir: &Path,
    opts: &Options,
    size: (u16, u16),
    log: &fs::File,
) -> Result<()> {
    let current = Cell::new("Set up");
    let step = |s: &'static str| {
        current.set(s);
        writeln!(&*log, "==> {s}")
    };
    // Node for the JS steps: the chosen version, else the newest installed.
    let node = opts
        .get("node")
        .cloned()
        .or_else(|| apps::runtime_version("node", parent))
        .map(|v| runtimes::env("node", &v));
    // Runtime environment for the template's tools.
    let mut env = step_env(size);
    if let Some(rt) = apps::runtime(kind) {
        let version = match rt {
            "node" => node.clone(),
            _ => apps::runtime_version(rt, parent).map(|v| runtimes::env(rt, &v)),
        };
        env.extend(version.with_context(|| {
            format!(
                "Install {} in Settings › Languages first",
                runtimes::name(rt)
            )
        })?);
    }
    let sh = |cmd: &mut Command, cwd: &Path| -> Result<()> {
        let status = cmd
            .current_dir(cwd)
            .envs(env.clone())
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log.try_clone()?)
            .status()
            .with_context(|| format!("failed to run {:?}", cmd.get_program()))?;
        if !status.success() {
            bail!("{} failed ({status})", current.get());
        }
        Ok(())
    };
    // node_modules with the chosen manager. A template's npm lock goes when
    // another manager is chosen, so its own lockfile says which one the app uses.
    let js_deps = |manager: &str| -> Result<()> {
        step("JS dependencies")?;
        if !dir.join("package.json").exists() {
            writeln!(&*log, "Nothing to install")?;
            return Ok(());
        }
        if manager != "npm" {
            let _ = fs::remove_file(dir.join("package-lock.json"));
        }
        let Some(node) = &node else {
            writeln!(
                &*log,
                "Skipped: install Node.js in Settings › Languages, then start the app"
            )?;
            return Ok(());
        };
        sh(
            crate::platform::shell(&apps::js_install(manager), None).envs(node.clone()),
            dir,
        )
    };

    match kind {
        "laravel" | "statamic" | "symfony" => {
            let v = opts.get("php").context("Install a PHP version first")?;
            let php_bin = crate::php_bin(v);
            let path = crate::prepend_path(php_bin.parent().unwrap(), &crate::env_path());
            let phar = composer()?;
            let composer = |args: &[&str], cwd: &Path| {
                sh(
                    crate::platform::command(&php_bin)
                        .args(["-d", "memory_limit=-1"])
                        .arg(&phar)
                        .args(args)
                        .args(["--no-interaction", "--ansi"])
                        .env("PATH", &path)
                        .env("COMPOSER_HOME", home().join("cache").join("composer-home"))
                        .env("COMPOSER_CACHE_DIR", home().join("cache").join("composer")),
                    cwd,
                )
            };
            let scripts = || -> serde_json::Value {
                fs::read_to_string(dir.join("composer.json"))
                    .ok()
                    .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
                    .map(|v| v["scripts"].clone())
                    .unwrap_or_default()
            };
            // create-project in steps, so each shows; the scripts run as create-project would.
            step("Download")?;
            let package = composer_package(kind, opts);
            composer(
                &[
                    "create-project",
                    &package,
                    name,
                    "--prefer-dist",
                    "--no-install",
                    "--no-scripts",
                ],
                parent,
            )?;
            step("PHP dependencies")?;
            if !scripts()["post-root-package-install"].is_null() {
                composer(&["run-script", "post-root-package-install"], dir)?;
            }
            composer(&["install"], dir)?;
            js_deps(js(opts))?;
            step("Set up")?;
            if let Some(id) = opts.get("db").and_then(|d| d.strip_prefix("service:")) {
                use_database(name, dir, id, log)?;
            }
            if !scripts()["post-create-project-cmd"].is_null() {
                composer(&["run-script", "post-create-project-cmd"], dir)?;
            }
        }
        "next" | "nuxt" | "vite" => {
            step("Download")?;
            sh(
                crate::platform::command("npx").args(npx_args(kind, name, opts)),
                parent,
            )?;
            js_deps(js(opts))?;
            step("Set up")?;
        }
        "rust" => {
            step("Set up")?;
            sh(
                crate::platform::command("cargo").args(["new", "--vcs", "none", name]),
                parent,
            )?;
            fs::write(dir.join("src/main.rs"), RUST_MAIN)?;
        }
        "go" => {
            step("Set up")?;
            fs::create_dir_all(dir)?;
            fs::write(dir.join("main.go"), GO_MAIN)?;
            sh(
                crate::platform::command("go").args(["mod", "init", name]),
                dir,
            )?;
        }
        "dotnet" => {
            step("Set up")?;
            sh(
                crate::platform::command("dotnet").args(["new", "web", "-o", name]),
                parent,
            )?;
        }
        // Python projects through uv; requirements.txt says what they need.
        // The settings package is `config`, so the folder may be any name (my-site);
        // --no-project keeps uv from treating the parent folder as a project.
        "django" => {
            step("Set up")?;
            fs::create_dir_all(dir)?;
            sh(
                crate::platform::command("uv").args([
                    "run",
                    "--no-project",
                    "--with",
                    "django",
                    "django-admin",
                    "startproject",
                    "config",
                    name,
                ]),
                parent,
            )?;
            let settings = dir.join("config").join("settings.py");
            let mut text = fs::read_to_string(&settings)?;
            let tld = crate::load()
                .map(|c| c.tld)
                .unwrap_or_else(|_| "test".into());
            text.push_str(&django_settings(&tld));
            fs::write(settings, text)?;
            fs::write(dir.join("requirements.txt"), "django\n")?;
        }
        "fastapi" => {
            step("Set up")?;
            fs::create_dir_all(dir)?;
            fs::write(dir.join("main.py"), FASTAPI_MAIN)?;
            fs::write(dir.join("requirements.txt"), "fastapi\nuvicorn\n")?;
        }
        "flask" => {
            step("Set up")?;
            fs::create_dir_all(dir)?;
            fs::write(dir.join("app.py"), FLASK_APP)?;
            fs::write(dir.join("requirements.txt"), "flask\n")?;
        }
        "node" => {
            step("Set up")?;
            fs::create_dir_all(dir)?;
            fs::write(
                dir.join("package.json"),
                format!(
                    "{{\n  \"name\": \"{name}\",\n  \"private\": true,\n  \"scripts\": {{ \"dev\": \"node --watch server.js\", \"start\": \"node server.js\" }}\n}}\n"
                ),
            )?;
            fs::write(dir.join("server.js"), NODE_SERVER)?;
        }
        "php" => {
            step("Set up")?;
            fs::create_dir_all(dir.join("public"))?;
            fs::write(
                dir.join("public/index.php"),
                "<?php\n\necho 'Hello from PHP ' . PHP_VERSION;\n",
            )?;
        }
        _ => {
            step("Set up")?;
            fs::create_dir_all(dir)?;
            fs::write(
                dir.join("index.html"),
                format!(
                    "<!doctype html>\n<meta charset=\"utf-8\">\n<title>{name}</title>\n<h1>{name}</h1>\n"
                ),
            )?;
        }
    }
    if on(opts, "git") && !crate::git_installed() {
        // Windows comes without Git: the app is made all the same.
        writeln!(&*log, "Git isn't installed: no repository made")?;
    } else if on(opts, "git") {
        // A repository with everything in a first commit; a missing git name or
        // email only skips the commit.
        sh(crate::platform::command("git").args(["init", "-q"]), dir)?;
        sh(crate::platform::command("git").args(["add", "-A"]), dir)?;
        if sh(
            crate::platform::command("git").args(["commit", "-q", "-m", "Initial commit"]),
            dir,
        )
        .is_err()
        {
            writeln!(&*log, "Skipped the first commit")?;
        }
    }
    // Last, so a failed creation leaves no settings for an app that isn't there.
    if let Some(v) = opts.get("https") {
        crate::update(|c| {
            choose_https(c, name, opts);
            Ok(())
        })?;
        writeln!(&*log, "HTTPS {v}")?;
    }
    writeln!(&*log, "Done")?;
    Ok(())
}

/// Point a new Laravel app at a database of its name in Anvil's service `id`,
/// created there if it can be; migrations then run against it.
fn use_database(name: &str, dir: &Path, id: &str, log: &fs::File) -> Result<()> {
    let cfg = crate::load()?;
    let db = name.replace(['.', '-'], "_");
    writeln!(&*log, "Using the {db} database in {id}")?;
    if let Err(e) = crate::databases::create(&cfg, id, &db) {
        writeln!(&*log, "{e:#}")?;
    }
    let mut pairs: Vec<(String, String)> = services::env_lines(&cfg, id)
        .lines()
        .filter(|l| l.starts_with("DB_"))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.into(), v.into()))
        .collect();
    pairs.push(("DB_DATABASE".into(), db));
    let env = dir.join(".env");
    let text = fs::read_to_string(&env).unwrap_or_default();
    fs::write(&env, set_env(&text, &pairs))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_simple_templates() {
        let parent = std::env::temp_dir().join(format!("anvil-create-{}", std::process::id()));
        let _ = fs::remove_dir_all(&parent);
        fs::create_dir_all(&parent).unwrap();
        let none = Options::new();
        create("static", "site", &parent, &none, LOG_SIZE).unwrap();
        assert!(parent.join("site/index.html").exists());
        assert!(!parent.join("site/.git").exists());
        create("php", "api", &parent, &none, LOG_SIZE).unwrap();
        assert_eq!(apps::detect(&parent.join("api")), "php");
        assert!(create("php", "api", &parent, &none, LOG_SIZE).is_err()); // exists
        assert!(create("cobol", "x", &parent, &none, LOG_SIZE).is_err());
        let git = Options::from([("git".into(), "on".into())]);
        create("static", "repo", &parent, &git, LOG_SIZE).unwrap();
        assert!(parent.join("repo/.git").is_dir());
        let log = fs::read_to_string(log_file("repo")).unwrap();
        assert!(log.starts_with("==> Set up\n"), "{log}");
        assert!(log.ends_with("Done\n"), "{log}");
        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn options_are_an_allowlist() {
        let cfg = Config::default();
        let keys = |k: &str| -> Vec<&str> { options(k, &cfg).iter().map(|o| o.key).collect() };
        assert!(keys("laravel").starts_with(&["kit", "db", "js"]));
        assert!(keys("next").starts_with(&["ts", "tailwind", "js"]));
        assert_eq!(keys("flask"), ["git", "https"]);
        assert_eq!(keys("static"), ["git", "https"]);
        assert!(keys("laravel").ends_with(&["git", "https"]));
        let full = resolve("next", &cfg, &Options::new()).unwrap();
        assert_eq!(full["ts"], "on");
        assert_eq!(full["js"], "npm");
        assert_eq!(full["git"], "on");
        let bad = |k: &str, v: &str| {
            resolve("next", &cfg, &Options::from([(k.into(), v.into())])).is_err()
        };
        assert!(bad("js", "npm; rm -rf ~"));
        assert!(bad("kit", "react")); // a Laravel option
        assert!(bad("ts", "maybe"));
        assert!(bad("https", "yes"));
        assert!(!bad("https", "on"));
        assert!(!bad("js", "pnpm"));
        // Without services, SQLite is the only database.
        let db = resolve(
            "laravel",
            &cfg,
            &Options::from([("db".into(), "service:mysql".into())]),
        );
        assert!(db.is_err());
    }

    #[test]
    fn flags_follow_the_options() {
        let o = |pairs: &[(&str, &str)]| -> Options {
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        };
        assert_eq!(
            composer_package("laravel", &o(&[("kit", "none")])),
            "laravel/laravel"
        );
        assert_eq!(
            composer_package("laravel", &o(&[("kit", "livewire")])),
            "laravel/livewire-starter-kit"
        );
        assert_eq!(composer_package("statamic", &o(&[])), "statamic/statamic");
        let next = npx_args(
            "next",
            "web",
            &o(&[("ts", "off"), ("tailwind", "on"), ("js", "pnpm")]),
        );
        for flag in [
            "create-next-app@latest",
            "--js",
            "--tailwind",
            "--skip-install",
            "--disable-git",
            "--use-pnpm",
        ] {
            assert!(next.iter().any(|a| a == flag), "{flag} in {next:?}");
        }
        let next = npx_args("next", "web", &o(&[("ts", "on"), ("tailwind", "off")]));
        assert!(next.contains(&"--ts".into()) && next.contains(&"--no-tailwind".into()));
        assert!(next.contains(&"--use-npm".into()));
        let vite = npx_args("vite", "ui", &o(&[("framework", "vue"), ("ts", "on")]));
        assert!(
            vite.ends_with(&["--template".into(), "vue-ts".into()]),
            "{vite:?}"
        );
        let vite = npx_args("vite", "ui", &o(&[("framework", "vanilla"), ("ts", "off")]));
        assert!(
            vite.ends_with(&["--template".into(), "vanilla".into()]),
            "{vite:?}"
        );
        let nuxt = npx_args("nuxt", "n", &o(&[("js", "bun")]));
        assert!(
            nuxt.windows(2)
                .any(|w| w[0] == "--packageManager" && w[1] == "bun")
        );
    }

    #[test]
    fn https_defaults_to_the_setting_and_is_kept_for_the_app() {
        let mut cfg = Config {
            https_default: Some(false),
            ..Default::default()
        };
        assert_eq!(
            resolve("laravel", &cfg, &Options::new()).unwrap()["https"],
            "off"
        );
        cfg.https_default = Some(true);
        assert_eq!(resolve("go", &cfg, &Options::new()).unwrap()["https"], "on");
        let off = Options::from([("https".into(), "off".into())]);
        assert_eq!(resolve("go", &cfg, &off).unwrap()["https"], "off");

        choose_https(&mut cfg, "My-App", &off);
        assert_eq!(cfg.sites["my-app"].https, Some(false));
        choose_https(
            &mut cfg,
            "shop",
            &Options::from([("https".into(), "on".into())]),
        );
        assert_eq!(cfg.sites["shop"].https, Some(true));
        choose_https(&mut cfg, "blog", &Options::new());
        assert!(!cfg.sites.contains_key("blog"));
    }

    #[test]
    fn steps_get_the_log_size() {
        let env = step_env((132, 41));
        let get = |k: &str| {
            env.iter()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(get("COLUMNS"), Some("132"));
        assert_eq!(get("LINES"), Some("41"));
        assert_eq!(get("FORCE_COLOR"), Some("1"));
        // Nonsense from a pane not laid out yet stays usable.
        let env = step_env((0, 0));
        assert!(env.contains(&("COLUMNS".into(), "20".into())));
        assert!(env.contains(&("LINES".into(), "5".into())));
        // A step's process sees them.
        let out = crate::platform::command("/bin/sh")
            .args(["-c", "echo $COLUMNS $LINES"])
            .env_clear()
            .envs(step_env(LOG_SIZE))
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout), "100 30\n");
    }

    #[test]
    fn env_lines_replace_or_append() {
        let text = "APP_NAME=x\nDB_CONNECTION=sqlite\n# DB_HOST=127.0.0.1\n# DB_DATABASE=laravel\n";
        let out = set_env(
            text,
            &[
                ("DB_CONNECTION".into(), "mysql".into()),
                ("DB_HOST".into(), "127.0.0.1".into()),
                ("DB_DATABASE".into(), "my_app".into()),
                ("DB_PORT".into(), "3307".into()),
            ],
        );
        assert_eq!(
            out,
            "APP_NAME=x\nDB_CONNECTION=mysql\nDB_HOST=127.0.0.1\nDB_DATABASE=my_app\nDB_PORT=3307\n"
        );
    }
}
