//! The other processes an app runs while you work on it: a queue worker, a log
//! tailer, Vite. Read from `.anvil.toml`, else from the project's own dev script
//! (`composer run dev` with concurrently, or package.json's `dev`).

use crate::{Site, apps, home, runtimes};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Process {
    /// Key and file-name part (`process_id` of the name); `name` is what the UI shows.
    pub id: String,
    pub name: String,
    pub command: String,
    /// Where it was found: `.anvil.toml`, `artisan dev`, `composer.json` or `package.json`.
    pub source: &'static str,
    /// A long-running dev server, as opposed to a one-off command.
    pub server: bool,
}

#[derive(Deserialize, Default)]
struct ProjectFile {
    #[serde(default)]
    processes: std::collections::BTreeMap<String, String>,
}

/// `.anvil.toml` that can't be read as TOML, so it isn't silently ignored.
pub fn project_file_error(site: &Site) -> Option<String> {
    let text = fs::read_to_string(site.path.join(".anvil.toml")).ok()?;
    toml::from_str::<ProjectFile>(&text)
        .err()
        .map(|e| format!(".anvil.toml: {}", e.message()))
}

/// An app's processes: `.anvil.toml`'s by name, a dev script's in its order.
pub fn detect(site: &Site) -> Vec<Process> {
    let php = home()
        .join("php")
        .join(site.php.as_deref().unwrap_or_default())
        .join("php");
    detect_in(site, &php, &home().join("cache"))
}

fn detect_in(site: &Site, php: &Path, cache: &Path) -> Vec<Process> {
    let own = fs::read_to_string(site.path.join(".anvil.toml"))
        .ok()
        .and_then(|t| toml::from_str::<ProjectFile>(&t).ok());
    if let Some(f) = own.filter(|f| !f.processes.is_empty()) {
        return f
            .processes
            .into_iter()
            .filter(|(n, c)| valid_name(n) && !c.trim().is_empty())
            .map(|(name, command)| Process {
                id: name.clone(),
                name,
                command,
                source: ".anvil.toml",
                server: false,
            })
            .collect();
    }
    // Apps with their own server already run their dev command; only PHP apps get these.
    if !apps::is_php(&site.kind) {
        return vec![];
    }
    if let Some(list) = laravel_dev_commands_cached(&site.path, php, cache) {
        return list
            .into_iter()
            .map(|(name, command)| Process {
                id: process_id(&name),
                name,
                command,
                source: "artisan dev",
                server: false,
            })
            .collect();
    }
    let json = |file: &str| -> serde_json::Value {
        fs::read_to_string(site.path.join(file))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    };
    let composer = json("composer.json");
    let dev = &composer["scripts"]["dev"];
    let scripts: Vec<&str> = match dev {
        serde_json::Value::String(s) => vec![s.as_str()],
        serde_json::Value::Array(a) => a.iter().filter_map(|v| v.as_str()).collect(),
        _ => vec![],
    };
    let split: Vec<Process> = scripts
        .iter()
        .find_map(|s| concurrently(s))
        .unwrap_or_default()
        .into_iter()
        .map(|(name, command)| Process {
            id: name.clone(),
            name,
            command,
            source: "composer.json",
            server: false,
        })
        .collect();
    if !split.is_empty() {
        return split;
    }
    // No dev script to split: Vite (or whatever `npm run dev` is) on its own.
    if json("package.json")["scripts"]["dev"].is_string() {
        return vec![Process {
            id: "vite".into(),
            name: "vite".into(),
            command: "npm run dev".into(),
            source: "package.json",
            server: false,
        }];
    }
    vec![]
}

const DEV_COMMANDS: &str = "vendor/laravel/framework/src/Illuminate/Foundation/DevCommands.php";

