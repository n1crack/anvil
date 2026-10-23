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
pub const TEMPLATES: [&str; 16] = [
    "laravel",
    "statamic",
    "symfony",
    "wordpress",
    "php",
    "static",
    "next",
    "nuxt",
    "vite",
    "node",
    "rust",
    "go",
    "dotnet",
    "django",
    "fastapi",
    "flask",
];

/// WordPress's latest release; it publishes a SHA-1 (and an MD5) beside it.
const WORDPRESS: &str = "https://wordpress.org/latest.tar.gz";

/// Prints WordPress's eight keys and salts, a line each: 64 characters from its own
/// generator's set less `'` and `\`, so each fits a single-quoted PHP string as it is.
const SALTS_PHP: &str = r#"$c = 'abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!@#$%^&*()-_[]{}<>~`+=,.;:/?|';
for ($k = 0; $k < 8; $k++) { $s = ''; for ($i = 0; $i < 64; $i++) $s .= $c[random_int(0, strlen($c) - 1)]; echo $s, PHP_EOL; }"#;

const SALT_NAMES: [&str; 8] = [
    "AUTH_KEY",
    "SECURE_AUTH_KEY",
    "LOGGED_IN_KEY",
    "NONCE_KEY",
    "AUTH_SALT",
    "SECURE_AUTH_SALT",
    "LOGGED_IN_SALT",
    "NONCE_SALT",
];

/// A new WordPress site's wp-config.php: its database from `.env` (or the environment,
/// which Anvil sets for a worktree), so a worktree of it gets a database of its own; its
/// address the one it is opened at; `salts` (8 lines) as its keys.
fn wp_config(salts: &str) -> String {
    let keys: String = SALT_NAMES
        .iter()
        .zip(salts.lines())
        .map(|(k, v)| format!("define( '{k}', '{v}' );\n"))
        .collect();
    format!(
        r#"<?php
/**
 * WordPress's settings, as Anvil wrote them. The database comes from .env, or from the
 * environment, which wins: Anvil sets it for a git worktree, so the worktree uses a
 * database of its own.
 */

function anvil_env( $key, $default = '' ) {{
	static $file = null;
	if ( null === $file ) {{
		$file  = array();
		$lines = is_readable( __DIR__ . '/.env' ) ? file( __DIR__ . '/.env', FILE_IGNORE_NEW_LINES ) : array();
		foreach ( $lines as $line ) {{
			$pair = explode( '=', $line, 2 );
			if ( 2 === count( $pair ) && '#' !== substr( ltrim( $line ), 0, 1 ) ) {{
				$file[ trim( $pair[0] ) ] = trim( trim( $pair[1] ), '"\'' );
			}}
		}}
	}}
	$value = getenv( $key );
	if ( false === $value && isset( $_SERVER[ $key ] ) ) {{
		$value = $_SERVER[ $key ];
	}}
	return false !== $value ? $value : ( isset( $file[ $key ] ) ? $file[ $key ] : $default );
}}

define( 'DB_NAME', anvil_env( 'DB_DATABASE', 'wordpress' ) );
define( 'DB_USER', anvil_env( 'DB_USERNAME', 'root' ) );
define( 'DB_PASSWORD', anvil_env( 'DB_PASSWORD' ) );
define( 'DB_HOST', anvil_env( 'DB_HOST', '127.0.0.1' ) . ':' . anvil_env( 'DB_PORT', '3306' ) );
define( 'DB_CHARSET', 'utf8mb4' );
define( 'DB_COLLATE', '' );

{keys}
$table_prefix = 'wp_';

define( 'WP_ENVIRONMENT_TYPE', 'local' );
define( 'WP_DEBUG', false );

// WordPress keeps its address in the database; here it follows the one it is opened at
// (HTTPS on or off, an alias, a worktree's own address).
if ( isset( $_SERVER['HTTP_HOST'] ) ) {{
	$anvil_url = ( isset( $_SERVER['HTTPS'] ) && 'off' !== $_SERVER['HTTPS'] ? 'https' : 'http' ) . '://' . $_SERVER['HTTP_HOST'];
	define( 'WP_HOME', $anvil_url );
	define( 'WP_SITEURL', $anvil_url );
}}

if ( ! defined( 'ABSPATH' ) ) {{
	define( 'ABSPATH', __DIR__ . '/' );
}}

require_once ABSPATH . 'wp-settings.php';
"#
    )
}

