//! Create a new application from a template, using Anvil's own PHP, Composer and
//! language toolchains so nothing is needed on the system.

use crate::{apps, home, run, runtimes};
use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Kinds that can be created, in the order the dialog shows them.
pub const TEMPLATES: [&str; 12] = [
    "laravel", "statamic", "symfony", "php", "static", "next", "nuxt", "vite", "node", "rust",
    "go", "dotnet",
];

/// What each template needs installed first: a PHP version or a runtime id.
pub fn needs(kind: &str) -> Option<&'static str> {
    match kind {
        "laravel" | "statamic" | "symfony" => Some("php"),
        k => apps::runtime(k),
    }
}

fn composer() -> Result<PathBuf> {
    let phar = home().join("bin").join("composer.phar");
    if !phar.exists() {
        fs::create_dir_all(phar.parent().unwrap())?;
        run(Command::new("curl")
            .args(["-fsSL", "-o"])
            .arg(&phar)
            .arg("https://getcomposer.org/download/latest-stable/composer.phar"))?;
    }
    Ok(phar)
}

fn check_name(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && !name.starts_with(['.', '-'])
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.');
    if !ok {
        bail!("Use lowercase letters, numbers, dots and dashes for the name");
    }
    Ok(())
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

/// Create `<parent>/<name>` as a `kind` project. Output goes to `log_file(name)`.
pub fn create(kind: &str, name: &str, parent: &Path, php: Option<&str>) -> Result<PathBuf> {
    check_name(name)?;
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

    // Runtime environment for the template's tools.
    let mut env: Vec<(String, String)> = vec![];
    if let Some(rt) = apps::runtime(kind) {
        let version = apps::runtime_version(rt, parent).with_context(|| {
            format!(
                "Install {} in Settings › Languages first",
                runtimes::name(rt)
            )
        })?;
        env = runtimes::env(rt, &version);
    }
    let sh = |cmd: &mut Command| -> Result<()> {
        let status = cmd
            .current_dir(parent)
            .envs(env.clone())
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log.try_clone()?)
            .status()
            .with_context(|| format!("failed to run {cmd:?}"))?;
        if !status.success() {
            bail!("Creating {name} failed — see {}", log_file(name).display());
        }
        Ok(())
    };

    let result = match kind {
        "laravel" | "statamic" | "symfony" => {
            let v = php.context("Install a PHP version first")?;
            let php_bin = home().join("php").join(v).join("php");
            let path = format!(
                "{}:{}",
                php_bin.parent().unwrap().display(),
                std::env::var("PATH").unwrap_or_default()
            );
            let package = match kind {
                "laravel" => "laravel/laravel",
                "statamic" => "statamic/statamic",
                _ => "symfony/skeleton",
            };
            sh(Command::new(&php_bin)
                .args(["-d", "memory_limit=-1"])
                .arg(composer()?)
                .args([
                    "create-project",
                    package,
                    name,
                    "--no-interaction",
                    "--prefer-dist",
                ])
                .env("PATH", path)
                .env("COMPOSER_HOME", home().join("cache").join("composer-home"))
                .env("COMPOSER_CACHE_DIR", home().join("cache").join("composer")))
        }
        "next" => sh(Command::new("npx").args([
            "--yes",
            "create-next-app@latest",
            name,
            "--yes",
            "--use-npm",
            "--skip-install",
        ])),
        "nuxt" => sh(Command::new("npx").args([
            "--yes",
            "nuxi@latest",
            "init",
            name,
            "--packageManager",
            "npm",
            "--gitInit",
            "false",
            "--no-install",
            "--template",
            "minimal",
        ])),
        "vite" => sh(Command::new("npx").args([
            "--yes",
            "create-vite@latest",
            name,
            "--template",
            "react-ts",
            "--no-interactive",
        ])),
        "rust" => sh(Command::new("cargo").args(["new", "--vcs", "none", name]))
            .and_then(|_| Ok(fs::write(dir.join("src/main.rs"), RUST_MAIN)?)),
        "go" => {
            fs::create_dir_all(&dir)?;
            fs::write(dir.join("main.go"), GO_MAIN)?;
            // `-C` because `sh` runs commands from the parent folder.
            sh(Command::new("go")
                .arg("-C")
                .arg(&dir)
                .args(["mod", "init", name]))
        }
        "dotnet" => sh(Command::new("dotnet").args(["new", "web", "-o", name])),
        "node" => {
            fs::create_dir_all(&dir)?;
            fs::write(
                dir.join("package.json"),
                format!(
                    "{{\n  \"name\": \"{name}\",\n  \"private\": true,\n  \"scripts\": {{ \"dev\": \"node --watch server.js\", \"start\": \"node server.js\" }}\n}}\n"
                ),
            )?;
            Ok(fs::write(dir.join("server.js"), NODE_SERVER)?)
        }
        "php" => {
            fs::create_dir_all(dir.join("public"))?;
            Ok(fs::write(
                dir.join("public/index.php"),
                "<?php\n\necho 'Hello from PHP ' . PHP_VERSION;\n",
            )?)
        }
        _ => {
            fs::create_dir_all(&dir)?;
            Ok(fs::write(
                dir.join("index.html"),
                format!(
                    "<!doctype html>\n<meta charset=\"utf-8\">\n<title>{name}</title>\n<h1>{name}</h1>\n"
                ),
            )?)
        }
    };
    if let Err(e) = result {
        // Leave nothing half-made behind.
        let _ = fs::remove_dir_all(&dir);
        return Err(e);
    }
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_simple_templates() {
        assert!(check_name("my-app").is_ok());
        assert!(check_name("shop.be").is_ok());
        for bad in ["", "../x", "My App", ".hidden", "a/b"] {
            assert!(check_name(bad).is_err(), "{bad}");
        }
        let parent = std::env::temp_dir().join(format!("anvil-create-{}", std::process::id()));
        let _ = fs::remove_dir_all(&parent);
        fs::create_dir_all(&parent).unwrap();
        create("static", "site", &parent, None).unwrap();
        assert!(parent.join("site/index.html").exists());
        create("php", "api", &parent, None).unwrap();
        assert_eq!(apps::detect(&parent.join("api")), "php");
        assert!(create("php", "api", &parent, None).is_err()); // exists
        assert!(create("cobol", "x", &parent, None).is_err());
        fs::remove_dir_all(parent).unwrap();
    }
}
