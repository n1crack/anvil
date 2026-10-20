//! "Did my change work?": what changed in an app's processes and logs since a
//! cursor the caller holds. Each call reads only the bytes after the cursor's
//! offsets, so its cost follows what's new, not the size of the logs.

use crate::{Site, apps, browser, logs, requests};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Where the last `check` stopped: when, and how far each log source was read.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq)]
pub struct Cursor {
    pub at: u64,
    /// By source id (`logs::LogSource::id`).
    pub offsets: BTreeMap<String, u64>,
    /// How far the app's request log was read (`requests::file`).
    #[serde(default)]
    pub requests: u64,
    /// How far the app's browser log was read (`browser::file`).
    #[serde(default)]
    pub browser: u64,
}

/// One of the app's processes as the caller (the running app) knows it.
pub struct ProcState {
    /// `<site>`, `<site>@<process>` or `<site>#<script>`: its log file is `apps::log_file(key)`.
    pub key: String,
    pub label: String,
    pub state: String,
    /// Exit code and when (Unix seconds), when it ended on its own.
    pub exit: Option<(i32, u64)>,
}

#[derive(Serialize, Debug)]
pub struct Exited {
    pub label: String,
    pub code: i32,
    /// At most `LAST_LINES`, oldest first.
    pub last_lines: Vec<String>,
}

#[derive(Serialize, Debug)]
pub struct CheckEntry {
    /// Source id, as `logs` takes it.
    pub source: String,
    pub level: String,
    pub time: Option<String>,
    pub message: String,
}

#[derive(Serialize, Debug)]
pub struct Check {
    pub summary: String,
    pub exited: Vec<Exited>,
    /// Sources whose file got shorter than the cursor's offset: read again from the start.
    pub restarted: Vec<String>,
    /// Errors and warnings, at most `MAX_ENTRIES`, newest last.
    pub entries: Vec<CheckEntry>,
    /// Requests answered with a 5xx, at most `MAX_ENTRIES`, newest last.
    pub requests: Vec<requests::Row>,
    /// Errors on the app's pages while its browser bridge is on, at most `MAX_ENTRIES`,
    /// most recently seen last.
    pub browser: Vec<browser::Group>,
    pub cursor: Cursor,
}

const MAX_ENTRIES: usize = 50;
const LAST_LINES: usize = 20;
/// The last lines of an exited process come from this much of its log's end.
const LAST_BYTES: u64 = 16 * 1024;

