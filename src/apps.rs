//! What kind of application a folder is, and running apps that bring their own
//! server (Node, Rust, Go, .NET): Anvil starts them on a private port and Caddy
//! proxies `<name>.<tld>` to it.

use crate::{Config, Site, home, kill_orphans, runtimes};
use anyhow::{Context, Result, bail};
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

/// Every kind Anvil knows, in detection order.
pub const KINDS: [&str; 15] = [
    "laravel", "statamic", "symfony", "php", "next", "nuxt", "vite", "node", "rust", "go",
    "dotnet", "django", "fastapi", "flask", "static",
];

pub fn label(kind: &str) -> &'static str {
    match kind {
        "laravel" => "Laravel",
        "statamic" => "Statamic",
        "symfony" => "Symfony",
        "php" => "PHP",
        "next" => "Next.js",
        "nuxt" => "Nuxt",
        "vite" => "Vite",
        "node" => "Node.js",
        "rust" => "Rust",
        "go" => "Go",
        "dotnet" => ".NET",
        "django" => "Django",
        "fastapi" => "FastAPI",
        "flask" => "Flask",
        _ => "Static",
    }
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

/// Recognise a project from the files in its folder.
pub fn detect(dir: &Path) -> &'static str {
    let has = |f: &str| dir.join(f).exists();
    if has("artisan") {
        return if read(&dir.join("composer.json")).contains("\"statamic/cms\"") {
            "statamic"
        } else {
            "laravel"
        };
    }
    if has("bin/console")
        && read(&dir.join("composer.json")).contains("\"symfony/framework-bundle\"")
    {
        return "symfony";
    }
    if has("composer.json") || has("index.php") || has("public/index.php") {
        return "php";
    }
    if has("Cargo.toml") {
        return "rust";
    }
    if has("go.mod") {
        return "go";
    }
    let dotnet = fs::read_dir(dir).into_iter().flatten().flatten().any(|e| {
        let n = e.file_name().to_string_lossy().into_owned();
        n.ends_with(".csproj") || n.ends_with(".fsproj") || n.ends_with(".sln")
    });
    if dotnet {
        return "dotnet";
    }
    // Python before package.json: Django and friends often carry a frontend too.
    if has("manage.py") && read(&dir.join("manage.py")).contains("django") {
        return "django";
    }
    let py = format!(
        "{}{}",
        read(&dir.join("pyproject.toml")),
        read(&dir.join("requirements.txt"))
    )
    .to_lowercase();
    if py.contains("fastapi") {
        return "fastapi";
    }
    if py.contains("flask") {
        return "flask";
    }
    if has("package.json") {
        let pkg = read(&dir.join("package.json"));
        return if pkg.contains("\"next\"") {
            "next"
        } else if pkg.contains("\"nuxt\"") {
            "nuxt"
        } else if pkg.contains("\"vite\"") {
            "vite"
        } else {
            "node"
        };
    }
    "static"
}

/// PHP kinds go through PHP-FPM; these run their own server behind the proxy.
pub fn runs_process(kind: &str) -> bool {
    runtime(kind).is_some()
}

pub fn runtime(kind: &str) -> Option<&'static str> {
    match kind {
        "next" | "nuxt" | "vite" | "node" => Some("node"),
        "rust" => Some("rust"),
        "go" => Some("go"),
        "dotnet" => Some("dotnet"),
        "django" | "fastapi" | "flask" => Some("python"),
        _ => None,
    }
}

pub fn is_php(kind: &str) -> bool {
    matches!(kind, "laravel" | "statamic" | "symfony" | "php")
}

/// The command Anvil runs when the app has no custom one. `$PORT` is set.
pub fn default_command(kind: &str, dir: &Path) -> Option<String> {
    // Stop if the install fails, so its error is the last thing in the log.
    let npm =
        |script: &str| format!("{{ [ -d node_modules ] || npm install; }} && npm run {script}");
    Some(match kind {
        "next" | "nuxt" => npm("dev"),
        // Vite ignores $PORT, so pass it explicitly.
        "vite" => npm("dev -- --port $PORT --strictPort --host 127.0.0.1"),
        "node" => {
            let pkg = read(&dir.join("package.json"));
            if pkg.contains("\"dev\"") {
                npm("dev")
            } else {
                npm("start")
            }
        }
        "rust" => "cargo run".into(),
        "go" => "go run .".into(),
        "dotnet" => "dotnet run --urls http://127.0.0.1:$PORT".into(),
        "django" => uv_run(dir, "django", "python manage.py runserver 127.0.0.1:$PORT"),
        "fastapi" => uv_run(
            dir,
            "fastapi",
            &format!(
                "--with uvicorn uvicorn {}:app --host 127.0.0.1 --port $PORT --reload",
                asgi_module(dir)
            ),
        ),
        // Flask finds app.py or wsgi.py by itself.
        "flask" => uv_run(
            dir,
            "flask",
            "flask run --host 127.0.0.1 --port $PORT --debug",
        ),
        _ => return None,
    })
}

