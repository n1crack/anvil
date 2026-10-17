//! The other processes an app runs while you work on it: a queue worker, a log
//! tailer, Vite. Read from `.anvil.toml`, else from the project's own dev script
//! (`composer run dev` with concurrently, or package.json's `dev`).

use crate::{Site, apps, home, runtimes};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Process {
    pub name: String,
    pub command: String,
    /// Where it was found: `.anvil.toml`, `composer.json` or `package.json`.
    pub source: &'static str,
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
    let own = fs::read_to_string(site.path.join(".anvil.toml"))
        .ok()
        .and_then(|t| toml::from_str::<ProjectFile>(&t).ok());
    if let Some(f) = own.filter(|f| !f.processes.is_empty()) {
        return f
            .processes
            .into_iter()
            .filter(|(n, c)| valid_name(n) && !c.trim().is_empty())
            .map(|(name, command)| Process {
                name,
                command,
                source: ".anvil.toml",
            })
            .collect();
    }
    // Apps with their own server already run their dev command; only PHP apps get these.
    if !apps::is_php(&site.kind) {
        return vec![];
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
            name,
            command,
            source: "composer.json",
        })
        .collect();
    if !split.is_empty() {
        return split;
    }
    // No dev script to split: Vite (or whatever `npm run dev` is) on its own.
    if json("package.json")["scripts"]["dev"].is_string() {
        return vec![Process {
            name: "vite".into(),
            command: "npm run dev".into(),
            source: "package.json",
        }];
    }
    vec![]
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

    fn site(dir: &std::path::Path, kind: &str) -> Site {
        Site {
            name: "shop".into(),
            host: String::new(),
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
}
