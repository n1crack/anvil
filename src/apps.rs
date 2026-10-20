//! What kind of application a folder is, and running apps that bring their own
//! server (Node, Rust, Go, .NET): Anvil starts them on a private port and Caddy
//! proxies `<name>.<tld>` to it.

use crate::platform::OwnGroup;
use crate::{Config, Site, Stopping, home, kill_orphans, runtimes};
use anyhow::{Context, Result, bail};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
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
        } else if pkg.contains("\"vite\"") || pkg.contains("\"vite-plus\"") {
            // Vite+ is Vite: its `vp dev` needs `--port`, as $PORT means nothing to it.
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

/// The package manager a JS project uses, by its lockfile; npm without one.
pub fn js_manager(dir: &Path) -> &'static str {
    [
        ("pnpm-lock.yaml", "pnpm"),
        ("yarn.lock", "yarn"),
        ("bun.lockb", "bun"),
        ("bun.lock", "bun"),
    ]
    .into_iter()
    .find(|(f, _)| dir.join(f).exists())
    .map_or("npm", |(_, m)| m)
}

/// The shell command installing a JS project's dependencies with `manager`.
/// pnpm and yarn come through Corepack when they aren't on the PATH. pnpm 11
/// fails an install whose dependencies' build scripts it skipped (esbuild); a
/// dev server runs without them, so that's no reason to stop.
pub fn js_install(manager: &str) -> String {
    let via = |m: &str| {
        format!("$(command -v {m} || echo env COREPACK_ENABLE_DOWNLOAD_PROMPT=0 corepack {m})")
    };
    match manager {
        "pnpm" => format!("{} install --config.strict-dep-builds=false", via("pnpm")),
        "yarn" => format!("{} install", via("yarn")),
        "bun" => "bun install".into(),
        _ => "npm install".into(),
    }
}

/// Whether a shell command runs a package script or a package's binary
/// (`npm run dev`, `pnpm build`, `yarn`, `bun run x`, `npx vite`), so needs node_modules.
fn runs_package_script(command: &str) -> bool {
    let words: Vec<&str> = command
        .split(|c: char| c.is_whitespace() || ";&|()'\"".contains(c))
        .filter(|w| !w.is_empty())
        .collect();
    words.iter().enumerate().any(|(i, w)| {
        let next = words.get(i + 1).copied().unwrap_or("");
        match *w {
            "npx" | "pnpx" | "bunx" => true,
            "npm" => matches!(next, "run" | "run-script" | "start" | "test" | "exec"),
            "bun" => matches!(next, "run" | "x" | "dev" | "start"),
            "pnpm" | "yarn" => !matches!(next, "install" | "i" | "add" | "dlx" | "create"),
            _ => false,
        }
    })
}

/// `command`, first installing node_modules with the lockfile's manager when it
/// runs a package script and they are missing, so the install shows in its log.
/// It stops if the install fails, so its error is the last thing in the log.
/// Remove `public/hot` once the dev server it names is gone. Laravel's Vite plugin writes
/// it on start and removes it when told to stop; one that ended any other way (crashed,
/// or ended on Windows, which can't tell a windowless process to stop) leaves it, and
/// pages then load their CSS and JS from nothing, ignoring `npm run build`'s.
pub fn clear_vite_hot(dir: &Path) {
    let hot = dir.join("public").join("hot");
    let Ok(url) = fs::read_to_string(&hot) else {
        return;
    };
    // `http://[::1]:5173`, `http://localhost:5173`, `http://127.0.0.1:5173`.
    let rest = url.trim().split_once("://").map_or("", |(_, r)| r);
    let Some((host, port)) = rest.trim_end_matches('/').rsplit_once(':') else {
        return;
    };
    let Ok(port) = port.parse::<u16>() else {
        return;
    };
    if !matches!(host, "localhost" | "127.0.0.1" | "[::1]") {
        return;
    }
    let answers = |ip: std::net::IpAddr| {
        let addr = std::net::SocketAddr::new(ip, port);
        std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(250)).is_ok()
    };
    if !answers([127, 0, 0, 1].into()) && !answers(std::net::Ipv6Addr::LOCALHOST.into()) {
        let _ = fs::remove_file(hot);
    }
}

pub fn with_js_install(dir: &Path, command: &str) -> String {
    if !runs_package_script(command) || !dir.join("package.json").exists() {
        return command.to_string();
    }
    let install = js_install(js_manager(dir));
    if cfg!(windows) {
        format!("(if not exist node_modules {install}) && {command}")
    } else {
        format!("{{ [ -d node_modules ] || {install}; }} && {command}")
    }
}