/// What `composer run dev` / `php artisan dev` would start, asked of Laravel 13 itself
/// (minus `server`, which Anvil already runs). `None` when it can't be asked.
pub fn laravel_dev_commands(dir: &Path, php: &Path) -> Option<Vec<(String, String)>> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    if !dir.join(DEV_COMMANDS).is_file() {
        return None;
    }
    let script = r#"require 'vendor/autoload.php'; $app = require 'bootstrap/app.php'; $app->make(Illuminate\Contracts\Console\Kernel::class)->bootstrap(); echo json_encode(array_map(fn($c) => ['name' => $c['name'], 'command' => $c['command']], \Illuminate\Foundation\DevCommands::commands()));"#;
    let mut child = Command::new(php)
        .args(["-r", script])
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        match child.try_wait().ok()? {
            Some(s) => break s,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    let mut out = String::new();
    child.stdout.take()?.read_to_string(&mut out).ok()?;
    if !status.success() {
        return None;
    }
    let list: Vec<serde_json::Value> = serde_json::from_str(out.trim()).ok()?;
    Some(
        list.iter()
            .filter_map(|c| Some((c["name"].as_str()?, c["command"].as_str()?)))
            .filter(|(n, _)| *n != "server")
            .map(|(n, c)| (n.to_string(), c.to_string()))
            .collect(),
    )
}

/// `laravel_dev_commands`, remembered until composer.lock or app/Providers changes
/// (booting Laravel costs a few hundred ms).
fn laravel_dev_commands_cached(
    dir: &Path,
    php: &Path,
    cache: &Path,
) -> Option<Vec<(String, String)>> {
    use std::hash::{Hash, Hasher};
    if !dir.join(DEV_COMMANDS).is_file() {
        return None;
    }
    let mtime = |p: PathBuf| {
        fs::metadata(p)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_nanos() as u64)
    };
    let stamp = [
        mtime(dir.join("composer.lock")),
        mtime(dir.join("app/Providers")),
    ];
    let mut h = std::collections::hash_map::DefaultHasher::new();
    dir.hash(&mut h);
    let file = cache.join(format!("devcommands-{:x}.json", h.finish()));
    // A failed boot is remembered too (`commands: None`), so it isn't retried every poll.
    #[derive(Serialize, Deserialize)]
    struct Cached {
        stamp: [u64; 2],
        php: PathBuf,
        commands: Option<Vec<(String, String)>>,
    }
    if let Some(c) = fs::read_to_string(&file)
        .ok()
        .and_then(|t| serde_json::from_str::<Cached>(&t).ok())
        .filter(|c| c.stamp == stamp && c.php == php)
    {
        return c.commands;
    }
    let commands = laravel_dev_commands(dir, php);
    let _ = fs::create_dir_all(cache);
    if let Ok(t) = serde_json::to_string(&Cached {
        stamp,
        php: php.into(),
        commands: commands.clone(),
    }) {
        let _ = fs::write(&file, t);
    }
    commands
}

