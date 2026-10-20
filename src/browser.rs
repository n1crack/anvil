//! The browser bridge: an app's page errors from the person's own browser, for agents.
//!
//! With an app's "Browser errors" switch on (`SiteConfig::browser`), PHP prepends
//! `prepend.php` to its requests (pool-wide `auto_prepend_file`; the app's site block
//! sets `ANVIL_INJECT` so only it acts); for that app's page loads it loads `inject.php`,
//! which puts `/__anvil/a.js` before `</head>`. PHP without OPcache compiles the prepend
//! on every request, so it stays a few lines. An app with a server of its own (a dev
//! server, a server row, Octane) gets the same tag from `proxy`, which Caddy sends only
//! its page loads to (`doc_route`).
//! The script sends batches of events with `sendBeacon` to `/__anvil/e?p=<page>&d=<JSON>`
//! on the page's own origin; Caddy answers 204 and writes the request line, without
//! headers or addresses, to `log/browser/<app>.log` (rolled at 1 MiB, one older copy,
//! 24 h). This module reads those lines back as grouped errors and page loads.

use crate::{Site, apps, requests};
use anyhow::Result;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub mod proxy;

const A_JS: &str = include_str!("browser/a.js");
const PREPEND: &str = include_str!("browser/prepend.php");
const INJECT: &str = include_str!("browser/inject.php");
/// Stack frames kept per group.
const FRAMES: usize = 5;
/// The response header `inject.php` adds when the page's CSP keeps the script out.
const BLOCKED: &str = "X-Anvil-Bridge";

/// Changes with the script, so a browser never keeps an old one (`a.js?v=`).
pub fn version() -> String {
    crate::short_hash(A_JS)
}

/// Where `a.js`, `prepend.php` and `inject.php` are written (`write_files`).
pub fn dir() -> PathBuf {
    crate::home().join("inject")
}

pub fn prepend_file() -> PathBuf {
    dir().join("prepend.php")
}

/// An app's browser log.
pub fn file(site: &str) -> PathBuf {
    crate::home()
        .join("log")
        .join("browser")
        .join(format!("{site}.log"))
}

/// The bridge is on for this app: its switch, and PHP serves it or Caddy passes it on to
/// a server of its own (a dev server; Octane, `doc_route`). Static sites stay out.
pub fn on(site: &Site) -> bool {
    site.browser && (php(site) || (apps::runs_process(&site.kind) && site.port.is_some()))
}

fn php(site: &Site) -> bool {
    apps::is_php(&site.kind) && site.php.is_some()
}

/// Any app's bridge is on: Anvil writes the script for Caddy to serve. Off for all,
/// nothing changes.
pub fn any(sites: &[Site]) -> bool {
    sites.iter().any(on)
}

/// An app with its bridge on is passed on to a server of its own (`doc_route`): the
/// proxy then runs, with the web server.
pub fn proxied(cfg: &crate::Config, sites: &[Site]) -> bool {
    sites.iter().filter(|s| on(s)).any(|s| {
        apps::runs_process(&s.kind)
            || crate::processes::octane_row(cfg, s)
                .and_then(|p| p.port)
                .is_some()
    })
}

/// The script tag in a page, as `inject.php` puts it in.
pub fn tag() -> String {
    format!("<script src=\"/__anvil/a.js?v={}\"></script>", version())
}

/// A site block's lines that send its page loads (a GET or POST asking for HTML first;
/// not a websocket or a `fetch`) to Anvil's proxy, which asks the app at `port`. They
/// go before the block's own `reverse_proxy` to that port, which takes the rest.
pub fn doc_route(port: u16) -> String {
    let addr = proxy::address();
    if addr.contains(['"', '\\', '\n', '\r']) {
        return String::new();
    }
    format!(
        "\t@anvil_doc {{\n\t\tmethod GET POST\n\t\theader_regexp Accept ^text/html\n\t\tnot header Upgrade *\n\t\tnot header Sec-Fetch-Dest empty\n\t}}\n\treverse_proxy @anvil_doc \"{addr}\" {{\n\t\theader_up {} 127.0.0.1:{port}\n\t}}\n",
        proxy::UPSTREAM
    )
}