pub fn check(site: &Site, procs: &[ProcState], since: Option<&Cursor>, now: u64) -> Check {
    let mut cursor = Cursor {
        at: now,
        offsets: BTreeMap::new(),
        requests: 0,
        browser: 0,
    };
    let (path, errors) = (
        requests::file(&site.name),
        requests::Filter {
            status: Some("5xx".into()),
            assets: true,
            ..Default::default()
        },
    );
    let mut failed: Vec<requests::Row> = Vec::new();
    let page = match since {
        // Nothing to read on the first call: only where the log ends.
        None => requests::read(&path, &errors, None, 0),
        Some(c) => requests::tail(&path, c.requests, &errors),
    };
    if let Ok(page) = page {
        cursor.requests = page.end_offset;
        failed = page.requests.iter().rev().map(Into::into).collect();
        failed.drain(..failed.len().saturating_sub(MAX_ENTRIES));
    }
    let mut page_errors: Vec<browser::Group> = Vec::new();
    if browser::on(site) {
        let file = browser::file(&site.name);
        match since {
            None => {
                let end = requests::read(&file, &errors, None, 0);
                cursor.browser = end.map_or(0, |p| p.end_offset);
            }
            Some(c) => {
                if let Ok(r) = browser::read(&file, c.browser) {
                    cursor.browser = r.end;
                    page_errors = r.errors().cloned().collect();
                    page_errors.drain(..page_errors.len().saturating_sub(MAX_ENTRIES));
                }
            }
        }
    }
    let mut restarted = Vec::new();
    let mut entries = Vec::new();
    // (label, errors, warnings) per source with any.
    let mut counts: Vec<(String, usize, usize)> = Vec::new();
    for src in logs::sources(site) {
        cursor.offsets.insert(src.id.clone(), src.bytes);
        let Some(since) = since else { continue };
        // A source that's new since the cursor is new from its first byte.
        let mut from = since.offsets.get(&src.id).copied().unwrap_or(0);
        if from > src.bytes {
            restarted.push(src.id.clone());
            from = 0;
        }
        if from == src.bytes {
            continue;
        }
        let Ok(page) = logs::tail(std::path::Path::new(&src.path), from) else {
            continue;
        };
        // The file may have grown since `sources` looked; the next call starts where this read ended.
        cursor.offsets.insert(src.id.clone(), page.end_offset);
        let (mut errors, mut warnings) = (0, 0);
        // `tail` gives newest first.
        for e in page.entries.into_iter().rev() {
            let level = match logs::group(&e.level) {
                "error" => {
                    errors += 1;
                    "error"
                }
                "warning" => {
                    warnings += 1;
                    "warning"
                }
                _ => continue,
            };
            entries.push(CheckEntry {
                source: src.id.clone(),
                level: level.into(),
                time: e.time,
                message: e.message,
            });
        }
        if errors + warnings > 0 {
            counts.push((src.label, errors, warnings));
        }
    }
    // ponytail: newest last within each source, not merged by time across sources.
    if entries.len() > MAX_ENTRIES {
        entries.drain(..entries.len() - MAX_ENTRIES);
    }
    let exited: Vec<Exited> = procs
        .iter()
        .filter_map(|p| {
            let (code, at) = p.exit?;
            since.is_none_or(|c| at > c.at).then(|| Exited {
                label: p.label.clone(),
                code,
                last_lines: last_lines(&apps::log_file(&p.key)),
            })
        })
        .collect();

    let running = procs.iter().filter(|p| p.state == "running").count();
    let state = format!("{running} of {} running", procs.len());
    let mut parts: Vec<String> = exited
        .iter()
        .map(|e| format!("{} exited ({})", e.label, e.code))
        .collect();
    let summary = match since {
        None => {
            parts.push(state);
            parts.join(", ")
        }
        Some(c) => {
            if !failed.is_empty() {
                parts.push(plural(failed.len(), "5xx request"));
            }
            let n: u64 = page_errors.iter().map(|g| g.count).sum();
            if n > 0 {
                parts.push(plural(n as usize, "browser error"));
            }
            for (label, errors, warnings) in &counts {
                let mut n = Vec::new();
                if *errors > 0 {
                    n.push(plural(*errors, "error"));
                }
                if *warnings > 0 {
                    n.push(plural(*warnings, "warning"));
                }
                parts.push(format!("{} in {label}", n.join(" and ")));
            }
            if parts.is_empty() {
                format!("No errors since {}, {state}", clock(c.at))
            } else {
                format!("{} since {}", parts.join(", "), clock(c.at))
            }
        }
    };
    Check {
        summary,
        exited,
        restarted,
        entries,
        requests: failed,
        browser: page_errors,
        cursor,
    }
}

fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

/// The newest non-empty lines of a log, escape codes removed, from its end only.
fn last_lines(path: &std::path::Path) -> Vec<String> {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(path) else {
        return vec![];
    };
    let len = f.metadata().map_or(0, |m| m.len());
    let start = len.saturating_sub(LAST_BYTES);
    if f.seek(SeekFrom::Start(start)).is_err() {
        return vec![];
    }
    let mut buf = Vec::new();
    let _ = f.read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf);
    // A read that starts mid-file starts mid-line: drop that partial line.
    let text = if start > 0 {
        text.split_once('\n').map_or("", |(_, t)| t)
    } else {
        &text
    };
    let mut lines: Vec<String> = text
        .lines()
        .map(logs::strip_ansi)
        .filter(|l| !l.trim().is_empty())
        .collect();
    lines.drain(..lines.len().saturating_sub(LAST_LINES));
    lines
}

