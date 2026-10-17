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
pub const KINDS: [&str; 17] = [
    "laravel", "statamic", "symfony", "php", "next", "nuxt", "vite", "node", "rust", "go",
    "dotnet", "django", "fastapi", "flask", "compose", "static", "folder",
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
        "compose" => "Docker Compose",
        "folder" => "Folder",
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
    // Next.js with a Flask API folder is a Node app; a Python app has its entry here.
    let entry = ["main.py", "app.py", "wsgi.py", "app/main.py"]
        .iter()
        .any(|f| has(f));
    if entry || !has("package.json") {
        if py.contains("fastapi") {
            return "fastapi";
        }
        if py.contains("flask") {
            return "flask";
        }
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
    // A page to serve: any .html at the top or in public/.
    let page = |d: &Path| {
        fs::read_dir(d).into_iter().flatten().flatten().any(|e| {
            let n = e.file_name();
            let n = n.to_string_lossy();
            n.ends_with(".html") || n.ends_with(".htm")
        })
    };
    let pages = page(dir) || page(&dir.join("public"));
    if crate::compose::file(dir).is_some() && !pages {
        return "compose";
    }
    // Nothing to serve: a project folder, not a site.
    if pages { "static" } else { "folder" }
}

/// Whether Anvil gives the app an address: a Compose project's containers answer on
/// their own ports, and a plain folder has no page.
pub fn serves(kind: &str) -> bool {
    !matches!(kind, "compose" | "folder")
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
        // Next ignores $HOSTNAME and would listen on every interface.
        "next" => npm("dev -- --hostname 127.0.0.1"),
        "nuxt" => npm("dev"),
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
/// project declaring neither gets just its framework. Without its own [project],
/// --no-project keeps uv from adopting a pyproject.toml in a parent folder.
fn uv_run(dir: &Path, framework: &str, cmd: &str) -> String {
    // A pyproject.toml with only tool settings (ruff, poetry) isn't a uv project.
    if read(&dir.join("pyproject.toml")).contains("[project]") {
        format!("uv run {cmd}")
    } else if dir.join("requirements.txt").exists() {
        format!("uv run --no-project --with-requirements requirements.txt {cmd}")
    } else {
        format!("uv run --no-project --with {framework} {cmd}")
    }
}

const NEXT_CONFIGS: [&str; 5] = [
    "next.config.ts",
    "next.config.mjs",
    "next.config.js",
    "next.config.cjs",
    "next.config.mts",
];

fn next_config(dir: &Path) -> Option<PathBuf> {
    NEXT_CONFIGS
        .iter()
        .map(|f| dir.join(f))
        .find(|p| p.exists())
}

/// Whether a Next.js app accepts `.<tld>`. In development Next only answers
/// `localhost` and `*.localhost` unless `allowedDevOrigins` lists the address;
/// anywhere else the page loads but never becomes interactive.
pub fn next_allows(dir: &Path, tld: &str) -> bool {
    tld == "localhost"
        || next_config(dir).is_some_and(|p| {
            let text = read(&p);
            text.contains("allowedDevOrigins")
                && ['\'', '"', '`']
                    .iter()
                    .any(|q| text.contains(&format!(".{tld}{q}")))
        })
}

/// Add `allowedDevOrigins: ['**.<tld>']` to a Next.js app's config. Only a config
/// with one plain top-level object is edited; anything else is left to the user.
pub fn allow_next_origin(dir: &Path, tld: &str) -> Result<()> {
    let line = format!("  allowedDevOrigins: ['**.{tld}'],");
    let Some(path) = next_config(dir) else {
        return Ok(fs::write(
            dir.join("next.config.mjs"),
            format!("export default {{\n{line}\n}};\n"),
        )?);
    };
    let text = read(&path);
    let file = path.file_name().unwrap().to_string_lossy();
    if text.contains("allowedDevOrigins") {
        bail!("{file} already sets allowedDevOrigins; add '**.{tld}' to it");
    }
    let opens = |l: &str| {
        let l = l.trim_end();
        l == "export default {"
            || l.ends_with("= {")
                && ["const ", "let ", "var ", "module.exports"]
                    .iter()
                    .any(|p| l.starts_with(p))
    };
    let mut lines: Vec<&str> = text.lines().collect();
    let found: Vec<usize> = (0..lines.len()).filter(|&i| opens(lines[i])).collect();
    let [at] = found[..] else {
        bail!(
            "Couldn't find the config object in {file}; add {} to it",
            line.trim()
        );
    };
    lines.insert(at + 1, &line);
    Ok(fs::write(&path, lines.join("\n") + "\n")?)
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
        let port = hashed_port(&s.name, 41000, &taken, cfg);
        taken.push(port);
        s.port = Some(port);
    }
}

/// A stable port from `key` in `base..base + 4000`, moved up past `taken` and Anvil's own web ports.
pub fn hashed_port(key: &str, base: u16, taken: &[u16], cfg: &Config) -> u16 {
    let hash = key
        .bytes()
        .fold(5381u32, |h, b| h.wrapping_mul(33) ^ u32::from(b));
    let mut port = base + (hash % 4000) as u16;
    while taken.contains(&port) || cfg.https_port == port || cfg.http_port == port {
        port += 1;
    }
    port
}

pub fn log_file(name: &str) -> PathBuf {
    log_in(&home(), name)
}

fn log_in(home: &Path, name: &str) -> PathBuf {
    home.join("log").join(format!("app-{name}.log"))
}

fn pid_file(name: &str) -> PathBuf {
    pid_in(&home(), name)
}

fn pid_in(home: &Path, name: &str) -> PathBuf {
    home.join("run").join(format!("app-{name}.pids"))
}

/// Whether any Anvil (the app or `anvil start`) is running this app right now.
pub fn running_anywhere(name: &str) -> bool {
    fs::read_to_string(pid_file(name)).is_ok_and(|f| f.lines().skip(1).any(crate::alive))
}

/// Stop a whole process group: `npm run dev` leaves node children behind otherwise.
fn terminate_group(child: &mut Child, grace: Duration) {
    let group = format!("-{}", child.id());
    let kill = |sig: &str| {
        let _ = Command::new("kill")
            .args([sig, "--", &group])
            .stderr(Stdio::null())
            .status();
    };
    kill("-TERM");
    let deadline = std::time::Instant::now() + grace;
    while std::time::Instant::now() < deadline {
        if let Ok(Some(_)) = child.try_wait() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    // Anything in the group still alive gets killed.
    kill("-KILL");
    let _ = child.wait();
}

/// Apps started from Anvil, by site name.
pub struct Apps {
    /// Where pid and log files go (Anvil's home; a temp folder in tests).
    home: PathBuf,
    children: BTreeMap<String, Child>,
    /// Keys of `children` that are an app's other processes, not apps.
    processes: std::collections::BTreeSet<String>,
    /// Keys of `processes` started by `run_once`, with the script's name: they are meant to end.
    once: BTreeMap<String, String>,
    /// How children that ended without `stop` ended, until stopped or started again.
    exits: BTreeMap<String, Exit>,
}

/// How a process ended on its own.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Exit {
    /// `None` when a signal ended it.
    pub code: Option<i32>,
    /// Unix seconds.
    pub at: u64,
}

impl Default for Apps {
    fn default() -> Self {
        Apps::in_home(home())
    }
}

impl Apps {
    fn in_home(home: PathBuf) -> Self {
        Apps {
            home,
            children: BTreeMap::new(),
            processes: Default::default(),
            once: Default::default(),
            exits: BTreeMap::new(),
        }
    }

    pub fn running(&mut self, name: &str) -> bool {
        let ended = self
            .children
            .get_mut(name)
            .and_then(|c| match c.try_wait() {
                Ok(None) => None,
                Ok(Some(status)) => Some(status.code()),
                Err(_) => Some(None),
            });
        if let Some(code) = ended {
            self.children.remove(name);
            let at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            self.exits.insert(name.to_string(), Exit { code, at });
        }
        self.children.contains_key(name)
    }

    /// Processes that ended on their own (not `run_once` ones), newest record per key.
    pub fn crashed(&mut self) -> Vec<(String, Exit)> {
        let keys: Vec<String> = self.children.keys().cloned().collect();
        for k in keys {
            self.running(&k);
        }
        self.exits
            .iter()
            .filter(|(k, _)| !self.once.contains_key(*k))
            .map(|(k, e)| (k.clone(), e.clone()))
            .collect()
    }

    /// The script a `run_once` key runs (`build:all` for `<site>#build-all`).
    pub fn once_name(&self, key: &str) -> Option<&str> {
        self.once.get(key).map(String::as_str)
    }

    /// Every running or crashed process of `site`, `<site>@…` and `<site>#…`, whether
    /// or not detection still lists it (a renamed script, an edited `.anvil.toml`).
    pub fn process_keys(&self, site: &str) -> Vec<String> {
        let mine = |k: &&String| {
            k.strip_prefix(site)
                .is_some_and(|rest| rest.starts_with(['@', '#']))
        };
        let mut keys: Vec<String> = self.children.keys().filter(mine).cloned().collect();
        keys.extend(self.exits.keys().filter(mine).cloned());
        keys.sort();
        keys.dedup();
        keys
    }

    /// How a `run_once` command ended, once it has.
    pub fn finished(&mut self, key: &str) -> Option<Exit> {
        self.running(key);
        self.exits.get(key).cloned()
    }

    /// Running keys with their process-group pid; `true` for an app's other processes.
    pub fn list(&mut self) -> Vec<(String, u32, bool)> {
        let keys: Vec<String> = self.children.keys().cloned().collect();
        for k in keys {
            self.running(&k);
        }
        self.children
            .iter()
            .map(|(k, c)| (k.clone(), c.id(), self.processes.contains(k)))
            .collect()
    }

    /// `npm run <script>` once, as a process of the app (key `<site>#<id>`); its end
    /// is not a crash.
    pub fn run_once(&mut self, site: &Site, script: &str) -> Result<()> {
        let key = crate::processes::once_key(&site.name, script);
        if self.running(&key) {
            return Ok(());
        }
        let command = format!("npm run '{}'", script.replace('\'', r"'\''"));
        self.spawn(&key, &site.path, &command, crate::processes::env(site))?;
        self.processes.insert(key.clone());
        self.once.insert(key, script.to_string());
        Ok(())
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

        // app_env: Python's version and uv environment rules on top of the runtime's.
        let mut env = app_env(site, runtime, &version);
        env.extend([
            ("PORT".to_string(), port.to_string()),
            // Best effort: most dev servers honour one of these and stay off the LAN.
            ("HOST".into(), "127.0.0.1".into()),
            ("HOSTNAME".into(), "127.0.0.1".into()),
            ("ASPNETCORE_URLS".into(), format!("http://127.0.0.1:{port}")),
            // Vite rejects unknown hosts; a leading dot allows every name under the tld.
            (
                "__VITE_ADDITIONAL_SERVER_ALLOWED_HOSTS".into(),
                format!(".{tld}"),
            ),
        ]);
        self.spawn(&site.name, &site.path, &command, env)
    }

    /// One of an app's other processes (queue, Vite…), see `processes`; `tld` as in `start`.
    pub fn start_process(
        &mut self,
        site: &Site,
        p: &crate::processes::Process,
        tld: &str,
    ) -> Result<()> {
        let key = crate::processes::key(&site.name, &p.id);
        if self.running(&key) {
            return Ok(());
        }
        self.spawn(&key, &site.path, &p.command, process_env(site, p, tld))?;
        self.processes.insert(key);
        Ok(())
    }

    /// Run `command` in `dir` as its own process group, output to `log_file(key)`.
    fn spawn(
        &mut self,
        key: &str,
        dir: &Path,
        command: &str,
        env: Vec<(String, String)>,
    ) -> Result<()> {
        kill_orphans(&pid_in(&self.home, key), true);
        crate::state_dirs_in(&self.home)?;
        let log = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_in(&self.home, key))?;
        // The trailing argument is $0: it tags the shell with Anvil's home so a
        // crashed Anvil's leftovers can be recognised and cleaned up.
        let tag = format!("anvil:{}:{key}", self.home.display());
        let child = Command::new("/bin/sh")
            .args(["-c", command, &tag])
            .current_dir(dir)
            .envs(env)
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log)
            .process_group(0)
            .spawn()
            .with_context(|| format!("failed to start {key}"))?;
        fs::write(
            pid_in(&self.home, key),
            format!("{}\n{}", std::process::id(), child.id()),
        )?;
        self.exits.remove(key);
        self.children.insert(key.to_string(), child);
        Ok(())
    }

    /// Process-group leader of a running app.
    pub fn pid(&mut self, name: &str) -> Option<u32> {
        self.running(name).then(|| self.children[name].id())
    }

    pub fn stop(&mut self, name: &str) {
        self.processes.remove(name);
        self.once.remove(name);
        self.exits.remove(name);
        if let Some(mut c) = self.children.remove(name) {
            terminate_group(&mut c, Duration::from_secs(5));
            let _ = fs::remove_file(pid_in(&self.home, name));
        }
    }

    /// Running apps (not their other processes).
    pub fn names(&mut self) -> Vec<String> {
        let names: Vec<String> = self
            .children
            .keys()
            .filter(|k| !self.processes.contains(*k))
            .cloned()
            .collect();
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

/// A process's environment: Anvil's PATH, plus `PORT` for a server row (`$PORT` in its
/// command), and Vite's allowed hosts as for the app itself (`start`).
pub(crate) fn process_env(
    site: &Site,
    p: &crate::processes::Process,
    tld: &str,
) -> Vec<(String, String)> {
    let mut env = crate::processes::env(site);
    env.extend(p.port.map(|port| ("PORT".to_string(), port.to_string())));
    env.push((
        "__VITE_ADDITIONAL_SERVER_ALLOWED_HOSTS".into(),
        format!(".{tld}"),
    ));
    env
}

/// The runtime's environment for one app. Python leaves the version to the
/// project (`.python-version`, `requires-python`) unless it is pinned in Anvil,
/// and keeps uv's environment out of the project, so an existing `.venv` stays.
fn app_env(site: &Site, runtime: &str, version: &str) -> Vec<(String, String)> {
    let mut env = runtimes::env(runtime, version);
    if runtime == "python" {
        if !site.runtime_pinned {
            env.retain(|(k, _)| k != "UV_PYTHON");
        }
        let venvs = home().join("cache").join("uv-venvs").join(&site.name);
        env.push(("UV_PROJECT_ENVIRONMENT".into(), venvs.display().to_string()));
    }
    env
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
        let cases: [(&[(&str, &str)], &str); 24] = [
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
            (&[("docker-compose.yml", ""), ("README.md", "")], "compose"),
            (&[("docker-compose.yml", ""), ("index.html", "")], "static"),
            (
                &[("docker-compose.yml", ""), ("public/index.html", "")],
                "static",
            ),
            (&[("docker-compose.yml", ""), ("index.php", "")], "php"),
            (
                &[
                    ("artisan", ""),
                    ("composer.json", "{}"),
                    ("docker-compose.yml", ""),
                ],
                "laravel",
            ),
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
            (&[("manage.py", "print(1)")], "folder"),
            // Nothing to serve: a project folder, not a site.
            (&[("README.md", ""), ("Cargo.lock", "")], "folder"),
            (&[("about.htm", "")], "static"),
            (&[("public/landing.html", "")], "static"),
            // Next.js with an API that uses Flask stays Next.js; with an entry file it's Flask.
            (
                &[
                    ("package.json", "{\"next\":1}"),
                    ("requirements.txt", "flask"),
                ],
                "next",
            ),
            (
                &[
                    ("package.json", "{}"),
                    ("requirements.txt", "flask"),
                    ("app.py", ""),
                ],
                "flask",
            ),
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
            cmd.starts_with("uv run --no-project --with-requirements requirements.txt "),
            "{cmd}"
        );
        assert!(
            cmd.contains("uvicorn app.main:app --host 127.0.0.1 --port $PORT"),
            "{cmd}"
        );
        fs::remove_dir_all(dir).unwrap();
        // A tool-only pyproject.toml isn't a uv project: requirements.txt still counts.
        let dir = project(&[
            ("pyproject.toml", "[tool.ruff]\n"),
            ("requirements.txt", "flask"),
        ]);
        assert!(
            default_command("flask", &dir)
                .unwrap()
                .contains("--with-requirements")
        );
        fs::remove_dir_all(dir).unwrap();
        let cmd = default_command("django", Path::new("/nonexistent")).unwrap();
        assert!(
            cmd.starts_with(
                "uv run --no-project --with django python manage.py runserver 127.0.0.1:$PORT"
            ),
            "{cmd}"
        );
        assert!(
            default_command("vite", Path::new("/"))
                .unwrap()
                .contains("--port $PORT")
        );
        assert!(default_command("laravel", Path::new("/")).is_none());
        assert!(
            default_command("next", Path::new("/"))
                .unwrap()
                .ends_with("npm run dev -- --hostname 127.0.0.1")
        );
    }

    #[test]
    fn next_config_gets_the_tld() {
        // create-next-app's TypeScript config.
        let dir = project(&[(
            "next.config.ts",
            "import type { NextConfig } from \"next\";\n\nconst nextConfig: NextConfig = {\n  /* config options here */\n};\n\nexport default nextConfig;\n",
        )]);
        assert!(!next_allows(&dir, "test"));
        assert!(next_allows(&dir, "localhost"));
        allow_next_origin(&dir, "test").unwrap();
        let text = fs::read_to_string(dir.join("next.config.ts")).unwrap();
        assert!(
            text.contains("= {\n  allowedDevOrigins: ['**.test'],\n  /* config"),
            "{text}"
        );
        assert!(next_allows(&dir, "test"));
        assert!(!next_allows(&dir, "anvil"));
        // Already set: Anvil doesn't merge into the user's list.
        assert!(allow_next_origin(&dir, "anvil").is_err());
        fs::remove_dir_all(dir).unwrap();

        // No config yet: one is written.
        let dir = project(&[("next-env.d.ts", "")]);
        allow_next_origin(&dir, "test").unwrap();
        assert!(next_allows(&dir, "test"));
        fs::remove_dir_all(dir).unwrap();

        // A config built by a function isn't edited.
        let dir = project(&[(
            "next.config.mjs",
            "export default withMDX({\n  reactStrictMode: true,\n});\n",
        )]);
        assert!(allow_next_origin(&dir, "test").is_err());
        assert!(
            !fs::read_to_string(dir.join("next.config.mjs"))
                .unwrap()
                .contains("allowed")
        );
        fs::remove_dir_all(dir).unwrap();
    }

    fn site_at(dir: &Path) -> Site {
        Site {
            name: "shop".into(),
            host: String::new(),
            secure: false,
            path: dir.into(),
            root: dir.into(),
            php: None,
            php_pinned: false,
            ini: Default::default(),
            linked: false,
            parked_in: None,
            modified: 0,
            kind: "node".into(),
            kind_detected: "node".into(),
            command: None,
            command_custom: false,
            port: None,
            autostart: false,
            runtime_version: None,
            runtime_pinned: false,
        }
    }

    fn proc(name: &str, command: &str) -> crate::processes::Process {
        crate::processes::Process {
            id: crate::processes::process_id(name),
            name: name.into(),
            command: command.into(),
            source: ".anvil.toml",
            server: false,
            port: None,
            ..Default::default()
        }
    }

    #[test]
    fn remembers_processes_that_exit_on_their_own() {
        let dir = project(&[("crash/x", "")]);
        let mut apps = Apps::in_home(dir.join("home"));
        let site = site_at(&dir);
        let (boom, idle) = (proc("boom", "sh -c 'exit 3'"), proc("docs:dev", "sleep 30"));
        apps.start_process(&site, &idle, "test").unwrap();
        apps.start_process(&site, &boom, "test").unwrap();
        std::thread::sleep(Duration::from_millis(300));

        let crashed = apps.crashed();
        assert_eq!(crashed.len(), 1);
        assert_eq!(crashed[0].0, "shop@boom");
        assert_eq!(crashed[0].1.code, Some(3));
        // The id, not the display name, makes the key and the log file.
        let list = apps.list();
        assert_eq!(list.len(), 1);
        assert_eq!((list[0].0.as_str(), list[0].2), ("shop@docs-dev", true));
        assert_eq!(Some(list[0].1), apps.pid("shop@docs-dev"));
        assert!(dir.join("home/log/app-shop@docs-dev.log").exists());

        // Starting it again clears the record; crashing again leaves exactly one.
        apps.start_process(&site, &boom, "test").unwrap();
        assert!(!apps.exits.contains_key("shop@boom"));
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(apps.crashed().len(), 1);

        // A stop is not a crash.
        apps.stop("shop@docs-dev");
        apps.stop("shop@boom");
        assert!(apps.crashed().is_empty());
        assert!(apps.list().is_empty());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn lists_every_process_of_a_site_detected_or_not() {
        let dir = project(&[("keys/x", "")]);
        let mut apps = Apps::in_home(dir.join("home"));
        let site = site_at(&dir);
        // Neither id is detected in `dir` (no package.json, no .anvil.toml).
        apps.start_process(&site, &proc("gone", "sleep 30"), "test")
            .unwrap();
        apps.start_process(&site, &proc("boom", "exit 2"), "test")
            .unwrap();
        let mut other = site_at(&dir);
        other.name = "shopping".into();
        apps.start_process(&other, &proc("w", "sleep 30"), "test")
            .unwrap();
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(apps.process_keys("shop"), ["shop@boom", "shop@gone"]);
        for k in apps.process_keys("shop") {
            apps.stop(&k);
        }
        assert!(apps.process_keys("shop").is_empty());
        assert!(apps.crashed().is_empty());
        assert_eq!(apps.process_keys("shopping"), ["shopping@w"]);
        apps.stop("shopping@w");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn run_once_ends_without_a_crash() {
        let dir = project(&[("once/x", "")]);
        let mut apps = Apps::in_home(dir.join("home"));
        let site = site_at(&dir);
        // No package.json: npm fails (or is missing), which still must not count as a crash.
        apps.run_once(&site, "build:all").unwrap();
        assert!(apps.processes.contains("shop#build-all"));
        // The Processes window shows the script, not its id.
        assert_eq!(apps.once_name("shop#build-all"), Some("build:all"));
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while apps.finished("shop#build-all").is_none() {
            assert!(std::time::Instant::now() < deadline, "npm never ended");
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(apps.crashed().is_empty());
        fs::remove_dir_all(dir).unwrap();
    }
}