/// A script name as an id: every char outside letters, digits, `-` and `_` becomes `-`.
pub fn process_id(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// Whether a package.json script is a long-running dev server rather than a
/// one-off command.
pub fn is_server_script(name: &str, command: &str) -> bool {
    // Secondary: needs a build first, or is a step of a `:dev` script.
    if name == "preview" || name.ends_with(":preview") || name.ends_with(":serve") {
        return false;
    }
    if command.trim_start().starts_with("start-server-and-test") {
        return false;
    }
    if matches!(name, "dev" | "start" | "serve") || name.ends_with(":dev") {
        return true;
    }
    let words = shell_words(command);
    let mut w = words.iter().map(String::as_str).peekable();
    // Skip the runner in front of the program.
    match w.next() {
        Some("npx" | "yarn") => {}
        Some("pnpm") if w.peek() == Some(&"exec") => {
            w.next();
        }
        Some(p) => return program_serves(p, w.next()),
        None => return false,
    }
    w.next().is_some_and(|p| program_serves(p, w.next()))
}

fn program_serves(program: &str, arg: Option<&str>) -> bool {
    match program {
        "vite" => !matches!(arg, Some("build" | "preview")),
        "vitepress" | "nuxt" | "next" | "astro" | "storybook" => arg == Some("dev"),
        _ => false,
    }
}

/// An app's package.json scripts: the dev servers as rows (in file order), and
/// the names of the rest, minus npm's lifecycle hooks.
pub fn package_scripts(dir: &Path) -> (Vec<Process>, Vec<String>) {
    let json: serde_json::Value = fs::read_to_string(dir.join("package.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let (mut servers, mut commands) = (vec![], vec![]);
    let empty = serde_json::Map::new();
    let scripts = json["scripts"].as_object().unwrap_or(&empty);
    for (name, command) in scripts {
        let Some(command) = command.as_str() else {
            continue;
        };
        // `pre<x>`/`post<x>` run around an existing script `<x>`; `preview` isn't one.
        let hook = ["pre", "post"].iter().any(|p| {
            name.strip_prefix(p)
                .is_some_and(|x| scripts.contains_key(x))
        }) || matches!(name.as_str(), "prepare" | "prepack" | "postinstall");
        if hook {
            continue;
        }
        if is_server_script(name, command) {
            servers.push(Process {
                id: process_id(name),
                name: name.clone(),
                command: format!("npm run {name}"),
                source: "package.json",
                server: true,
            });
        } else {
            commands.push(name.clone());
        }
    }
    (servers, commands)
}

/// Names become file names and ids: letters, digits, `-` and `_`.
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 40
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Concurrently's options that take a value as the next word.
const VALUE_FLAGS: [&str; 20] = [
    "-c",
    "--prefix-colors",
    "-n",
    "--names",
    "-p",
    "--prefix",
    "-l",
    "--prefix-length",
    "-t",
    "--timestamp-format",
    "-m",
    "--max-processes",
    "--restart-tries",
    "--restart-after",
    "-s",
    "--success",
    "--hide",
    "--name-separator",
    "--default-input-target",
    "--pad-prefix",
];

/// The named commands of `npx concurrently "a" "b" --names=x,y`, without
/// `php artisan serve`: Anvil already serves the site. `None` if it isn't one.
pub fn concurrently(script: &str) -> Option<Vec<(String, String)>> {
    let words = shell_words(script);
    let at = words.iter().position(|w| w == "concurrently")?;
    let (mut commands, mut names) = (vec![], vec![]);
    let mut rest = words[at + 1..].iter();
    while let Some(w) = rest.next() {
        // `concurrently … && echo done`: what follows isn't concurrently's.
        if matches!(w.as_str(), "&&" | "||" | ";" | "|" | "&") {
            break;
        }
        if let Some(flag) = w.strip_prefix('-').map(|_| w.as_str()) {
            let (flag, inline) = match flag.split_once('=') {
                Some((f, v)) => (f, Some(v.to_string())),
                None => (flag, None),
            };
            // `-kc red,blue`: grouped short flags take a value if the last one does.
            let grouped = !flag.starts_with("--") && flag.len() > 2;
            let takes = VALUE_FLAGS.contains(&flag)
                || (grouped && VALUE_FLAGS.contains(&&*format!("-{}", &flag[flag.len() - 1..])));
            let value = if takes {
                inline.or_else(|| rest.next().cloned())
            } else {
                None
            };
            if matches!(flag, "-n" | "--names") {
                names = value
                    .unwrap_or_default()
                    .split(',')
                    .map(str::to_string)
                    .collect();
            }
            continue;
        }
        // "npm:dev" is concurrently's shorthand for `npm run dev`.
        let cmd = match w.strip_prefix("npm:") {
            Some(script) => format!("npm run {script}"),
            None => w.trim_start_matches('@').to_string(),
        };
        commands.push(cmd);
    }
    let out: Vec<(String, String)> = commands
        .into_iter()
        .enumerate()
        .map(|(i, cmd)| {
            let name = names
                .get(i)
                .map(|n| n.trim().to_lowercase())
                .filter(|n| valid_name(n))
                .unwrap_or_else(|| format!("process-{}", i + 1));
            (name, cmd)
        })
        // Any form of `php … artisan serve`: Anvil is the server.
        .filter(|(_, cmd)| {
            !(cmd.starts_with("php")
                && cmd
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .windows(2)
                    .any(|w| w == ["artisan", "serve"]))
        })
        .collect();
    (!out.is_empty()).then_some(out)
}

/// Split like a POSIX shell: spaces separate words, quotes group, `\` escapes.
fn shell_words(s: &str) -> Vec<String> {
    let (mut words, mut cur, mut any) = (vec![], String::new(), false);
    let mut quote: Option<char> = None;
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('"'), '\\') => cur.extend(chars.next()),
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                any = true;
            }
            (None, '\\') => cur.extend(chars.next()),
            (None, c) if c.is_whitespace() => {
                if any || !cur.is_empty() {
                    words.push(std::mem::take(&mut cur));
                }
                any = false;
            }
            (None, c) => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        words.push(cur);
    }
    words
}