/// `uv run` with the project's dependencies: a pyproject.toml project gets its
/// own environment; requirements.txt goes into uv's cache, not the project; a
/// project declaring neither gets just its framework.
fn uv_run(dir: &Path, framework: &str, cmd: &str) -> String {
    if dir.join("pyproject.toml").exists() {
        format!("uv run {cmd}")
    } else if dir.join("requirements.txt").exists() {
        format!("uv run --with-requirements requirements.txt {cmd}")
    } else {
        format!("uv run --with {framework} {cmd}")
    }
}

/// Where a FastAPI app's `app` lives: main.py, app/main.py or app.py.
fn asgi_module(dir: &Path) -> &'static str {
    if dir.join("main.py").exists() {
        "main"
    } else if dir.join("app/main.py").exists() {
        "app.main"
    } else if dir.join("app.py").exists() {
        "app"
    } else {
        "main"
    }
}

/// Pick the runtime version for a project: `.nvmrc` / `.node-version` for Node,
/// otherwise the newest installed.
pub fn runtime_version(runtime: &str, dir: &Path) -> Option<String> {
    let installed = runtimes::installed(runtime);
    if runtime == "node" {
        for f in [".nvmrc", ".node-version"] {
            let want = read(&dir.join(f));
            let major = want
                .trim()
                .trim_start_matches('v')
                .split('.')
                .next()
                .unwrap_or("")
                .to_string();
            if installed.contains(&major) {
                return Some(major);
            }
        }
    }
    // uv's .python-version, e.g. 3.13 or 3.13.5.
    if runtime == "python" {
        let want: String = read(&dir.join(".python-version"))
            .trim()
            .split('.')
            .take(2)
            .collect::<Vec<_>>()
            .join(".");
        if installed.contains(&want) {
            return Some(want);
        }
    }
    installed.into_iter().next()
}

/// A stable private port per app: the configured one, or one derived from the
/// name, moved past ports other apps already took.
pub fn assign_ports(cfg: &Config, sites: &mut [Site]) {
    let mut taken: Vec<u16> = sites.iter().filter_map(|s| s.port).collect();
    for s in sites
        .iter_mut()
        .filter(|s| runs_process(&s.kind) && s.port.is_none())
    {
        // ponytail: hash-derived ports can shift if a new app collides first; set a port in the app to pin it.
        let hash = s
            .name
            .bytes()
            .fold(5381u32, |h, b| h.wrapping_mul(33) ^ u32::from(b));
        let mut port = 41000 + (hash % 4000) as u16;
        while taken.contains(&port) || cfg.https_port == port || cfg.http_port == port {
            port += 1;
        }
        taken.push(port);
        s.port = Some(port);
    }
}

pub fn log_file(name: &str) -> PathBuf {
    home().join("log").join(format!("app-{name}.log"))
}

fn pid_file(name: &str) -> PathBuf {
    home().join("run").join(format!("app-{name}.pids"))
}

/// Whether any Anvil (the app or `anvil start`) is running this app right now.
pub fn running_anywhere(name: &str) -> bool {
    fs::read_to_string(pid_file(name)).is_ok_and(|f| f.lines().skip(1).any(crate::alive))
}

/// Stop a whole process group: `npm run dev` leaves node children behind otherwise.
fn terminate_group(child: &mut Child, grace: Duration) {
    let group = format!("-{}", child.id());
    let _ = Command::new("kill").args(["-TERM", "--", &group]).status();
    let deadline = std::time::Instant::now() + grace;
    while std::time::Instant::now() < deadline {
        if let Ok(Some(_)) = child.try_wait() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    // Anything in the group still alive gets killed.
    let _ = Command::new("kill").args(["-KILL", "--", &group]).status();
    let _ = child.wait();
}

/// Apps started from Anvil, by site name.
#[derive(Default)]
pub struct Apps {
    children: BTreeMap<String, Child>,
}

impl Apps {
    pub fn running(&mut self, name: &str) -> bool {
        if let Some(c) = self.children.get_mut(name)
            && !matches!(c.try_wait(), Ok(None))
        {
            self.children.remove(name);
        }
        self.children.contains_key(name)
    }

    /// `tld` lets dev servers that check the Host header (Vite, Nuxt) accept `<name>.<tld>`.
    pub fn start(&mut self, site: &Site, tld: &str) -> Result<()> {
        if self.running(&site.name) {
            return Ok(());
        }
        let runtime = runtime(&site.kind)
            .with_context(|| format!("{} apps are served by Anvil directly", label(&site.kind)))?;
        let version = site.runtime_version.clone().with_context(|| {
            format!(
                "{} isn't installed in Anvil yet — add it in Settings › Languages",
                runtimes::name(runtime)
            )
        })?;
        let command = site
            .command
            .clone()
            .with_context(|| format!("No command to start {}", site.name))?;
        let port = site.port.context("No port assigned")?;

        kill_orphans(&pid_file(&site.name), true);
        crate::state_dirs()?;
        let log = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_file(&site.name))?;
        // The trailing argument is $0: it tags the shell with Anvil's home so a
        // crashed Anvil's leftovers can be recognised and cleaned up.
        let tag = format!("anvil:{}:{}", home().display(), site.name);
        let child = Command::new("/bin/sh")
            .args(["-c", &command, &tag])
            .current_dir(&site.path)
            .envs(runtimes::env(runtime, &version))
            .env("PORT", port.to_string())
            // Best effort: most dev servers honour one of these and stay off the LAN.
            .env("HOST", "127.0.0.1")
            .env("HOSTNAME", "127.0.0.1")
            .env("ASPNETCORE_URLS", format!("http://127.0.0.1:{port}"))
            // Vite rejects unknown hosts; a leading dot allows every name under the tld.
            .env("__VITE_ADDITIONAL_SERVER_ALLOWED_HOSTS", format!(".{tld}"))
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log)
            .process_group(0)
            .spawn()
            .with_context(|| format!("failed to start {}", site.name))?;
        fs::write(
            pid_file(&site.name),
            format!("{}\n{}", std::process::id(), child.id()),
        )?;
        self.children.insert(site.name.clone(), child);
        Ok(())
    }

    /// Process-group leader of a running app.
    pub fn pid(&mut self, name: &str) -> Option<u32> {
        self.running(name).then(|| self.children[name].id())
    }

    pub fn stop(&mut self, name: &str) {
        if let Some(mut c) = self.children.remove(name) {
            terminate_group(&mut c, Duration::from_secs(5));
            let _ = fs::remove_file(pid_file(name));
        }
    }

    pub fn names(&mut self) -> Vec<String> {
        let names: Vec<String> = self.children.keys().cloned().collect();
        names.into_iter().filter(|n| self.running(n)).collect()
    }
}