/// `HH:MM` in local time.
fn clock(at: u64) -> String {
    crate::platform::local_clock(at)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// ANVIL_HOME and a Laravel app in a fresh /tmp dir, removed on drop.
    struct Env(
        PathBuf,
        Site,
        #[allow(dead_code)] std::sync::MutexGuard<'static, ()>,
    );
    impl Drop for Env {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn env() -> Env {
        static N: AtomicU32 = AtomicU32::new(0);
        let guard = crate::lock(&crate::ENV_LOCK);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = PathBuf::from(format!("/tmp/anvil-chk-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let app = dir.join("shop");
        std::fs::create_dir_all(app.join("storage/logs")).unwrap();
        std::fs::create_dir_all(dir.join("home/log")).unwrap();
        // SAFETY: every test that sets ANVIL_HOME holds ENV_LOCK.
        unsafe { std::env::set_var("ANVIL_HOME", dir.join("home")) };
        let site = Site {
            name: "shop".into(),
            host: String::new(),
            secure: false,
            path: app.clone(),
            root: app,
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
            kind: "laravel".into(),
            kind_detected: "laravel".into(),
            command: None,
            command_custom: false,
            port: None,
            autostart: false,
            runtime_version: None,
            runtime_pinned: false,
            browser: false,
            packages_pinned: None,
            shared_deps: None,
            worktree_of: None,
            branch: None,
            aliases: vec![],
        };
        Env(dir, site, guard)
    }
    fn log(e: &Env) -> PathBuf {
        e.1.path.join("storage/logs/laravel.log")
    }
    fn append(path: &std::path::Path, text: &str) {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
    }
    fn proc(label: &str, state: &str, exit: Option<(i32, u64)>) -> ProcState {
        ProcState {
            key: format!("shop@{label}"),
            label: label.into(),
            state: state.into(),
            exit,
        }
    }

    #[test]
    fn first_call_gives_state_and_cursor() {
        let e = env();
        append(&log(&e), "[2026-10-19 14:00:00] local.ERROR: old\n");
        let procs = [proc("api", "running", None), proc("vite", "running", None)];
        let c = check(&e.1, &procs, None, 1000);
        assert_eq!(c.summary, "2 of 2 running");
        assert!(c.entries.is_empty());
        assert_eq!(c.cursor.at, 1000);
        let len = std::fs::metadata(log(&e)).unwrap().len();
        assert_eq!(c.cursor.offsets.get("laravel.log"), Some(&len));
    }

    #[test]
    fn new_5xx_requests_are_reported() {
        let e = env();
        let file = crate::requests::file("shop");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let line = |uri: &str, status: u16| {
            format!(
                r#"{{"ts":1792502416.2,"request":{{"method":"GET","host":"shop.test","uri":"{uri}","headers":{{"Cookie":["REDACTED"]}}}},"duration":0.5,"size":1,"status":{status}}}"#
            ) + "\n"
        };
        append(&file, &line("/old", 500));
        let first = check(&e.1, &[], None, 1000);
        assert!(first.requests.is_empty());
        append(
            &file,
            &(line("/ok", 200) + &line("/boom", 502) + &line("/nf", 404)),
        );
        let c = check(&e.1, &[], Some(&first.cursor), 1100);
        assert_eq!(c.requests.len(), 1);
        assert_eq!(
            (c.requests[0].uri.as_str(), c.requests[0].status),
            ("/boom", 502)
        );
        assert!(
            c.summary.starts_with("1 5xx request since "),
            "{}",
            c.summary
        );
        let again = check(&e.1, &[], Some(&c.cursor), 1200);
        assert!(again.requests.is_empty());
    }

    #[test]
    fn new_browser_errors_are_reported_while_the_bridge_is_on() {
        let mut e = env();
        e.1.browser = true;
        e.1.php = Some("8.4".into());
        let file = crate::browser::file("shop");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let beacon = |events: &str| {
            let d: String = events.bytes().map(|b| format!("%{b:02X}")).collect();
            format!(
                r#"{{"ts":1792502416.2,"request":{{"method":"POST","host":"shop.test","uri":"/__anvil/e?p=x&d={d}"}},"status":204}}"#
            ) + "\n"
        };
        append(&file, &beacon(r#"[{"k":"error","msg":"old","at":"/"}]"#));
        let first = check(&e.1, &[], None, 1000);
        assert!(first.browser.is_empty());
        append(
            &file,
            &beacon(
                r#"[{"k":"error","msg":"boom","at":"/"},{"k":"console","level":"warn","msg":"meh","at":"/"},{"k":"load","at":"/"}]"#,
            ),
        );
        let c = check(&e.1, &[], Some(&first.cursor), 1100);
        let msgs: Vec<&str> = c.browser.iter().map(|g| g.message.as_str()).collect();
        // Errors only: warnings and page loads are in `browser`.
        assert_eq!(msgs, ["boom"]);
        assert!(
            c.summary.starts_with("1 browser error since "),
            "{}",
            c.summary
        );
        assert!(check(&e.1, &[], Some(&c.cursor), 1200).browser.is_empty());
        // Off: not read.
        e.1.browser = false;
        append(&file, &beacon(r#"[{"k":"error","msg":"later","at":"/"}]"#));
        assert!(check(&e.1, &[], Some(&c.cursor), 1300).browser.is_empty());
    }

    #[test]
    fn new_error_lines_are_reported() {
        let e = env();
        append(&log(&e), "[2026-10-19 14:00:00] local.ERROR: old\n");
        let first = check(&e.1, &[], None, 1000);
        append(
            &log(&e),
            "[2026-10-19 14:03:00] local.ERROR: Call to undefined method\n",
        );
        let c = check(&e.1, &[], Some(&first.cursor), 1100);
        assert_eq!(c.entries.len(), 1);
        assert_eq!(c.entries[0].message, "Call to undefined method");
        assert_eq!(c.entries[0].level, "error");
        assert_eq!(c.entries[0].source, "laravel.log");
        assert!(
            c.summary.starts_with("1 error in laravel.log since "),
            "{}",
            c.summary
        );
        // Nothing new after that.
        let again = check(&e.1, &[], Some(&c.cursor), 1200);
        assert!(again.entries.is_empty());
        assert!(
            again.summary.starts_with("No errors since "),
            "{}",
            again.summary
        );
    }

    #[test]
    fn info_lines_are_not_reported() {
        let e = env();
        append(&log(&e), "");
        let first = check(&e.1, &[], None, 1000);
        append(&log(&e), "[2026-10-19 14:03:00] local.INFO: fine\n");
        append(&log(&e), "[2026-10-19 14:03:01] local.WARNING: slow\n");
        let c = check(&e.1, &[], Some(&first.cursor), 1100);
        assert_eq!(c.entries.len(), 1);
        assert_eq!(c.entries[0].level, "warning");
    }

    #[test]
    fn truncated_log_reads_from_start() {
        let e = env();
        let body = "[2026-10-19 14:05:00] local.ERROR: after restart\n";
        let info = "[2026-10-19 14:05:01] local.INFO: ";
        let body = format!(
            "{body}{info}{}\n",
            "x".repeat(120 - body.len() - info.len() - 1)
        );
        std::fs::write(log(&e), &body).unwrap();
        assert_eq!(body.len(), 120);
        let since = Cursor {
            at: 1000,
            offsets: [("laravel.log".to_string(), 500)].into(),
            requests: 0,
            browser: 0,
        };
        let c = check(&e.1, &[], Some(&since), 1100);
        assert_eq!(c.restarted, ["laravel.log"]);
        assert_eq!(c.entries.len(), 1);
        assert_eq!(c.entries[0].message, "after restart");
        assert_eq!(c.cursor.offsets.get("laravel.log"), Some(&120));
    }

    #[test]
    fn exit_after_cursor_is_reported() {
        let e = env();
        append(
            &apps::log_file("shop@api"),
            "booting\n\x1b[31mboom\x1b[0m\n",
        );
        let since = Cursor {
            at: 1000,
            offsets: Default::default(),
            requests: 0,
            browser: 0,
        };
        let procs = [
            proc("api", "crashed", Some((1, 1050))),
            proc("old", "crashed", Some((2, 900))),
            proc("vite", "running", None),
        ];
        let c = check(&e.1, &procs, Some(&since), 1100);
        assert_eq!(c.exited.len(), 1);
        assert_eq!(c.exited[0].label, "api");
        assert_eq!(c.exited[0].code, 1);
        assert_eq!(c.exited[0].last_lines, ["booting", "boom"]);
        assert!(
            c.summary.starts_with("api exited (1) since "),
            "{}",
            c.summary
        );
    }

    #[test]
    fn entries_are_capped_at_50() {
        let e = env();
        append(&log(&e), "");
        let first = check(&e.1, &[], None, 1000);
        let text: String = (0..80)
            .map(|i| format!("[2026-10-19 14:03:00] local.ERROR: e{i}\n"))
            .collect();
        append(&log(&e), &text);
        let c = check(&e.1, &[], Some(&first.cursor), 1100);
        assert_eq!(c.entries.len(), 50);
        assert_eq!(c.entries[49].message, "e79");
        assert_eq!(c.entries[0].message, "e30");
        assert!(
            c.summary.starts_with("80 errors in laravel.log"),
            "{}",
            c.summary
        );
    }
}