/// A must-use plugin sending WordPress's mail (PHP's mail() otherwise, which goes
/// nowhere here) to Anvil's mail catcher while it catches the site's: the `MAIL_*` Anvil
/// passes PHP then.
const WP_MAIL_PLUGIN: &str = r#"<?php
/**
 * Plugin Name: Anvil mail
 * Description: Sends mail to Anvil's mail catcher while it catches this site's mail.
 */

add_action(
	'phpmailer_init',
	function ( $mailer ) {
		$get = function ( $key ) {
			$value = getenv( $key );
			return false !== $value ? $value : ( isset( $_SERVER[ $key ] ) ? $_SERVER[ $key ] : '' );
		};
		if ( '' === $get( 'MAIL_HOST' ) ) {
			return;
		}
		$mailer->isSMTP();
		$mailer->Host        = $get( 'MAIL_HOST' );
		$mailer->Port        = (int) $get( 'MAIL_PORT' );
		$mailer->SMTPAuth    = '' !== $get( 'MAIL_USERNAME' );
		$mailer->Username    = $get( 'MAIL_USERNAME' );
		$mailer->Password    = $get( 'MAIL_PASSWORD' );
		$mailer->SMTPSecure  = '';
		$mailer->SMTPAutoTLS = false;
	}
);
"#;

/// What git leaves out of a new WordPress site: its database settings and uploads.
const WP_GITIGNORE: &str = ".env\n/wp-content/uploads/\n/wp-content/upgrade/\n";

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
/// Anvil's HTTPS proxy, and only in debug. The tld is the app's `ANVIL_TLD` when it
/// starts, so a later `anvil tld` needs no edit here.
const DJANGO_SETTINGS: &str = "\n\n# Anvil serves this app over HTTPS at its local address.\nif DEBUG:\n    import os\n\n    ALLOWED_HOSTS += [\".\" + os.environ.get(\"ANVIL_TLD\", \"test\"), \"localhost\", \"127.0.0.1\"]\n    SECURE_PROXY_SSL_HEADER = (\"HTTP_X_FORWARDED_PROTO\", \"https\")\n";

/// What each template needs installed first: a PHP version or a runtime id.
pub fn needs(kind: &str) -> Option<&'static str> {
    match kind {
        "laravel" | "statamic" | "symfony" | "wordpress" => Some("php"),
        k => apps::runtime(k),
    }
}

/// `needs(kind)` when it isn't installed for an app in `parent` with `opts`: what
/// `create` refuses on before it starts, so the New sheet can offer installing it.
/// `mysql` for WordPress without a MySQL service of Anvil's to make its database in.
pub fn missing(kind: &str, parent: &Path, opts: &Options) -> Option<&'static str> {
    let need = needs(kind)?;
    let has = match need {
        "php" => opts.contains_key("php"),
        "node" => opts.contains_key("node") || apps::runtime_version("node", parent).is_some(),
        rt => apps::runtime_version(rt, parent).is_some(),
    };
    if !has {
        return Some(need);
    }
    (kind == "wordpress" && !opts.contains_key("db")).then_some("mysql")
}