impl Drop for Apps {
    fn drop(&mut self) {
        let names: Vec<String> = self.children.keys().cloned().collect();
        for n in names {
            self.stop(&n);
        }
    }
}

/// Validate a custom start command before it is saved.
pub fn check_command(command: &str) -> Result<()> {
    if command.trim().is_empty() || command.contains('\0') {
        bail!("The start command can't be empty");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(files: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "anvil-detect-{}-{}",
            std::process::id(),
            files.first().map_or("empty", |f| f.0).replace('/', "_")
        ));
        let _ = fs::remove_dir_all(&dir);
        for (f, body) in files {
            let p = dir.join(f);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, body).unwrap();
        }
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn detects_kinds() {
        let cases: [(&[(&str, &str)], &str); 14] = [
            (
                &[("artisan", ""), ("package.json", "{\"vite\":1}")],
                "laravel",
            ),
            (
                &[("artisan", ""), ("composer.json", "{\"statamic/cms\":1}")],
                "statamic",
            ),
            (
                &[
                    ("bin/console", ""),
                    ("composer.json", "{\"symfony/framework-bundle\":1}"),
                ],
                "symfony",
            ),
            (&[("public/index.php", "")], "php"),
            (&[("Cargo.toml", "")], "rust"),
            (&[("go.mod", "")], "go"),
            (&[("Api.csproj", "")], "dotnet"),
            (
                &[("package.json", "{\"dependencies\":{\"next\":\"15\"}}")],
                "next",
            ),
            (
                &[("package.json", "{\"scripts\":{\"start\":\"node .\"}}")],
                "node",
            ),
            (&[("index.html", "")], "static"),
            // Django with a frontend is still Django.
            (
                &[
                    ("manage.py", "from django.core.management import x"),
                    ("package.json", "{\"vite\":1}"),
                ],
                "django",
            ),
            (&[("requirements.txt", "FastAPI==0.115\n")], "fastapi"),
            (&[("pyproject.toml", "dependencies = [\"flask\"]")], "flask"),
            (&[("manage.py", "print(1)")], "static"),
        ];
        for (files, want) in cases {
            let dir = project(files);
            assert_eq!(detect(&dir), want, "{files:?}");
            fs::remove_dir_all(dir).unwrap();
        }
        // Python runs through uv with the project's dependencies, or just its framework.
        let dir = project(&[("requirements.txt", "fastapi"), ("app/main.py", "")]);
        let cmd = default_command("fastapi", &dir).unwrap();
        assert!(
            cmd.starts_with("uv run --with-requirements requirements.txt "),
            "{cmd}"
        );
        assert!(
            cmd.contains("uvicorn app.main:app --host 127.0.0.1 --port $PORT"),
            "{cmd}"
        );
        fs::remove_dir_all(dir).unwrap();
        let cmd = default_command("django", Path::new("/nonexistent")).unwrap();
        assert!(
            cmd.starts_with("uv run --with django python manage.py runserver 127.0.0.1:$PORT"),
            "{cmd}"
        );
        assert!(
            default_command("vite", Path::new("/"))
                .unwrap()
                .contains("--port $PORT")
        );
        assert!(default_command("laravel", Path::new("/")).is_none());
    }
}