/// The command Anvil runs when the app has no custom one. `$PORT` is set.
/// `spawn` installs node_modules first (`with_js_install`).
pub fn default_command(kind: &str, dir: &Path) -> Option<String> {
    let npm = |script: &str| format!("npm run {script}");
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

/// Pick the runtime version for a project: `.anvil.toml`'s `node`, then `.nvmrc` /
/// `.node-version` for Node, otherwise the newest installed. Only installed ones count.
pub fn runtime_version(runtime: &str, dir: &Path) -> Option<String> {
    runtime_version_in(runtime, dir, runtimes::installed(runtime))
}

fn runtime_version_in(runtime: &str, dir: &Path, installed: Vec<String>) -> Option<String> {
    if runtime == "node" {
        if let Some(v) = crate::project::node(dir, &installed) {
            return Some(v);
        }
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
    let hash = crate::stable_hash(key);
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

/// Empty the output of every app and process that no Anvil runs now: their screens
/// show only the current run. Commands run once (`<site>#<id>`) keep their logs.
pub fn clear_stale_logs() {
    clear_stale_in(&home());
}

fn clear_stale_in(home: &Path) {
    let Ok(dir) = fs::read_dir(home.join("log")) else {
        return;
    };
    for f in dir.flatten() {
        let name = f.file_name().to_string_lossy().into_owned();
        let Some(key) = name
            .strip_prefix("app-")
            .and_then(|n| n.strip_suffix(".log"))
        else {
            continue;
        };
        // A symlink is not ours to empty.
        let file = f.file_type().is_ok_and(|t| t.is_file());
        if !key.contains('#') && file && !running_in(home, key) {
            let _ = fs::OpenOptions::new()
                .write(true)
                .open(f.path())
                .and_then(|l| l.set_len(0));
        }
    }
}

fn pid_in(home: &Path, name: &str) -> PathBuf {
    home.join("run").join(format!("app-{name}.pids"))
}

/// Whether any Anvil (the app or `anvil start`) is running this app right now: its
/// own server or any of its processes (`<name>@<process>`, a queue or Vite).
pub fn running_anywhere(name: &str) -> bool {
    let home = home();
    let own = format!("app-{name}@");
    running_in(&home, name)
        || fs::read_dir(home.join("run")).is_ok_and(|dir| {
            dir.flatten().any(|f| {
                let file = f.file_name().to_string_lossy().into_owned();
                file.strip_prefix(&own)
                    .and_then(|k| k.strip_suffix(".pids"))
                    .is_some_and(|k| running_in(&home, &format!("{name}@{k}")))
            })
        })
}

fn running_in(home: &Path, name: &str) -> bool {
    fs::read_to_string(pid_in(home, name)).is_ok_and(|f| {
        f.lines()
            .skip(1)
            .any(|l| crate::alive(l.split(' ').next().unwrap_or(l)))
    })
}

/// Stop what a crashed Anvil left running: every app, process and tunnel whose pid file's
/// owner is gone (see `kill_orphans`). Run once at startup, before `clear_stale_logs`.
pub fn kill_all_orphans() {
    orphans_in(&home());
}

fn orphans_in(home: &Path) {
    let Ok(dir) = fs::read_dir(home.join("run")) else {
        return;
    };
    for f in dir.flatten() {
        let name = f.file_name().to_string_lossy().into_owned();
        // tunnel-<site>: a shared app's cloudflared (`tunnel`).
        if (name.starts_with("app-") || name.starts_with("tunnel-")) && name.ends_with(".pids") {
            kill_orphans(&f.path(), true);
        }
    }
}

/// How long a dev process gets to exit after SIGTERM before it is killed.
pub const GRACE: Duration = Duration::from_secs(5);

/// Apps started from Anvil, by site name.
pub struct Apps {
    /// Where pid and log files go (Anvil's home; a temp folder in tests).
    home: PathBuf,
    children: BTreeMap<String, Child>,
    /// The folder each child was started in.
    dirs: BTreeMap<String, PathBuf>,
    /// Keys of `children` that are an app's other processes, not apps.
    processes: std::collections::BTreeSet<String>,
    /// Keys of `processes` started by `run_once`, with the script's name: they are meant to end.
    once: BTreeMap<String, String>,
    /// How children that ended without `stop` ended, until stopped or started again.
    exits: BTreeMap<String, Exit>,
    /// Each app's terminal size in characters (its Dev output pane), as the UI last said.
    sizes: BTreeMap<String, (u16, u16)>,
}

/// COLUMNS and LINES when the UI hasn't said (autostart): Laravel's and Symfony's
/// padding fits a terminal this wide.
pub const TERM_SIZE: (u16, u16) = (100, 30);

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

/// Keys whose folder is gone: one `stat` each, the cheap sign that a site vanished.
pub fn gone_dirs(running: &[(String, PathBuf)]) -> Vec<&str> {
    let gone = |(_, d): &&(String, PathBuf)| !d.exists();
    running
        .iter()
        .filter(gone)
        .map(|(k, _)| k.as_str())
        .collect()
}

impl Apps {
    fn in_home(home: PathBuf) -> Self {
        Apps {
            home,
            children: BTreeMap::new(),
            dirs: BTreeMap::new(),
            processes: Default::default(),
            once: Default::default(),
            exits: BTreeMap::new(),
            sizes: BTreeMap::new(),
        }
    }

    /// The size of the pane `site`'s output shows in: processes started from now on are
    /// told it as COLUMNS and LINES.
    pub fn set_size(&mut self, site: &str, columns: u16, lines: u16) {
        self.sizes.insert(
            site.to_string(),
            (columns.clamp(20, 1000), lines.clamp(5, 500)),
        );
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
            if let Some(dir) = self.dirs.get(name) {
                clear_vite_hot(dir);
            }
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

    /// A one-off script, as a process of the app (key `<site>#<id>`); its end is not a
    /// crash. Its log holds the last run.
    pub fn run_once(
        &mut self,
        site: &Site,
        script: &crate::processes::Script,
        cfg: &crate::Config,
    ) -> Result<()> {
        let key = crate::processes::once_key(&site.name, &script.id);
        if self.running(&key) {
            return Ok(());
        }
        self.spawn(
            &key,
            &site.path,
            &script.run,
            crate::processes::env(cfg, site, crate::processes::is_test_script(&script.name))?,
        )?;
        self.processes.insert(key.clone());
        self.once.insert(key, script.name.clone());
        Ok(())
    }

    /// `cfg.tld` lets dev servers that check the Host header (Vite, Nuxt) accept `<name>.<tld>`.
    pub fn start(&mut self, site: &Site, cfg: &crate::Config) -> Result<()> {
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
        env.extend(crate::worktrees::checked_env(cfg, site, false)?);
        env.extend([
            ("PORT".to_string(), port.to_string()),
            // Best effort: most dev servers honour one of these and stay off the LAN.
            ("HOST".into(), "127.0.0.1".into()),
            ("HOSTNAME".into(), "127.0.0.1".into()),
            ("ASPNETCORE_URLS".into(), format!("http://127.0.0.1:{port}")),
            // Vite rejects unknown hosts; a leading dot allows every name under the tld.
            (
                "__VITE_ADDITIONAL_SERVER_ALLOWED_HOSTS".into(),
                format!(".{}", cfg.tld),
            ),
        ]);
        self.spawn(&site.name, &site.path, &command, env)
    }

    /// One of an app's other processes (queue, Vite…), see `processes`; `cfg` as in `start`.
    pub fn start_process(
        &mut self,
        site: &Site,
        p: &crate::processes::Process,
        cfg: &crate::Config,
    ) -> Result<()> {
        let key = crate::processes::key(&site.name, &p.id);
        if self.running(&key) {
            return Ok(());
        }
        // Done by the caller beforehand, so this is quick; without its binary Octane would
        // offer to download one into the project.
        crate::octane::prepare(site, p)?;
        self.spawn(&key, &site.path, &p.command, process_env(site, p, cfg)?)?;
        self.processes.insert(key);
        Ok(())
    }

    /// Run `command` in `dir` as its own process group, output to `log_file(key)`, with
    /// colors as in a terminal. The log starts empty, as a new terminal does.
    fn spawn(
        &mut self,
        key: &str,
        dir: &Path,
        command: &str,
        env: Vec<(String, String)>,
    ) -> Result<()> {
        // Its old shell, if a crashed Anvil left one, ends first: nothing writes the log meanwhile.
        kill_orphans(&pid_in(&self.home, key), true);
        crate::state_dirs_in(&self.home)?;
        // Emptied apart: Windows can't shorten a file opened only to append to.
        fs::File::create(log_in(&self.home, key))?;
        let log = fs::OpenOptions::new()
            .append(true)
            .open(log_in(&self.home, key))?;
        let command = crate::processes::with_ansi(command);
        // The trailing argument is $0: it tags the shell with Anvil's home so a
        // crashed Anvil's leftovers can be recognised and cleaned up.
        let tag = format!("anvil:{}:{key}", self.home.display());
        let command = with_js_install(dir, &command);
        // `<site>`, `<site>@<process>` or `<site>#<script>`.
        let site = key.split(['@', '#']).next().unwrap_or(key);
        let (columns, lines) = self.sizes.get(site).copied().unwrap_or(TERM_SIZE);
        let child = crate::platform::shell(&command, Some(&tag))
            .current_dir(dir)
            .envs(crate::processes::COLOR_ENV)
            .env("COLUMNS", columns.to_string())
            .env("LINES", lines.to_string())
            .envs(env)
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log)
            .own_group()
            .spawn()
            .with_context(|| format!("failed to start {key}"))?;
        let at = crate::started(child.id()).unwrap_or_default();
        fs::write(
            pid_in(&self.home, key),
            format!("{}\n{} {at}", crate::owner_line(), child.id()).trim_end(),
        )?;
        self.exits.remove(key);
        self.dirs.insert(key.to_string(), dir.to_path_buf());
        self.children.insert(key.to_string(), child);
        Ok(())
    }

    /// Process-group leader of a running app.
    pub fn pid(&mut self, name: &str) -> Option<u32> {
        self.running(name).then(|| self.children[name].id())
    }

    pub fn stop(&mut self, name: &str) {
        if let Some(s) = self.take(name) {
            s.wait(GRACE);
        }
    }

    /// Forget `name` and hand back its process to stop with `Stopping::wait` once the
    /// lock on `Apps` is released: a shutdown can take 5 s.
    pub fn take(&mut self, name: &str) -> Option<Stopping> {
        self.processes.remove(name);
        self.once.remove(name);
        self.exits.remove(name);
        let dir = self.dirs.remove(name);
        let child = self.children.remove(name)?;
        Some(Stopping {
            child,
            pid_files: vec![pid_in(&self.home, name)],
            dir,
        })
    }

    /// Running children with the folder they were started in.
    pub fn running_dirs(&self) -> Vec<(String, PathBuf)> {
        let dir = |k: &String| self.dirs.get(k).map(|d| (k.clone(), d.clone()));
        self.children.keys().filter_map(dir).collect()
    }

    /// `take` everything (app, processes, one-off scripts, crash records) of a site that is
    /// not in `sites` any more, a worktree whose folder is gone.
    pub fn take_vanished(&mut self, sites: &[Site]) -> Vec<Stopping> {
        let gone = |k: &&String| {
            let site = k.split(['@', '#']).next().unwrap_or(k);
            !sites.iter().any(|s| s.name == site)
        };
        let keys: Vec<String> = self
            .children
            .keys()
            .chain(self.exits.keys())
            .filter(gone)
            .cloned()
            .collect();
        keys.iter().filter_map(|k| self.take(k)).collect()
    }

    /// `take` every app and process, to stop together with `stop_all`.
    pub fn take_all(&mut self) -> Vec<Stopping> {
        let names: Vec<String> = self.children.keys().cloned().collect();
        names.iter().filter_map(|n| self.take(n)).collect()
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
        crate::stop_all(self.take_all(), GRACE);
    }
}

/// A process's environment: Anvil's PATH, plus `PORT` for a server row (`$PORT` in its
/// command), and Vite's allowed hosts as for the app itself (`start`).
pub(crate) fn process_env(
    site: &Site,
    p: &crate::processes::Process,
    cfg: &crate::Config,
) -> Result<Vec<(String, String)>> {
    let mut env = crate::processes::env(cfg, site, false)?;
    env.extend(p.port.map(|port| ("PORT".to_string(), port.to_string())));
    // Octane finds its server's binary on PATH; behind HTTPS it makes https:// URLs.
    if p.id == crate::octane::ID
        && let Some(server) = crate::octane::server(&site.path)
    {
        if let Some((_, path)) = env.iter_mut().find(|(k, _)| k == "PATH") {
            *path = crate::prepend_path(&crate::octane::dir(server), path);
        }
        if site.secure {
            env.push(("OCTANE_HTTPS".into(), "true".into()));
        }
        match server {
            crate::octane::Server::FrankenPhp => {
                crate::octane::frankenphp_env(cfg, site, &mut env)?
            }
            crate::octane::Server::Swoole => crate::octane::swoole_env(cfg, site, &mut env)?,
            crate::octane::Server::RoadRunner => {}
        }
    }
    env.push((
        "__VITE_ADDITIONAL_SERVER_ALLOWED_HOSTS".into(),
        format!(".{}", cfg.tld),
    ));
    Ok(env)
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
    #[test]
    fn running_anywhere_counts_the_apps_processes() {
        let _env = crate::lock(&crate::ENV_LOCK);
        let dir = tempfile::tempdir().unwrap();
        // SAFETY: every test that sets ANVIL_HOME holds ENV_LOCK.
        unsafe { std::env::set_var("ANVIL_HOME", dir.path()) };
        fs::create_dir_all(dir.path().join("run")).unwrap();
        let me = std::process::id();
        let pids = |key: &str| dir.path().join("run").join(format!("app-{key}.pids"));
        fs::write(pids("shop--x@vite"), format!("{me}\n{me} vite\n")).unwrap();
        assert!(running_anywhere("shop--x"));
        assert!(!running_anywhere("shop"));
        fs::write(pids("shop--x@vite"), "4194302\n4194302 vite\n").unwrap();
        assert!(!running_anywhere("shop--x"));
    }

    use super::*;

    #[test]
    fn vite_hot_goes_only_when_its_server_is_gone() {
        let dir = tempfile::tempdir().unwrap();
        let hot = dir.path().join("public/hot");
        fs::create_dir_all(hot.parent().unwrap()).unwrap();
        // A server that answers keeps it.
        let live = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = live.local_addr().unwrap().port();
        fs::write(&hot, format!("http://localhost:{port}")).unwrap();
        clear_vite_hot(dir.path());
        assert!(hot.exists());
        // Gone: removed. Port 1 (tcpmux) has no listener, and no parallel test takes it
        // the way one could take `port` once it is free.
        drop(live);
        fs::write(&hot, "http://[::1]:1").unwrap();
        clear_vite_hot(dir.path());
        assert!(!hot.exists());
        // Not this machine's: left alone.
        fs::write(&hot, "http://vite.example:5173").unwrap();
        clear_vite_hot(dir.path());
        assert!(hot.exists());
    }

    /// Every Start, process and script of an app, in the CLI and the app, makes a
    /// worktree's own database first (`worktrees::prepare_site`, outside the Apps lock).
    #[test]
    fn every_start_path_prepares_the_worktree_database_first() {
        let sources = [
            ("src/main.rs", include_str!("main.rs")),
            ("app/src/main.rs", include_str!("../app/src/main.rs")),
        ];
        let mut calls = 0;
        for (file, text) in sources {
            let lines: Vec<&str> = text.lines().collect();
            for (i, line) in lines.iter().enumerate() {
                let is_call = [".start(", ".start_process(", ".run_once("]
                    .iter()
                    .any(|c| line.contains(c))
                    && (line.contains("apps") || lines[i.saturating_sub(1)].contains("app.apps"));
                if !is_call {
                    continue;
                }
                calls += 1;
                let before = lines[i.saturating_sub(3)..=i].join("\n");
                assert!(
                    before.contains("prepare"),
                    "{file}:{}: starts without prepare first: {line}",
                    i + 1
                );
            }
        }
        // start_app, command_run, start_process (process_run's and process_settings' too),
        // CLI start.
        assert_eq!(calls, 4, "a start path was added or the scan lost one");
    }

    #[test]
    fn node_is_the_project_file_then_nvmrc_then_the_newest() {
        let dir = std::env::temp_dir().join(format!("anvil-node-pick-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let pick = || runtime_version_in("node", &dir, vec!["24".into(), "22".into(), "20".into()]);
        assert_eq!(pick().as_deref(), Some("24"));
        fs::write(dir.join(".nvmrc"), "v20.11.0\n").unwrap();
        assert_eq!(pick().as_deref(), Some("20"));
        fs::write(dir.join(".anvil.toml"), "node = \"22\"\n").unwrap();
        assert_eq!(pick().as_deref(), Some("22"));
        // Not installed: what the folder says otherwise.
        fs::write(dir.join(".anvil.toml"), "node = \"18\"\n").unwrap();
        assert_eq!(pick().as_deref(), Some("20"));
        fs::remove_dir_all(&dir).unwrap();
    }

    /// A folder with `files`, a new one each call (tests run in parallel).
    fn project(files: &[(&str, &str)]) -> PathBuf {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "anvil-detect-{}-{}-{}",
            std::process::id(),
            N.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
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
        let cases: [(&[(&str, &str)], &str); 25] = [
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
            // Vite+ (`vp dev`) is Vite, so it is given --port.
            (
                &[(
                    "package.json",
                    "{\"devDependencies\":{\"vite-plus\":\"1\"}}",
                )],
                "vite",
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
    fn package_scripts_install_node_modules_first() {
        for cmd in [
            "npm run dev",
            "pnpm dev",
            "yarn",
            "bun run build",
            "npx vite",
            "cd web && npm start",
            "php artisan serve & npm run dev",
        ] {
            assert!(runs_package_script(cmd), "{cmd}");
        }
        for cmd in [
            "php artisan queue:work",
            "npm install",
            "pnpm install",
            "cargo run",
            "echo npm",
        ] {
            assert!(!runs_package_script(cmd), "{cmd}");
        }
        let dir = project(&[("package.json", "{}")]);
        assert_eq!(js_manager(&dir), "npm");
        assert_eq!(
            with_js_install(&dir, "npm run dev"),
            "{ [ -d node_modules ] || npm install; } && npm run dev"
        );
        assert_eq!(with_js_install(&dir, "cargo run"), "cargo run");
        for (lock, manager) in [
            ("pnpm-lock.yaml", "pnpm"),
            ("yarn.lock", "yarn"),
            ("bun.lockb", "bun"),
            ("bun.lock", "bun"),
        ] {
            fs::write(dir.join(lock), "").unwrap();
            assert_eq!(js_manager(&dir), manager);
            assert!(with_js_install(&dir, "npx vite").contains(&js_install(manager)));
            fs::remove_file(dir.join(lock)).unwrap();
        }
        // Without a package.json there is nothing to install.
        fs::remove_file(dir.join("package.json")).unwrap();
        assert_eq!(with_js_install(&dir, "npm run dev"), "npm run dev");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn next_origin_covers_the_double_dash_rows() {
        // `**.test` is what allowedDevOrigins gets; Next's glob `**` spans labels.
        let dir = project(&[("next.config.mjs", "export default {\n};\n")]);
        allow_next_origin(&dir, "test").unwrap();
        assert!(next_allows(&dir, "test"));
        fs::remove_dir_all(dir).unwrap();
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
            root_custom: None,
            root_detected: ".".into(),
            php: None,
            php_pinned: false,
            ext: vec![],
            own_master: false,
            octane_ports: None,
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
            worktree_of: None,
            branch: None,
            aliases: vec![],
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
        apps.start_process(&site, &idle, &crate::Config::default())
            .unwrap();
        apps.start_process(&site, &boom, &crate::Config::default())
            .unwrap();
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
        apps.start_process(&site, &boom, &crate::Config::default())
            .unwrap();
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
        apps.start_process(&site, &proc("gone", "sleep 30"), &crate::Config::default())
            .unwrap();
        apps.start_process(&site, &proc("boom", "exit 2"), &crate::Config::default())
            .unwrap();
        let mut other = site_at(&dir);
        other.name = "shopping".into();
        apps.start_process(&other, &proc("w", "sleep 30"), &crate::Config::default())
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
    fn a_start_begins_with_an_empty_colored_log() {
        let dir = project(&[("fresh/x", "")]);
        let mut apps = Apps::in_home(dir.join("home"));
        let site = site_at(&dir);
        let p = proc(
            "env",
            "echo \"$FORCE_COLOR $COLORTERM $TERM $CLICOLOR_FORCE\"; echo run",
        );
        let log = dir.join("home/log/app-shop@env.log");
        let wait_for = |want: &str| {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while !fs::read_to_string(&log).unwrap_or_default().contains(want) {
                assert!(
                    std::time::Instant::now() < deadline,
                    "no {want:?} in the log"
                );
                std::thread::sleep(Duration::from_millis(20));
            }
        };
        apps.start_process(&site, &p, &crate::Config::default())
            .unwrap();
        wait_for("run");
        apps.stop("shop@env");
        apps.start_process(&site, &p, &crate::Config::default())
            .unwrap();
        wait_for("run");
        std::thread::sleep(Duration::from_millis(100));
        // Only the second run's output, with the color variables set.
        assert_eq!(
            fs::read_to_string(&log).unwrap(),
            "1 truecolor xterm-256color 1\nrun\n"
        );
        apps.stop("shop@env");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_new_anvil_empties_only_logs_of_what_no_longer_runs() {
        let dir = project(&[("stale/x", "")]);
        let home = dir.join("home");
        fs::create_dir_all(home.join("log")).unwrap();
        fs::create_dir_all(home.join("run")).unwrap();
        for key in ["shop", "shop@queue", "shop@vite", "shop#build"] {
            fs::write(log_in(&home, key), "old run\n").unwrap();
        }
        // vite still runs (this test process stands in for it); queue's pid file is stale.
        let me = std::process::id();
        fs::write(pid_in(&home, "shop@vite"), format!("{me}\n{me}")).unwrap();
        fs::write(pid_in(&home, "shop@queue"), "999999\n999999").unwrap();
        clear_stale_in(&home);
        let text = |key| fs::read_to_string(log_in(&home, key)).unwrap();
        assert_eq!(text("shop"), "");
        assert_eq!(text("shop@queue"), "");
        assert_eq!(text("shop@vite"), "old run\n");
        assert_eq!(text("shop#build"), "old run\n");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn startup_kills_only_the_orphans_anvil_started() {
        let dir = project(&[("orphans/x", "")]);
        let home = dir.join("home");
        fs::create_dir_all(home.join("run")).unwrap();
        // An owner that is gone: a child that already ended.
        let mut gone = crate::platform::command("true").spawn().unwrap();
        gone.wait().unwrap();
        let sleep = || {
            crate::platform::command("sleep")
                .arg("30")
                .own_group()
                .spawn()
                .unwrap()
        };
        let (mut orphan, mut stranger, mut tunnel) = (sleep(), sleep(), sleep());
        let at = crate::started(orphan.id()).unwrap();
        let file = |key, pid, at: &str| {
            fs::write(pid_in(&home, key), format!("{}\n{pid} {at}", gone.id())).unwrap()
        };
        file("shop", orphan.id(), &at);
        // Same pid file shape, but the pid now names a process started another time.
        file("shop@queue", stranger.id(), "Mon Jan  1 00:00:00 2001");
        // A shared app's cloudflared (`tunnel`).
        fs::write(
            home.join("run/tunnel-shop.pids"),
            format!(
                "{}\n{} {}",
                gone.id(),
                tunnel.id(),
                crate::started(tunnel.id()).unwrap()
            ),
        )
        .unwrap();
        orphans_in(&home);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while orphan.try_wait().unwrap().is_none() || tunnel.try_wait().unwrap().is_none() {
            assert!(
                std::time::Instant::now() < deadline,
                "the orphan still runs"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            stranger.try_wait().unwrap().is_none(),
            "a reused pid was killed"
        );
        assert!(!pid_in(&home, "shop").exists());
        let _ = stranger.kill();
        let _ = stranger.wait();
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn startup_stops_whole_orphan_groups_before_dropping_their_pid_files() {
        use nix::sys::signal::{Signal, killpg};
        use nix::unistd::Pid;
        let dir = project(&[("groups/x", "")]);
        let home = dir.join("home");
        fs::create_dir_all(home.join("run")).unwrap();
        let mut gone = crate::platform::command("true").spawn().unwrap();
        gone.wait().unwrap();
        // A dev shell as `spawn` starts it: its own group, Anvil's tag as $0, a child in
        // its group. Reaped once it ends, as launchd reaps an orphan.
        let shell = |key: &str, script: &str| {
            let tag = format!("anvil:{}:{key}", home.display());
            let mut c = crate::platform::command("/bin/sh")
                .args(["-c", script, &tag])
                .own_group()
                .spawn()
                .unwrap();
            let pid = c.id();
            std::thread::spawn(move || c.wait());
            pid
        };
        let file = |key, pid: u32, at: &str| {
            fs::write(pid_in(&home, key), format!("{}\n{pid} {at}", gone.id())).unwrap()
        };
        // Its start time as `ps` wrote it for an Anvil started from a Turkish terminal; a
        // relaunch from Finder reads "Tue Oct 20 …" for the same process.
        let dev = shell("shop@dev", "sleep 60 & wait");
        let tr = crate::platform::command("ps")
            .args(["-p", &dev.to_string(), "-o", "lstart="])
            .env("LC_ALL", "tr_TR.UTF-8")
            .output()
            .unwrap();
        file("shop@dev", dev, String::from_utf8_lossy(&tr.stdout).trim());
        // Its leader ended; its child still holds the port.
        let start = shell("shop@start", "sleep 60 & sleep 0.3");
        file("shop@start", start, &crate::started(start).unwrap());
        while crate::alive(&start.to_string()) {
            std::thread::sleep(Duration::from_millis(20));
        }
        // Ignores SIGTERM: SIGKILL after GRACE.
        let queue = shell("shop@queue", "trap '' TERM; sleep 60 & wait");
        file("shop@queue", queue, &crate::started(queue).unwrap());
        std::thread::sleep(Duration::from_millis(100)); // the trap is set

        orphans_in(&home);
        std::thread::sleep(Duration::from_millis(100)); // SIGKILL lands
        let left: Vec<_> = [("dev", dev), ("start", start), ("queue", queue)]
            .into_iter()
            .filter(|(_, g)| killpg(Pid::from_raw(*g as i32), None).is_ok())
            .collect();
        for (_, g) in &left {
            let _ = killpg(Pid::from_raw(*g as i32), Signal::SIGKILL);
        }
        assert!(left.is_empty(), "still running: {left:?}");
        for key in ["shop@dev", "shop@start", "shop@queue"] {
            assert!(!pid_in(&home, key).exists(), "{key}'s pid file is left");
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn run_once_ends_without_a_crash() {
        let dir = project(&[("once/x", "")]);
        let mut apps = Apps::in_home(dir.join("home"));
        let site = site_at(&dir);
        // No package.json: npm fails (or is missing), which still must not count as a crash.
        let script = crate::processes::Script {
            name: "build:all".into(),
            id: "build-all".into(),
            text: String::new(),
            source: "package.json",
            run: "npm run build:all".into(),
        };
        apps.run_once(&site, &script, &crate::Config::default())
            .unwrap();
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

    #[test]
    fn processes_get_the_pane_size_or_a_default() {
        let dir = project(&[("size/x", "")]);
        let mut apps = Apps::in_home(dir.join("home"));
        let site = site_at(&dir);
        let size = |apps: &mut Apps, id: &str| {
            apps.start_process(
                &site,
                &proc(id, "echo $COLUMNS $LINES"),
                &crate::Config::default(),
            )
            .unwrap();
            let key = crate::processes::key(&site.name, id);
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            while apps.running(&key) {
                assert!(std::time::Instant::now() < deadline, "echo never ended");
                std::thread::sleep(Duration::from_millis(20));
            }
            fs::read_to_string(log_in(&apps.home, &key)).unwrap()
        };
        // Autostart: nothing said yet.
        assert_eq!(size(&mut apps, "a"), "100 30\n");
        apps.set_size(&site.name, 132, 40);
        assert_eq!(size(&mut apps, "b"), "132 40\n");
        // Another app keeps the default; nonsense is kept in bounds.
        apps.set_size("other", 3, 0);
        assert_eq!(apps.sizes["other"], (20, 5));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn each_test_project_has_its_own_folder() {
        // Two tests asking for the same files run in parallel: one's clean-up mustn't race the other.
        assert_ne!(project(&[("x/y", "")]), project(&[("x/y", "")]));
    }

    #[test]
    fn processes_of_a_vanished_site_are_stopped() {
        let dir = project(&[("x/y", "")]);
        let mut apps = Apps::in_home(dir.join("home"));
        let (mut gone, mut kept) = (site_at(&dir), site_at(&dir));
        gone.name = "shop--x".into();
        kept.name = "shop".into();
        let cfg = crate::Config::default();
        for s in [&gone, &kept] {
            apps.start_process(s, &proc("w", "sleep 30"), &cfg).unwrap();
        }
        apps.start_process(&gone, &proc("boom", "exit 2"), &cfg)
            .unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let stopping = apps.take_vanished(std::slice::from_ref(&kept));
        assert_eq!(stopping.len(), 2); // running, and crashed but not yet polled
        crate::stop_all(stopping, GRACE);
        assert!(apps.process_keys("shop--x").is_empty());
        assert_eq!(apps.process_keys("shop"), ["shop@w"]);
        apps.stop("shop@w");
    }

    #[test]
    fn a_running_child_whose_folder_is_gone_is_reported() {
        let dir = project(&[("x/y", "")]);
        let (a, b) = (dir.join("a"), dir.join("b"));
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        let mut apps = Apps::in_home(dir.join("home"));
        let cfg = crate::Config::default();
        for (name, d) in [("shop--a", &a), ("shop--b", &b)] {
            let mut s = site_at(&dir);
            (s.name, s.path) = (name.into(), d.clone());
            apps.start_process(&s, &proc("w", "sleep 30"), &cfg)
                .unwrap();
        }
        assert!(gone_dirs(&apps.running_dirs()).is_empty());
        fs::remove_dir_all(&a).unwrap();
        assert_eq!(gone_dirs(&apps.running_dirs()), ["shop--a@w"]);
        crate::stop_all(apps.take_all(), GRACE);
        assert!(apps.running_dirs().is_empty());
    }
}