/// Turn an app's switch on or off. A worktree keeps its own choice; an app's off is unset.
pub fn set(cfg: &mut crate::Config, site: &str, on: bool) -> Result<()> {
    let name = crate::site_name(site);
    let Some(s) = crate::sites(cfg).into_iter().find(|s| s.name == name) else {
        anyhow::bail!("No site named {site}");
    };
    if on && !apps::is_php(&s.kind) && !apps::runs_process(&s.kind) {
        anyhow::bail!(
            "Browser errors works for PHP apps and dev servers; {name} is a {} app",
            apps::label(&s.kind)
        );
    }
    let worktree = s.worktree_of.is_some();
    cfg.sites.entry(name).or_default().browser = (on || worktree).then_some(on);
    Ok(())
}

/// Write `a.js`, `prepend.php` and `inject.php` as this Anvil has them, when they differ.
pub fn write_files() -> Result<()> {
    let d = dir();
    std::fs::create_dir_all(&d)?;
    std::fs::create_dir_all(crate::home().join("log").join("browser"))?;
    let inject = INJECT.replace("__VERSION__", &version());
    for (name, body) in [
        ("a.js", A_JS),
        ("prepend.php", PREPEND),
        ("inject.php", &inject),
    ] {
        let p = d.join(name);
        if std::fs::read_to_string(&p).ok().as_deref() != Some(body) {
            std::fs::write(&p, body)?;
        }
    }
    Ok(())
}

/// php.ini values for a pool: with any bridge on, `auto_prepend_file` is Anvil's, and the
/// user's own (global or the app's) goes to `ANVIL_PREPEND`, which the prepend includes.
pub fn pool_ini(
    sites: &[Site],
    mut ini: BTreeMap<String, String>,
) -> (BTreeMap<String, String>, Option<String>) {
    if !sites.iter().any(|s| s.browser && php(s)) {
        return (ini, None);
    }
    let theirs = ini
        .insert(
            "auto_prepend_file".into(),
            prepend_file().display().to_string(),
        )
        .map(|v| v.trim_matches('"').to_string())
        .filter(|v| !v.is_empty() && !v.eq_ignore_ascii_case("none"));
    (ini, theirs)
}

/// Caddy's global named logger for an app's browser log: the request line only, no
/// headers, addresses or TLS details.
pub fn logger(site: &str) -> Option<String> {
    let path = crate::caddy_path(&file(site));
    if path.contains(['"', '\\', '\n', '\r']) {
        return None;
    }
    Some(format!(
        "\tlog {name} {{\n\t\toutput file \"{path}\" {{\n\t\t\troll_size 1MiB\n\t\t\troll_keep 1\n\t\t\troll_keep_for 24h\n\t\t}}\n\t\tformat filter {{\n\t\t\twrap json\n\t\t\tfields {{\n\t\t\t\trequest>headers delete\n\t\t\t\trequest>remote_ip delete\n\t\t\t\trequest>remote_port delete\n\t\t\t\trequest>client_ip delete\n\t\t\t\trequest>tls delete\n\t\t\t\tresp_headers delete\n\t\t\t\tuser_id delete\n\t\t\t}}\n\t\t}}\n\t\tinclude http.log.access.{name}\n\t}}\n",
        name = logger_name(site)
    ))
}

fn logger_name(site: &str) -> String {
    // A dot would make it a child of another app's logger (`my` and `my.app`); site names
    // never hold `_`, so this stays one name per app.
    format!("anvil_browser_{}", site.replace('.', "_"))
}

/// A site block's collector and script, before PHP: neither is in the requests log.
pub fn site_lines(site: &str) -> String {
    let dir = crate::caddy_path(&dir());
    if dir.contains(['"', '\\', '\n', '\r']) {
        return String::new();
    }
    format!(
        "\thandle /__anvil/e {{\n\t\tlog_name {}\n\t\trespond 204\n\t}}\n\thandle /__anvil/a.js {{\n\t\tlog_skip\n\t\troot * \"{dir}\"\n\t\trewrite * /a.js\n\t\theader Cache-Control \"public, max-age=31536000, immutable\"\n\t\tfile_server\n\t}}\n",
        logger_name(site)
    )
}

/// One kind of problem on the app's pages: the same kind, message and place.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Group {
    /// error, rejection, console, fetch, xhr, resource, csp, deprecation, intervention
    pub kind: String,
    /// error or warning
    pub level: String,
    pub message: String,
    /// `url:line:col` of a script error, or the URL a request or resource failed at.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u64>,
    /// How many times, repeats on a page included.
    pub count: u64,
    /// Seconds since 1970.
    pub first: f64,
    pub last: f64,
    /// The page (path and query) it last happened on.
    pub page: String,
    /// The top stack frames, Anvil's own left out.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub frames: Vec<String>,
}