/// Anvil's services of `kinds`, as the Database choice offers them: `service:<id>`.
fn db_services(cfg: &Config, kinds: &[&str]) -> Vec<(String, String)> {
    services::instances(cfg)
        .into_iter()
        .filter_map(|id| {
            let kind = services::kind_of(cfg, &id)
                .ok()
                .filter(|k| kinds.contains(k))?;
            let name = services::name(kind);
            let label = if id == kind {
                name.to_string()
            } else {
                format!("{name} · {id}")
            };
            Some((format!("service:{id}"), label))
        })
        .collect()
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

/// What became of the repository `opts` asked for in the new app at `dir`, when it isn't
/// one with a first commit: the CLI says so, the app's window shows the log.
pub fn git_note(name: &str, dir: &Path, opts: &Options) -> Option<String> {
    if !on(opts, "git") {
        return None;
    }
    if !crate::git_installed() {
        return Some("Git isn't installed, so the app has no repository.".into());
    }
    let git = |args: &[&str]| {
        crate::platform::command("git")
            .args(args)
            .current_dir(dir)
            .stdin(Stdio::null())
            .output()
            .ok()
            .filter(|o| o.status.success() && !o.stdout.trim_ascii().is_empty())
    };
    if git(&["rev-parse", "-q", "--verify", "HEAD"]).is_some() {
        return None;
    }
    Some(
        if git(&["config", "user.name"]).is_none() || git(&["config", "user.email"]).is_none() {
            "The repository has no first commit: Git has no name or email yet. Set them with `git config --global user.name \"Your Name\"` and `git config --global user.email you@example.com`, then commit.".into()
        } else {
            format!(
                "The repository has no first commit: `git commit` failed, see {}.",
                log_file(name).display()
            )
        },
    )
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
            db.choices.extend(db_services(cfg, &["mysql", "postgres"]));
            v.push(db);
        }
        // MySQL only: WordPress has no other database without a plugin. None at all
        // leaves the option out, and `missing` says so.
        "wordpress" => {
            let choices = db_services(cfg, &["mysql"]);
            if let Some((first, _)) = choices.first() {
                v.push(Opt {
                    key: "db",
                    label: "Database",
                    default: first.clone(),
                    choices,
                });
            }
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
            &[("pnpm", "pnpm"), ("npm", "npm"), ("bun", "bun")],
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
    opts.get("js").map_or("pnpm", String::as_str)
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
pub(crate) fn set_env(text: &str, pairs: &[(String, String)]) -> String {
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
    match missing(kind, parent, opts) {
        Some("php") => bail!("Install a PHP version first"),
        Some("mysql") => bail!("Add a MySQL service first, in Services"),
        Some(rt) => bail!("Install {0} in Settings › {0} first", runtimes::name(rt)),
        None => {}
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
        env.extend(
            version.with_context(|| {
                format!("Install {0} in Settings › {0} first", runtimes::name(rt))
            })?,
        );
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
                "Skipped: install Node.js in Settings › Node.js, then start the app"
            )?;
            return Ok(());
        };
        if manager == "pnpm" {
            runtimes::pnpm_ready(node)?;
        }
        // A new app is no worktree: its node_modules are its own (`apps::shared_deps`).
        sh(
            crate::platform::shell(&apps::js_install(manager, false), None).envs(node.clone()),
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
        // WordPress's own release, checked against its published SHA-1. Its install screen
        // is the first page: the site's title and admin account are WordPress's to ask.
        "wordpress" => {
            let v = opts.get("php").context("Install a PHP version first")?;
            let id = opts
                .get("db")
                .and_then(|d| d.strip_prefix("service:"))
                .context("Add a MySQL service first, in Services")?;
            if !crate::opcache::has_mysqli(v) {
                step("PHP’s mysqli")?;
                writeln!(
                    &*log,
                    "WordPress reaches MySQL through mysqli, which Anvil's PHP {v} comes without: building it, once"
                )?;
                crate::opcache::build_mysqli(v)?;
                writeln!(&*log, "mysqli built for PHP {v}")?;
            }
            step("Download")?;
            let sum = published_sum(&format!("{WORDPRESS}.sha1"), "latest.tar.gz")?;
            crate::fetch_tgz(WORDPRESS, dir, true, Some(&sum))?;
            writeln!(&*log, "WordPress {} (SHA-1 checked)", wp_version(dir))?;
            step("Set up")?;
            let pairs = database_env(name, id, log)?;
            fs::write(dir.join(".env"), set_env("", &pairs))?;
            let salts = crate::platform::command(crate::php_bin(v))
                .args(["-n", "-r", SALTS_PHP])
                .output()
                .context("failed to run PHP")?;
            let salts = String::from_utf8_lossy(&salts.stdout);
            if salts.lines().filter(|l| l.len() == 64).count() != 8 {
                bail!("PHP {v} made no keys for wp-config.php");
            }
            fs::write(dir.join("wp-config.php"), wp_config(&salts))?;
            let mu = dir.join("wp-content").join("mu-plugins");
            fs::create_dir_all(&mu)?;
            fs::write(mu.join("anvil-mail.php"), WP_MAIL_PLUGIN)?;
            fs::write(dir.join(".gitignore"), WP_GITIGNORE)?;
            writeln!(&*log, "Open the site to finish WordPress's install")?;
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
            text.push_str(DJANGO_SETTINGS);
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
    let pairs = database_env(name, id, log)?;
    let env = dir.join(".env");
    let text = fs::read_to_string(&env).unwrap_or_default();
    fs::write(&env, set_env(&text, &pairs))?;
    Ok(())
}

/// A database of the new app's name in Anvil's service `id`, created there if it can be,
/// and the `DB_*` lines that reach it.
fn database_env(name: &str, id: &str, log: &fs::File) -> Result<Vec<(String, String)>> {
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
    Ok(pairs)
}

/// The WordPress release in `dir`, as wp-includes/version.php names it.
fn wp_version(dir: &Path) -> String {
    let text = fs::read_to_string(dir.join("wp-includes/version.php")).unwrap_or_default();
    text.lines()
        .find_map(|l| l.trim().strip_prefix("$wp_version = '")?.split('\'').next())
        .unwrap_or("")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_simple_templates() {
        // The log is under ANVIL_HOME: another test may point it elsewhere meanwhile.
        let _env = crate::lock(&crate::ENV_LOCK);
        let home = tempfile::tempdir().unwrap();
        // SAFETY: every test that sets ANVIL_HOME holds ENV_LOCK.
        unsafe { std::env::set_var("ANVIL_HOME", home.path()) };
        let parent = home.path().join("apps");
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
        // A missing PHP or runtime is refused before there's a log: a form error, not a failed build.
        assert_eq!(missing("laravel", &parent, &none), Some("php"));
        let php = Options::from([("php".into(), "8.4".into())]);
        assert_eq!(missing("laravel", &parent, &php), None);
        assert_eq!(missing("go", &parent, &none), Some("go"));
        assert_eq!(missing("static", &parent, &none), None);
        let err = create("go", "svc", &parent, &none, LOG_SIZE).unwrap_err();
        assert_eq!(err.to_string(), "Install Go in Settings › Go first");
        assert!(!log_file("svc").exists());
        unsafe { std::env::remove_var("ANVIL_HOME") };
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
        assert_eq!(full["js"], "pnpm");
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
        assert!(
            next.contains(&"--use-pnpm".into()),
            "pnpm unless chosen otherwise"
        );
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
    fn wordpress_needs_a_mysql_service() {
        let mut cfg = Config::default();
        let parent = Path::new("/nowhere");
        let php = Options::from([("php".into(), "8.4".into())]);
        // No MySQL of Anvil's: no Database choice, and the form says what's missing.
        assert!(!options("wordpress", &cfg).iter().any(|o| o.key == "db"));
        assert_eq!(missing("wordpress", parent, &Options::new()), Some("php"));
        assert_eq!(missing("wordpress", parent, &php), Some("mysql"));
        for (id, kind) in [("pg", "postgres"), ("mysql", "mysql"), ("mysql-2", "mysql")] {
            cfg.services.insert(
                id.into(),
                services::ServiceConfig {
                    kind: Some(kind.into()),
                    ..Default::default()
                },
            );
        }
        let db = options("wordpress", &cfg)
            .into_iter()
            .find(|o| o.key == "db")
            .unwrap();
        let ids: Vec<&str> = db.choices.iter().map(|c| c.0.as_str()).collect();
        assert_eq!(ids, ["service:mysql", "service:mysql-2"], "MySQL only");
        assert_eq!(db.default, "service:mysql");
        // (No PHP here to offer: the chosen one as the sheet would send it.)
        let mut full = resolve("wordpress", &cfg, &Options::new()).unwrap();
        full.extend(php);
        assert_eq!(missing("wordpress", parent, &full), None);
        let pg = Options::from([("db".into(), "service:pg".into())]);
        assert!(resolve("wordpress", &cfg, &pg).is_err());
    }

    #[test]
    fn wp_config_reads_its_database_from_the_env() {
        let salts: String = (0..8)
            .map(|i| format!("{}\n", i.to_string().repeat(64)))
            .collect();
        let text = wp_config(&salts);
        assert!(text.starts_with("<?php\n"));
        assert!(text.contains("define( 'DB_NAME', anvil_env( 'DB_DATABASE', 'wordpress' ) );"));
        assert!(text.contains(
            "anvil_env( 'DB_HOST', '127.0.0.1' ) . ':' . anvil_env( 'DB_PORT', '3306' )"
        ));
        assert!(text.contains(&format!("define( 'AUTH_KEY', '{}' );", "0".repeat(64))));
        assert!(text.contains(&format!("define( 'NONCE_SALT', '{}' );", "7".repeat(64))));
        assert!(text.contains("require_once ABSPATH . 'wp-settings.php';"));
        // What the worktree check looks for: it reads DB_DATABASE.
        assert!(text.contains("DB_DATABASE"));
        // The salt alphabet leaves out what would end or escape a PHP string.
        let set = SALTS_PHP.split('\'').nth(1).unwrap();
        assert!(!set.contains('\\') && set.len() > 80, "{set}");
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("wp-includes")).unwrap();
        fs::write(
            dir.path().join("wp-includes/version.php"),
            "<?php\n/**\n * @global string $wp_version\n */\n$wp_version = '6.8.3';\n",
        )
        .unwrap();
        assert_eq!(wp_version(dir.path()), "6.8.3");
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