/// Key for the process manager, pid and log files: `<site>@<process>`. Site
/// names never contain `@`, so a key can't be another app's name.
pub fn key(site: &str, process: &str) -> String {
    format!("{site}@{process}")
}

pub fn log_file(site: &str, process: &str) -> PathBuf {
    apps::log_file(&key(site, process))
}

/// Anvil's PHP for the app and its newest Node.js, first on PATH, with Node's
/// caches kept in Anvil's folder.
pub fn env(site: &Site) -> Vec<(String, String)> {
    let mut env = vec![];
    let mut path = std::env::var("PATH").unwrap_or_default();
    // The project's .nvmrc / .node-version when installed, else the newest.
    if let Some(node) = apps::runtime_version("node", &site.path) {
        env = runtimes::env("node", &node);
        if let Some(i) = env.iter().position(|(k, _)| k == "PATH") {
            path = env.remove(i).1;
        }
    }
    if let Some(v) = &site.php {
        path = format!("{}:{path}", home().join("php").join(v).display());
    }
    env.push(("PATH".into(), path));
    env
}

#[cfg(test)]
mod tests {
    use super::*;

    const LARAVEL: &str = r##"npx concurrently -c "#93c5fd,#c4b5fd,#fb7185,#fdba74" "php artisan serve --host=localhost" "php artisan queue:listen --tries=1 --timeout=0" "php artisan pail --timeout=0" "npm run dev" --names=server,queue,logs,vite --kill-others"##;

    #[test]
    fn splits_laravels_dev_script() {
        let p = concurrently(LARAVEL).unwrap();
        assert_eq!(
            p,
            [
                (
                    "queue".into(),
                    "php artisan queue:listen --tries=1 --timeout=0".into()
                ),
                ("logs".into(), "php artisan pail --timeout=0".into()),
                ("vite".into(), "npm run dev".into()),
            ]
        );
    }