/// A page load as the browser timed it, in ms from the start of the navigation.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Load {
    pub ts: f64,
    pub page: String,
    pub status: Option<u64>,
    pub ttfb: Option<u64>,
    pub dom_ready: Option<u64>,
    pub load: Option<u64>,
}

#[derive(Serialize, Debug, Default)]
pub struct Report {
    /// Most recently seen last.
    pub groups: Vec<Group>,
    /// Oldest first.
    pub loads: Vec<Load>,
    /// Where the next read starts (`read`'s `from`).
    pub end: u64,
}

impl Report {
    pub fn errors(&self) -> impl Iterator<Item = &Group> {
        self.groups.iter().filter(|g| g.level == "error")
    }
}

/// `error` or `warning` for an event; `None` for what isn't a problem.
fn level(kind: &str, console: Option<&str>) -> Option<&'static str> {
    Some(match kind {
        "error" | "rejection" | "fetch" | "xhr" | "resource" | "csp" => "error",
        "console" if console == Some("error") => "error",
        "console" | "deprecation" | "intervention" => "warning",
        _ => return None,
    })
}

/// `%XX` and `+` decoded; what isn't UTF-8 is replaced.
fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let hex = |i: usize| b.get(i).and_then(|&c| (c as char).to_digit(16));
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match (b[i], hex(i + 1), hex(i + 2)) {
            (b'%', Some(h), Some(l)) => {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
            (b'+', ..) => out.push(b' '),
            (c, ..) => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A beacon's page id and events; `None` for any other line.
fn beacon(uri: &str) -> Option<(String, Vec<serde_json::Value>)> {
    let query = uri.strip_prefix("/__anvil/e?")?;
    let (mut page, mut data) = (String::new(), None);
    for pair in query.split('&') {
        match pair.split_once('=') {
            Some(("p", v)) => page = unescape(v),
            Some(("d", v)) => data = serde_json::from_str(&unescape(v)).ok(),
            _ => {}
        }
    }
    Some((page, data?))
}

/// The frames of a stack that aren't Anvil's: Chrome's `at …`, Firefox's and Safari's `fn@url`.
fn frames(stack: &str) -> Vec<String> {
    stack
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("at ") || l.contains('@'))
        .filter(|l| !l.contains("/__anvil/a.js"))
        .take(FRAMES)
        .map(|l| l.chars().take(300).collect())
        .collect()
}

/// The events written since `from` (0: the whole log), grouped. A log that shrank (Caddy
/// rolled it) is read from its start.
pub fn read(path: &Path, from: u64) -> Result<Report> {
    let all = requests::Filter {
        assets: true,
        ..Default::default()
    };
    let page = requests::tail(path, from, &all)?;
    let mut groups: Vec<Group> = Vec::new();
    let mut at: BTreeMap<(String, String, String, String), usize> = BTreeMap::new();
    // Per page and group: the most times a repeat said it happened.
    let mut repeats: BTreeMap<(String, usize), u64> = BTreeMap::new();
    let mut loads = Vec::new();
    // `tail` gives newest first.
    for r in page.requests.iter().rev() {
        let Some((pid, events)) = beacon(&r.uri) else {
            continue;
        };
        for e in events {
            let s = |k: &str| e[k].as_str().unwrap_or_default().to_string();
            let n = |k: &str| e[k].as_u64();
            let ts = n("t").map_or(r.ts, |t| t as f64 / 1000.0);
            let kind = s("k");
            if kind == "load" {
                loads.push(Load {
                    ts,
                    page: s("at"),
                    status: n("status").filter(|s| *s > 0),
                    ttfb: n("ttfb"),
                    dom_ready: n("dom"),
                    load: n("load"),
                });
                continue;
            }
            let (kind, repeat) = match kind.as_str() {
                "repeat" => (s("of"), n("n")),
                _ => (kind, None),
            };
            let console = e["level"].as_str();
            let Some(lvl) = level(&kind, console) else {
                continue;
            };
            let url = s("url");
            let request = matches!(kind.as_str(), "fetch" | "xhr");
            let source = match (url.is_empty() || request, n("line")) {
                (true, _) => None,
                (false, Some(l)) if l > 0 => Some(format!("{url}:{l}:{}", n("col").unwrap_or(0))),
                _ => Some(url.clone()),
            };
            // A request says what was asked and how it ended: `GET /api 500`, `POST /x: Failed to fetch`.
            let message = match (kind.as_str(), s("msg")) {
                ("fetch" | "xhr", m) => match (n("status"), m.is_empty()) {
                    (Some(st), _) if st > 0 => format!("{} {url} {st}", s("method")),
                    (_, false) => format!("{} {url}: {m}", s("method")),
                    _ => format!("{} {url} failed", s("method")),
                },
                ("resource", m) if m.is_empty() => format!("{} failed to load", s("tag")),
                (_, m) => m,
            };
            let key = (
                kind.clone(),
                console.unwrap_or_default().to_string(),
                // A repeat carries the first 200 characters of its message.
                message.chars().take(200).collect(),
                // A page's inline script is at the page's URL: one group for every query.
                format!(
                    "{}:{}",
                    url.split(['?', '#']).next().unwrap_or_default(),
                    n("line").unwrap_or(0)
                ),
            );
            let i = match at.get(&key) {
                Some(&i) => i,
                None if repeat.is_some() => continue,
                None => {
                    groups.push(Group {
                        kind,
                        level: lvl.into(),
                        message,
                        source,
                        status: n("status"),
                        count: 0,
                        first: ts,
                        last: ts,
                        page: s("at"),
                        frames: frames(&s("stack")),
                    });
                    at.insert(key, groups.len() - 1);
                    groups.len() - 1
                }
            };
            let g = &mut groups[i];
            match repeat {
                Some(times) => {
                    let had = repeats.entry((pid.clone(), i)).or_insert(1);
                    if times > *had {
                        g.count += times - *had;
                        *had = times;
                    }
                }
                None => {
                    g.count += 1;
                    repeats.entry((pid.clone(), i)).or_insert(1);
                    if ts >= g.last {
                        g.last = ts;
                        g.page = s("at");
                    }
                }
            }
        }
    }
    groups.sort_by(|a, b| a.last.total_cmp(&b.last));
    Ok(Report {
        groups,
        loads,
        end: page.end_offset,
    })
}

/// The newest page load of the app the bridge left out because of its CSP, from the
/// requests log; `false` once a newer page took the script.
pub fn blocked(site: &str) -> bool {
    let html = requests::Filter::default();
    let Ok(page) = requests::read(&requests::file(site), &html, None, 300) else {
        return false;
    };
    page.requests
        .iter()
        .find(|r| {
            r.status == 200
                && r.resp_headers
                    .get("Content-Type")
                    .and_then(|v| v.first())
                    .is_some_and(|t| t.starts_with("text/html"))
        })
        .is_some_and(|r| r.resp_headers.contains_key(BLOCKED))
}

/// For the app's Overview and Settings: errors in the log, since when, and the CSP note.
#[derive(Serialize, Debug, Default)]
pub struct Status {
    pub on: bool,
    pub errors: u64,
    /// When the oldest error in the log happened (seconds since 1970).
    pub since: Option<f64>,
    pub blocked: bool,
}

pub fn status(site: &Site) -> Status {
    if !on(site) {
        return Status::default();
    }
    let report = read(&file(&site.name), 0).unwrap_or_default();
    let errors: Vec<&Group> = report.errors().collect();
    Status {
        on: true,
        errors: errors.iter().map(|g| g.count).sum(),
        since: errors.iter().map(|g| g.first).reduce(f64::min),
        blocked: blocked(&site.name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A line as Anvil's Caddy writes it for one beacon (headers and addresses filtered out).
    fn line(page: &str, events: serde_json::Value) -> String {
        let d: String = events
            .to_string()
            .bytes()
            .map(|b| match b {
                b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' => {
                    (b as char).to_string()
                }
                _ => format!("%{b:02X}"),
            })
            .collect();
        format!(
            r#"{{"level":"info","ts":1792502416.2,"logger":"http.log.access.anvil_browser_shop","msg":"handled request","request":{{"proto":"HTTP/2.0","method":"POST","host":"shop.test","uri":"/__anvil/e?p={page}&d={d}"}},"bytes_read":0,"duration":0.0001,"size":0,"status":204}}"#
        ) + "\n"
    }

    fn log(lines: &[String]) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("shop.log");
        std::fs::write(&p, lines.concat()).unwrap();
        (dir, p)
    }

    #[test]
    fn decodes_what_encode_uri_component_wrote() {
        assert_eq!(unescape("a%20b%22%C3%A9%7B+c"), "a b\"é{ c");
        assert_eq!(unescape("50%"), "50%");
        assert_eq!(unescape("%zz%4"), "%zz%4");
    }

    #[test]
    fn groups_events_by_kind_message_and_place_with_repeats_counted() {
        let err = json!({"k": "error", "t": 1792502400000u64, "at": "/a", "msg": "Uncaught TypeError: x is undefined", "url": "https://shop.test/build/app.js", "line": 12, "col": 5,
            "stack": "TypeError: x is undefined\n    at go (https://shop.test/build/app.js:12:5)\n    at https://shop.test/build/app.js:40:1"});
        let (_d, p) = log(&[
            line(
                "p1",
                json!([err, {"k": "console", "level": "warn", "t": 1792502401000u64, "at": "/a", "msg": "careful",
                "stack": "Error\n    at console.warn (https://shop.test/__anvil/a.js?v=1:1:2)\n    at main (https://shop.test/build/app.js:3:1)"}]),
            ),
            line(
                "p1",
                json!([{"k": "repeat", "of": "error", "msg": "Uncaught TypeError: x is undefined", "url": "https://shop.test/build/app.js", "line": 12, "n": 2}]),
            ),
            line(
                "p1",
                json!([{"k": "repeat", "of": "error", "msg": "Uncaught TypeError: x is undefined", "url": "https://shop.test/build/app.js", "line": 12, "n": 10}]),
            ),
            line(
                "p2",
                json!([{"k": "error", "t": 1792502500000u64, "at": "/b", "msg": "Uncaught TypeError: x is undefined", "url": "https://shop.test/build/app.js", "line": 12, "col": 5},
                {"k": "fetch", "t": 1792502501000u64, "at": "/b", "method": "POST", "url": "/api/save", "status": 500},
                {"k": "load", "t": 1792502499000u64, "at": "/b", "status": 200, "ttfb": 12, "dom": 80, "load": 95}]),
            ),
            "not a beacon\n".into(),
        ]);
        let r = read(&p, 0).unwrap();
        let kinds: Vec<&str> = r.groups.iter().map(|g| g.kind.as_str()).collect();
        // Most recently seen last.
        assert_eq!(kinds, ["console", "error", "fetch"]);
        let e = &r.groups[1];
        // 10 on the first page, 1 on the second.
        assert_eq!(e.count, 11);
        assert_eq!(e.level, "error");
        assert_eq!(e.page, "/b");
        assert_eq!(
            e.source.as_deref(),
            Some("https://shop.test/build/app.js:12:5")
        );
        assert_eq!(e.first, 1792502400.0);
        assert_eq!(e.last, 1792502500.0);
        assert_eq!(e.frames.len(), 2);
        // Anvil's own frame (the console wrapper) is left out.
        assert_eq!(
            r.groups[0].frames,
            ["at main (https://shop.test/build/app.js:3:1)"]
        );
        assert_eq!(r.groups[0].level, "warning");
        assert_eq!(r.groups[2].message, "POST /api/save 500");
        assert_eq!(r.groups[2].source, None);
        assert_eq!(r.groups[2].status, Some(500));
        assert_eq!(r.errors().count(), 2);
        assert_eq!(
            r.loads,
            [Load {
                ts: 1792502499.0,
                page: "/b".into(),
                status: Some(200),
                ttfb: Some(12),
                dom_ready: Some(80),
                load: Some(95)
            }]
        );
        assert_eq!(r.end, std::fs::metadata(&p).unwrap().len());
        // From the end: nothing new.
        assert!(read(&p, r.end).unwrap().groups.is_empty());
    }

    #[test]
    fn an_inline_scripts_error_is_one_group_whatever_the_query() {
        let e = |q: &str| json!({"k": "error", "msg": "Uncaught Error: x", "url": format!("https://shop.test/list{q}"), "line": 2, "col": 3, "at": format!("/list{q}")});
        let (_d, p) = log(&[
            line("a", json!([e("?page=1")])),
            line("b", json!([e("?page=2#top")])),
        ]);
        let r = read(&p, 0).unwrap();
        assert_eq!(r.groups.len(), 1);
        assert_eq!(
            (r.groups[0].count, r.groups[0].page.as_str()),
            (2, "/list?page=2#top")
        );
    }

    #[test]
    fn a_rolled_log_is_read_from_its_start() {
        let (_d, p) = log(&[line(
            "p",
            json!([{"k": "rejection", "msg": "nope", "at": "/"}]),
        )]);
        let r = read(&p, 1 << 20).unwrap();
        assert_eq!(r.groups.len(), 1);
        assert_eq!(r.groups[0].message, "nope");
    }

    #[test]
    fn pool_ini_puts_the_prepend_first_and_keeps_the_users() {
        let _g = crate::lock(&crate::ENV_LOCK);
        let mut on = crate::tests::site("shop", "/s/shop", Some("8.4"));
        let off = crate::tests::site("blog", "/s/blog", Some("8.4"));
        let mut ini = BTreeMap::new();
        ini.insert("auto_prepend_file".to_string(), "/s/mine.php".to_string());
        // No app on: nothing changes.
        let (same, theirs) = pool_ini(&[on.clone(), off.clone()], ini.clone());
        assert_eq!((same, theirs), (ini.clone(), None));
        on.browser = true;
        on.kind = "laravel".into();
        let (out, theirs) = pool_ini(&[on.clone(), off], ini);
        assert_eq!(
            out["auto_prepend_file"],
            prepend_file().display().to_string()
        );
        assert_eq!(theirs.as_deref(), Some("/s/mine.php"));
        let (_, none) = pool_ini(&[on], BTreeMap::new());
        assert_eq!(none, None);
    }

    #[test]
    fn the_bridge_is_for_php_apps_and_dev_servers_not_static_sites() {
        let mut s = crate::tests::site("shop", "/s/shop", Some("8.4"));
        s.browser = true;
        for (kind, port, yes) in [
            ("laravel", None, true),
            ("php", None, true),
            ("static", None, false),
            ("compose", Some(3000), false),
            ("vite", Some(5173), true),
            ("next", Some(3000), true),
            ("django", Some(8000), true),
            // A dev server Anvil has no port for isn't behind Caddy.
            ("vite", None, false),
        ] {
            s.kind = kind.into();
            s.port = port;
            assert_eq!(on(&s), yes, "{kind}");
        }
        // Only PHP's own apps put the prepend into PHP's pools.
        s.kind = "vite".into();
        s.port = Some(5173);
        assert_eq!(pool_ini(&[s], BTreeMap::new()).0, BTreeMap::new());
    }

    #[test]
    fn the_switch_turns_on_for_a_dev_server_but_not_a_static_site() {
        let _g = crate::lock(&crate::ENV_LOCK);
        let home = tempfile::tempdir().unwrap();
        // SAFETY: every test that sets ANVIL_HOME holds ENV_LOCK.
        unsafe { std::env::set_var("ANVIL_HOME", home.path()) };
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = crate::Config::default();
        for (name, file, body) in [
            ("web", "package.json", r#"{"scripts":{"dev":"vite"}}"#),
            ("plain", "index.html", ""),
        ] {
            let p = dir.path().join(name);
            std::fs::create_dir_all(&p).unwrap();
            std::fs::write(p.join(file), body).unwrap();
            crate::link(&mut cfg, name, p).unwrap();
        }
        set(&mut cfg, "web", true).unwrap();
        assert_eq!(cfg.sites["web"].browser, Some(true));
        let err = set(&mut cfg, "plain", true).unwrap_err().to_string();
        assert!(err.contains("plain"), "{err}");
    }

    #[test]
    fn the_logger_keeps_no_headers_or_addresses() {
        let l = logger("my-shop.app").unwrap();
        for f in [
            "request>headers delete",
            "request>remote_ip delete",
            "request>client_ip delete",
            "resp_headers delete",
        ] {
            assert!(l.contains(f), "{f}: {l}");
        }
        assert!(l.contains("include http.log.access.anvil_browser_my-shop_app\n"));
        assert!(l.contains("roll_size 1MiB"));
        assert!(site_lines("my-shop.app").contains("log_name anvil_browser_my-shop_app\n"));
    }

    #[test]
    fn the_script_and_prepend_are_written_with_the_version() {
        let _g = crate::lock(&crate::ENV_LOCK);
        let home = tempfile::tempdir().unwrap();
        // SAFETY: every test that sets ANVIL_HOME holds ENV_LOCK.
        unsafe { std::env::set_var("ANVIL_HOME", home.path()) };
        write_files().unwrap();
        assert_eq!(std::fs::read_to_string(prepend_file()).unwrap(), PREPEND);
        let php = std::fs::read_to_string(dir().join("inject.php")).unwrap();
        // The proxy puts in the same tag.
        assert!(php.contains(&tag()), "{php}");
        assert!(!php.contains("__VERSION__"));
        assert_eq!(std::fs::read_to_string(dir().join("a.js")).unwrap(), A_JS);
        assert!(home.path().join("log/browser").is_dir());
    }
}
