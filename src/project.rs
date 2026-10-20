//! A project's own `.anvil.toml`: its dev processes, the PHP and Node.js versions and
//! the services it needs, so whoever clones it gets the same setup. Anvil only reads it.
//!
//! ```toml
//! php = "8.4"
//! node = "22"
//!
//! [services]
//! mysql = "8.4"   # "" means any version
//! redis = ""
//!
//! [processes]
//! queue = "php artisan queue:work"
//!
//! [worktree]      # see `worktrees::Hooks`
//! copy = [".env"]
//! ```

use crate::{Config, Site, services};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[derive(Deserialize, Default, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProjectFile {
    #[serde(default)]
    pub php: Option<String>,
    #[serde(default)]
    pub node: Option<String>,
    /// Service kind -> version ("" for any).
    #[serde(default)]
    pub services: BTreeMap<String, String>,
    #[serde(default)]
    pub processes: BTreeMap<String, String>,
    /// Worktree hooks shared with the team (`worktrees::Hooks`).
    #[serde(default)]
    pub worktree: Option<crate::worktrees::Hooks>,
}

/// The file's text checked: TOML, known keys, kinds and versions.
pub fn parse(text: &str) -> Result<ProjectFile, String> {
    let f: ProjectFile = toml::from_str(text).map_err(|e| e.message().trim().to_string())?;
    if let Some(v) = &f.php {
        crate::php_offered(v).map_err(|e| format!("php = \"{v}\": {e}"))?;
    }
    if let Some(v) = &f.node
        && (v.is_empty() || !v.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(format!("node = \"{v}\": use a major version, e.g. \"22\""));
    }
    for (kind, v) in &f.services {
        if !services::SERVICES.contains(&kind.as_str()) {
            return Err(format!(
                "unknown service {kind}: use {}",
                services::SERVICES.join(", ")
            ));
        }
        if offered(kind, v).is_none() {
            return Err(format!(
                "{kind} = \"{v}\": Anvil offers {} {}",
                services::name(kind),
                services::versions(kind).join(", ")
            ));
        }
    }
    if let Some(h) = &f.worktree {
        crate::worktrees::check_hooks(h).map_err(|e| format!("[worktree] {e}"))?;
    }
    Ok(f)
}

/// `dir`'s `.anvil.toml`: None when there is none, an error when it can't be used.
pub fn read(dir: &Path) -> Option<Result<ProjectFile, String>> {
    let text = fs::read_to_string(dir.join(".anvil.toml")).ok()?;
    Some(parse(&text))
}

/// The usable file only.
pub fn load(dir: &Path) -> Option<ProjectFile> {
    read(dir)?.ok()
}

/// `.anvil.toml: …` for a file that can't be used, so it isn't silently ignored.
pub fn error(dir: &Path) -> Option<String> {
    read(dir)?.err().map(|e| format!(".anvil.toml: {e}"))
}

/// Whether a service version satisfies what the file asks ("" any, "8.4" any 8.4.x).
fn matches(version: &str, want: &str) -> bool {
    want.is_empty() || version == want || version.starts_with(&format!("{want}."))
}

/// The version Anvil would add for `want`: the newest offered match, the default for "".
pub fn offered(kind: &str, want: &str) -> Option<&'static str> {
    if want.is_empty() {
        return Some(services::default_version(kind));
    }
    services::versions(kind)
        .iter()
        .copied()
        .find(|v| matches(v, want))
}

/// The PHP version a project runs: the user's choice in Anvil, then `.anvil.toml`'s
/// when it is installed, then `default`. Sites, PHP-FPM pools and the shims use it.
pub fn php_version(
    pin: Option<String>,
    dir: &Path,
    installed: &[String],
    default: Option<String>,
) -> Option<String> {
    pin.or_else(|| load(dir)?.php.filter(|v| installed.contains(v)))
        .or(default)
}

/// The file's Node.js major, when it is installed.
pub fn node(dir: &Path, installed: &[String]) -> Option<String> {
    load(dir)?.node.filter(|v| installed.contains(v))
}

/// One thing the file asks for.
#[derive(Serialize, Debug, PartialEq, Clone)]
pub struct Need {
    /// "php", "node" or a service kind.
    pub kind: String,
    /// As the file says it ("" for any version).
    pub version: String,
    /// What installing or adding gets: the PHP or Node.js version, the service version.
    pub install: String,
    /// "PHP 8.3", "Redis", "MySQL 8.4".
    pub label: String,
    /// Installed, or a service of this kind and version is added.
    pub met: bool,
    /// The version the user chose in Anvil instead (PHP and Node.js only).
    pub overridden: Option<String>,
}

impl Need {
    /// Shown as missing: not there, and not replaced by the user's own choice.
    pub fn missing(&self) -> bool {
        !self.met && self.overridden.is_none()
    }
}

/// What the file asks for against what Anvil has: the user's PHP and Node.js
/// choices, the installed versions and the added services as (kind, version).
pub fn needs_in(
    f: &ProjectFile,
    pins: (Option<&str>, Option<&str>),
    php: &[String],
    node: &[String],
    added: &[(&str, String)],
) -> Vec<Need> {
    let runtime =
        |kind: &str, label: &str, want: &Option<String>, pin: Option<&str>, have: &[String]| {
            want.as_ref().map(|v| Need {
                kind: kind.into(),
                version: v.clone(),
                install: v.clone(),
                label: format!("{label} {v}"),
                met: have.contains(v),
                overridden: pin.filter(|p| p != v).map(String::from),
            })
        };
    let mut out: Vec<Need> = [
        runtime("php", "PHP", &f.php, pins.0, php),
        runtime("node", "Node.js", &f.node, pins.1, node),
    ]
    .into_iter()
    .flatten()
    .collect();
    for (kind, want) in &f.services {
        let name = services::name(kind);
        out.push(Need {
            kind: kind.clone(),
            version: want.clone(),
            install: offered(kind, want).unwrap_or_default().into(),
            label: if want.is_empty() {
                name.into()
            } else {
                format!("{name} {want}")
            },
            met: added.iter().any(|(k, v)| k == kind && matches(v, want)),
            overridden: None,
        });
    }
    out
}

/// What the site's `.anvil.toml` asks for; empty without a usable file.
pub fn needs(cfg: &Config, site: &Site) -> Vec<Need> {
    let Some(f) = load(&site.path) else {
        return vec![];
    };
    let php_pin = cfg.sites.get(&site.name).and_then(|s| s.php.as_deref());
    let node_pin = (site.runtime_pinned && crate::apps::runtime(&site.kind) == Some("node"))
        .then_some(site.runtime_version.as_deref())
        .flatten();
    let added: Vec<(&str, String)> = services::instances(cfg)
        .iter()
        .filter_map(|id| Some((services::kind_of(cfg, id).ok()?, services::version(cfg, id))))
        .collect();
    needs_in(
        &f,
        (php_pin, node_pin),
        &crate::installed_php(),
        &crate::runtimes::installed("node"),
        &added,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "php = \"8.4\"\nnode = \"22\"\n\n[services]\nmysql = \"8.4\"\nredis = \"\"\n\n[processes]\nqueue = \"php artisan queue:work\"\n";

    #[test]
    fn parses_a_good_file() {
        let f = parse(GOOD).unwrap();
        assert_eq!(f.php.as_deref(), Some("8.4"));
        assert_eq!(f.node.as_deref(), Some("22"));
        assert_eq!(f.services["mysql"], "8.4");
        assert_eq!(f.services["redis"], "");
        assert_eq!(f.processes["queue"], "php artisan queue:work");
        // Processes alone, as before.
        assert!(parse("[processes]\nw = \"x\"\n").is_ok());
    }

    #[test]
    fn refuses_unknown_keys_kinds_and_bad_versions() {
        for (text, says) in [
            ("ruby = \"3\"\n", "unknown field `ruby`"),
            ("[services]\nmongo = \"\"\n", "unknown service mongo"),
            ("php = \"7.4\"\n", "php = \"7.4\""),
            (
                "php = \"8.0\"\n",
                "php = \"8.0\": PHP 8.0 is end of life — Anvil offers PHP 8.5, 8.4, 8.3, 8.2, 8.1",
            ),
            ("node = \"lts\"\n", "node = \"lts\""),
            ("[services]\nmysql = \"5.7\"\n", "mysql = \"5.7\""),
            ("[services]\nredis = \"8.1\"\n", "redis = \"8.1\""),
        ] {
            let e = parse(text).unwrap_err();
            assert!(e.contains(says), "{text}: {e}");
        }
        let dir = std::env::temp_dir().join(format!("anvil-project-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(".anvil.toml"), "php = \"7.4\"\n").unwrap();
        assert!(error(&dir).unwrap().starts_with(".anvil.toml: php"));
        assert!(load(&dir).is_none());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn php_is_the_users_choice_then_the_file_then_the_default() {
        let dir = std::env::temp_dir().join(format!("anvil-project-php-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let installed = ["8.3".to_string(), "8.4".to_string()];
        let pick = |pin: Option<&str>| {
            php_version(pin.map(Into::into), &dir, &installed, Some("8.4".into()))
        };
        assert_eq!(pick(None).as_deref(), Some("8.4"));
        fs::write(dir.join(".anvil.toml"), "php = \"8.3\"\n").unwrap();
        assert_eq!(pick(None).as_deref(), Some("8.3"));
        assert_eq!(pick(Some("8.2")).as_deref(), Some("8.2"));
        // Not installed: the default until it is.
        fs::write(dir.join(".anvil.toml"), "php = \"8.1\"\n").unwrap();
        assert_eq!(pick(None).as_deref(), Some("8.4"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn versions_match_on_dot_boundaries() {
        assert_eq!(offered("mysql", "8.4"), Some("8.4.11"));
        assert_eq!(offered("mysql", "8"), Some("8.4.11"));
        assert_eq!(offered("mysql", ""), Some("8.4.11"));
        assert_eq!(offered("redis", "8.10.2"), Some("8.10.2"));
        assert_eq!(offered("mysql", "8.4.1"), None);
    }

    #[test]
    fn says_whats_met_missing_and_overridden() {
        let f = parse(GOOD).unwrap();
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        // Nothing there: everything is missing, with what adding would get.
        let n = needs_in(&f, (None, None), &[], &[], &[]);
        let labels: Vec<_> = n
            .iter()
            .filter(|n| n.missing())
            .map(|n| n.label.as_str())
            .collect();
        assert_eq!(labels, ["PHP 8.4", "Node.js 22", "MySQL 8.4", "Redis"]);
        assert_eq!(n[2].install, "8.4.11");
        assert_eq!(n[3].install, "8.10.2");
        // Installed and added, any app's service counting; the wrong MySQL doesn't.
        let added = [
            ("mysql", "8.0.46".to_string()),
            ("redis", "7.2.16".to_string()),
        ];
        let n = needs_in(&f, (None, None), &s(&["8.4"]), &s(&["22"]), &added);
        let missing: Vec<_> = n
            .iter()
            .filter(|n| n.missing())
            .map(|n| n.label.as_str())
            .collect();
        assert_eq!(missing, ["MySQL 8.4"]);
        let added = [
            ("mysql", "8.4.11".to_string()),
            ("redis", "7.2.16".to_string()),
        ];
        let n = needs_in(&f, (None, None), &s(&["8.4"]), &s(&["22"]), &added);
        assert!(n.iter().all(|n| !n.missing()));
        // The user's own PHP choice replaces the file's: not missing, though not installed.
        let n = needs_in(&f, (Some("8.3"), None), &s(&["8.3"]), &s(&["22"]), &added);
        assert_eq!(
            (n[0].met, n[0].overridden.as_deref(), n[0].missing()),
            (false, Some("8.3"), false)
        );
    }
}