    #[test]
    fn names_flags_and_shorthands() {
        let p = concurrently(r#"concurrently -n "Web,Worker" -k "npm:dev" '@php artisan horizon'"#)
            .unwrap();
        assert_eq!(
            p,
            [
                ("web".into(), "npm run dev".into()),
                ("worker".into(), "php artisan horizon".into()),
            ]
        );
        // Missing or odd names get a numbered one.
        let p = concurrently(r#"concurrently "a" "b" --names=ok,"no way""#).unwrap();
        assert_eq!(p[0].0, "ok");
        assert_eq!(p[1].0, "process-2");
        assert!(concurrently("@php artisan dev").is_none());
        // Shell operators end the list; grouped flags take their value; any serve is dropped.
        let p = concurrently(r#"concurrently -kc red,blue "php -d x=1 artisan serve" "npm run dev" -n a,b && echo done"#).unwrap();
        assert_eq!(p, [("b".into(), "npm run dev".into())]);
        assert!(concurrently("concurrently \"php artisan serve\"").is_none());
    }

    #[test]
    fn shell_quoting() {
        assert_eq!(
            shell_words(r#"a "b c" 'd "e"' f\ g """#),
            ["a", "b c", r#"d "e""#, "f g", ""]
        );
    }

    #[test]
    fn package_scripts_split_servers_from_commands() {
        // vuefinder's real scripts.
        let pkg = r#"{"scripts": {
    "dev": "vite",
    "build": "run-s \"build-only {@}\" type-gen --",
    "preview": "vite preview",
    "test:unit": "vitest",
    "prepare": "cypress install",
    "prepack": "npm run build",
    "test:e2e": "start-server-and-test preview http://localhost:4173 'cypress run --e2e'",
    "test:e2e:dev": "start-server-and-test 'vite dev --port 4173' http://localhost:4173 'cypress open --e2e'",
    "build-only": "vite build",
    "type-check": "vue-tsc --noEmit -p tsconfig.app.json",
    "type-gen": "vue-tsc -p tsconfig.app.json",
    "lint": "oxlint",
    "lint:fix": "oxlint --fix",
    "format": "prettier --write .",
    "format:check": "prettier --check .",
    "docs:dev": "run-s build-only docs:serve",
    "docs:serve": "vitepress dev docs",
    "docs:build": "npm run build && vitepress build docs",
    "docs:preview": "vitepress preview docs"
  }}"#;
        let dir = std::env::temp_dir().join(format!("anvil-pkg-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("package.json"), pkg).unwrap();
        let (servers, commands) = package_scripts(&dir);
        fs::remove_dir_all(&dir).unwrap();
        let names: Vec<_> = servers.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["dev", "docs:dev"]);
        assert_eq!(servers[1].id, "docs-dev");
        assert_eq!(servers[1].command, "npm run docs:dev");
        assert!(
            servers
                .iter()
                .all(|p| p.server && p.source == "package.json")
        );
        for c in [
            "build",
            "test:unit",
            "lint",
            "format",
            "type-check",
            "preview",
            "docs:serve",
            "docs:preview",
            "test:e2e",
            "test:e2e:dev",
        ] {
            assert!(commands.iter().any(|n| n == c), "{c} should be a command");
        }
        assert!(!commands.iter().any(|n| n == "prepare" || n == "prepack"));
        assert_eq!(process_id("test:e2e:dev"), "test-e2e-dev");
        let mut ids: Vec<_> = servers
            .iter()
            .map(|p| p.name.as_str())
            .chain(commands.iter().map(String::as_str))
            .map(process_id)
            .collect();
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n);
        assert!(is_server_script("web", "npx vite --port 3000"));
        assert!(!is_server_script("web", "vite build"));
        assert!(is_server_script("sb", "pnpm exec storybook dev -p 6006"));
    }

    fn site(dir: &std::path::Path, kind: &str) -> Site {
        Site {
            name: "shop".into(),
            host: String::new(),
            secure: false,
            path: dir.into(),
            root: dir.into(),
            php: Some("8.4".into()),
            php_pinned: false,
            ini: Default::default(),
            linked: false,
            parked_in: None,
            modified: 0,
            kind: kind.into(),
            kind_detected: kind.into(),
            command: None,
            command_custom: false,
            port: None,
            autostart: false,
            runtime_version: None,
            runtime_pinned: false,
        }
    }

    #[test]
    fn project_file_comes_first_then_dev_scripts() {
        let dir = std::env::temp_dir().join(format!("anvil-proc-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let composer = serde_json::json!({ "scripts": { "dev": ["Composer\\Config::disableProcessTimeout", LARAVEL] } });
        fs::write(dir.join("composer.json"), composer.to_string()).unwrap();
        fs::write(dir.join("package.json"), r#"{"scripts":{"dev":"vite"}}"#).unwrap();
        let names = |s: &Site| detect(s).into_iter().map(|p| p.name).collect::<Vec<_>>();
        assert_eq!(names(&site(&dir, "laravel")), ["queue", "logs", "vite"]);
        // Not a PHP app: its own dev server is the app itself.
        assert!(detect(&site(&dir, "next")).is_empty());
        // `php artisan dev` can't be split: Vite from package.json.
        fs::write(
            dir.join("composer.json"),
            r#"{"scripts":{"dev":["@php artisan dev"]}}"#,
        )
        .unwrap();
        assert_eq!(detect(&site(&dir, "laravel"))[0].command, "npm run dev");
        // .anvil.toml wins, for any kind; bad names are skipped.
        fs::write(
            dir.join(".anvil.toml"),
            "[processes]\nworker = \"node worker.js\"\n\"bad name\" = \"x\"\n",
        )
        .unwrap();
        let p = detect(&site(&dir, "next"));
        assert_eq!((p.len(), p[0].source), (1, ".anvil.toml"));
        fs::remove_dir_all(dir).unwrap();
    }

    /// A `php` that ignores its arguments and runs `body`, in its own temp dir.
    fn fake_php(tag: &str, body: &str) -> (PathBuf, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("anvil-lv-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("app/vendor/laravel/framework/src/Illuminate/Foundation"))
            .unwrap();
        fs::write(root.join("app").join(DEV_COMMANDS), "<?php").unwrap();
        let php = root.join("php");
        fs::write(&php, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&php, fs::Permissions::from_mode(0o755)).unwrap();
        (root.join("app"), php)
    }

    const DEV_JSON: &str = r#"[{"name":"server","command":"php artisan serve"},{"name":"queue","command":"php artisan queue:listen --tries=1 --timeout=0"},{"name":"logs","command":"php artisan pail --timeout=0"},{"name":"vite","command":"pnpm run dev"}]"#;

    #[test]
    fn laravel_list_drops_server() {
        let (dir, php) = fake_php("ok", &format!("echo '{DEV_JSON}'"));
        let want: Vec<(String, String)> = vec![
            (
                "queue".into(),
                "php artisan queue:listen --tries=1 --timeout=0".into(),
            ),
            ("logs".into(), "php artisan pail --timeout=0".into()),
            ("vite".into(), "pnpm run dev".into()),
        ];
        assert_eq!(laravel_dev_commands(&dir, &php), Some(want.clone()));
        // Cached: a second ask doesn't need PHP, and detect uses it as the second source.
        let cache = dir.parent().unwrap().join("cache");
        let mut s = site(&dir, "laravel");
        s.php = Some("8.4".into());
        let names = |s: &Site| detect_in(s, &php, &cache);
        let p = names(&s);
        assert_eq!(
            p.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
            ["queue", "logs", "vite"]
        );
        assert_eq!(
            (p[0].source, p[0].server, p[0].id.as_str()),
            ("artisan dev", false, "queue")
        );
        fs::write(&php, "#!/bin/sh\nexit 1\n").unwrap();
        assert_eq!(laravel_dev_commands_cached(&dir, &php, &cache), Some(want));
        // .anvil.toml still comes first.
        fs::write(dir.join(".anvil.toml"), "[processes]\nw = \"x\"\n").unwrap();
        assert_eq!(detect_in(&s, &php, &cache)[0].source, ".anvil.toml");
        fs::remove_dir_all(dir.parent().unwrap()).unwrap();
    }

    #[test]
    fn laravel_list_falls_back() {
        let (dir, php) = fake_php("bad", "exit 1");
        assert_eq!(laravel_dev_commands(&dir, &php), None);
        fs::write(&php, "#!/bin/sh\necho not json\n").unwrap();
        assert_eq!(laravel_dev_commands(&dir, &php), None);
        fs::write(&php, "#!/bin/sh\nexec sleep 10\n").unwrap();
        let t = std::time::Instant::now();
        assert_eq!(laravel_dev_commands(&dir, &php), None);
        assert!(t.elapsed().as_secs() < 6);
        // No DevCommands.php: PHP isn't run at all.
        let marker = dir.parent().unwrap().join("ran");
        fs::write(&php, format!("#!/bin/sh\ntouch {}\n", marker.display())).unwrap();
        fs::remove_file(dir.join(DEV_COMMANDS)).unwrap();
        assert_eq!(laravel_dev_commands(&dir, &php), None);
        assert!(!marker.exists());
        fs::remove_dir_all(dir.parent().unwrap()).unwrap();
    }

    #[test]
    fn laravel_failure_is_cached() {
        let (dir, php) = fake_php("cnt", "");
        let root = dir.parent().unwrap().to_path_buf();
        let log = root.join("runs");
        fs::write(
            &php,
            format!("#!/bin/sh\necho x >> {}\nexit 1\n", log.display()),
        )
        .unwrap();
        let cache = root.join("cache");
        let runs = || fs::read_to_string(&log).map_or(0, |t| t.lines().count());
        assert_eq!(laravel_dev_commands_cached(&dir, &php, &cache), None);
        assert_eq!(laravel_dev_commands_cached(&dir, &php, &cache), None);
        assert_eq!(runs(), 1);
        // Another PHP binary retries.
        let other = root.join("php2");
        fs::copy(&php, &other).unwrap();
        assert_eq!(laravel_dev_commands_cached(&dir, &other, &cache), None);
        assert_eq!(runs(), 2);
        fs::remove_dir_all(root).unwrap();
    }
}
