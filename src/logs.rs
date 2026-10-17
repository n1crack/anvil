//! Reading an app's log files without loading them: pages are read backwards
//! from the end, and Monolog entries (one header line plus any continuation
//! lines) are kept whole.

use crate::{Site, apps, home};
use anyhow::{Context, Result};
use serde::Serialize;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// How much one page reads.
pub const PAGE: u64 = 256 * 1024;
/// A window grows up to this to find the start of a long entry.
const MAX_WINDOW: u64 = 2 * 1024 * 1024;
/// Search gives up after this much of the file, or after `limit` matches.
pub const SEARCH_BYTES: u64 = 50 * 1024 * 1024;
const SEARCH_CHUNK: u64 = 1024 * 1024;

#[derive(Serialize, Debug, Clone)]
pub struct LogSource {
    pub id: String,
    pub label: String,
    pub path: String,
    pub bytes: u64,
    pub modified: u64,
}

#[derive(Serialize, Debug, Clone, Default, PartialEq)]
pub struct Entry {
    pub time: Option<String>,
    pub env: Option<String>,
    pub level: String,
    pub message: String,
    pub context: Option<String>,
    pub trace: Vec<String>,
}

#[derive(Serialize, Debug, Default)]
pub struct Page {
    /// Newest first.
    pub entries: Vec<Entry>,
    /// Pass as `before` to get older entries.
    pub start_offset: u64,
    /// Where new content will start; pass to `tail`.
    pub end_offset: u64,
    /// True when there is more before `start_offset`.
    pub truncated: bool,
}

/// A log file; a symlink is not one. A cloned project could point
/// `storage/logs/x.log` at any file, and Clear would empty it.
fn source(id: &str, label: &str, path: &Path) -> Option<LogSource> {
    let meta = fs::symlink_metadata(path).ok().filter(|m| m.is_file())?;
    Some(LogSource {
        id: id.into(),
        label: label.into(),
        path: path.display().to_string(),
        bytes: meta.len(),
        modified: meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs()),
    })
}

/// Source id and log file of one of an app's processes, both by its id.
fn process_log(site: &str, p: &crate::processes::Process) -> (String, PathBuf) {
    (
        format!("proc:{}", p.id),
        crate::processes::log_file(site, &p.id),
    )
}

/// Source id and log file of a package.json command run once, by its id.
fn once_log(site: &str, script: &str) -> (String, PathBuf) {
    (
        format!("once:{}", crate::processes::process_id(script)),
        apps::log_file(&crate::processes::once_key(site, script)),
    )
}

/// The log files an app has, newest modified first, then Anvil's own.
pub fn sources(site: &Site) -> Vec<LogSource> {
    let dir = match site.kind.as_str() {
        "laravel" | "statamic" => Some(site.path.join("storage/logs")),
        "symfony" => Some(site.path.join("var/log")),
        _ => None,
    }
    // The log folder itself may be a link out of the project.
    .filter(|d| inside(d, &site.path));
    let mut out: Vec<LogSource> = dir
        .and_then(|d| fs::read_dir(d).ok())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name.ends_with(".log")
                .then(|| source(&name, &name, &e.path()))
                .flatten()
        })
        .collect();
    out.sort_by(|a, b| b.modified.cmp(&a.modified).then(a.id.cmp(&b.id)));
    if apps::runs_process(&site.kind) {
        out.extend(source("app", "App output", &apps::log_file(&site.name)));
    }
    // The app's other processes, once they have written something.
    for p in crate::processes::detect(site) {
        let (id, path) = process_log(&site.name, &p);
        out.extend(source(&id, &p.name, &path));
    }
    // Commands run once, once they have written something.
    for script in crate::processes::package_scripts(&site.path).1 {
        let (id, path) = once_log(&site.name, &script);
        out.extend(source(&id, &script, &path));
    }
    if let Some(v) = site.php.as_ref().filter(|_| apps::is_php(&site.kind)) {
        let path = home().join("log").join(format!("php-{v}-fpm.log"));
        out.extend(source("php-fpm", &format!("PHP-FPM {v}"), &path));
    }
    out
}

fn inside(path: &Path, root: &Path) -> bool {
    match (path.canonicalize(), root.canonicalize()) {
        (Ok(p), Ok(r)) => p.starts_with(r),
        _ => false,
    }
}

/// One of the app's sources by id or label; anything else is refused.
pub fn find(site: &Site, id: &str) -> Result<LogSource> {
    sources(site)
        .into_iter()
        // A process or command is found by its id only: its name could be another log's label.
        .find(|s| {
            s.id == id
                || (!s.id.starts_with("proc:")
                    && !s.id.starts_with("once:")
                    && s.label.eq_ignore_ascii_case(id))
        })
        .with_context(|| format!("{} has no log named {id}", site.name))
}

/// Empty a log file. Only the app's own sources can be cleared.
pub fn clear(site: &Site, id: &str) -> Result<()> {
    let src = find(site, id)?;
    fs::OpenOptions::new()
        .write(true)
        .open(&src.path)?
        .set_len(0)?;
    Ok(())
}

/// A page of entries ending at `before` (default: the end of the file).
pub fn read(path: &Path, before: Option<u64>, max_bytes: u64) -> Result<Page> {
    let mut f = File::open(path)?;
    let len = f.metadata()?.len();
    let end = before.unwrap_or(len).min(len);
    let (start, text) = window(&mut f, end, max_bytes)?;
    let mut entries: Vec<Entry> = split(&text).into_iter().map(entry).collect();
    entries.reverse();
    Ok(Page {
        entries,
        start_offset: start,
        end_offset: len,
        truncated: start > 0,
    })
}

/// Entries appended since `from`. A file that shrank (cleared or rotated) starts over.
pub fn tail(path: &Path, from: u64) -> Result<Page> {
    let len = fs::metadata(path)?.len();
    let from = if from > len { 0 } else { from };
    if len - from > PAGE {
        return read(path, None, PAGE);
    }
    let mut f = File::open(path)?;
    f.seek(SeekFrom::Start(from))?;
    let mut buf = Vec::new();
    f.take(len - from).read_to_end(&mut buf)?;
    // ponytail: content appended mid-entry shows as its own lines; Monolog writes whole entries at once.
    let mut entries: Vec<Entry> = split(&String::from_utf8_lossy(&buf))
        .into_iter()
        .map(entry)
        .collect();
    entries.reverse();
    Ok(Page {
        entries,
        start_offset: from,
        end_offset: len,
        truncated: false,
    })
}

/// Entries matching `query` (case-insensitive) and a level group, newest first,
/// scanning backwards from `before` until `limit` matches or `SEARCH_BYTES`.
pub fn search(
    path: &Path,
    query: &str,
    level: Option<&str>,
    before: Option<u64>,
    limit: usize,
) -> Result<Page> {
    let mut f = File::open(path)?;
    let len = f.metadata()?.len();
    let q = query.trim().to_lowercase();
    let level = level.filter(|l| !l.is_empty() && *l != "all");
    let mut end = before.unwrap_or(len).min(len);
    let stop = end.saturating_sub(SEARCH_BYTES);
    let mut entries = Vec::new();
    while end > 0 && end > stop && entries.len() < limit {
        let (start, text) = window(&mut f, end, SEARCH_CHUNK)?;
        let mut hits: Vec<&str> = split(&text)
            .into_iter()
            .filter(|raw| level.is_none_or(|l| group(&level_of(raw)) == l))
            .filter(|raw| q.is_empty() || raw.to_lowercase().contains(&q))
            .collect();
        hits.reverse();
        entries.extend(hits.into_iter().take(limit - entries.len()).map(entry));
        if start >= end {
            break;
        }
        end = start;
    }
    Ok(Page {
        entries,
        start_offset: end,
        end_offset: len,
        truncated: end > 0,
    })
}

/// Bytes `[start, end)` where `start` is the beginning of an entry (or of the
/// file), widened up to `MAX_WINDOW` to find one.
fn window(f: &mut File, end: u64, size: u64) -> Result<(u64, String)> {
    let mut size = size.max(1);
    loop {
        let start = end.saturating_sub(size);
        let mut buf = vec![0; (end - start) as usize];
        f.seek(SeekFrom::Start(start))?;
        f.read_exact(&mut buf)?;
        if start == 0 {
            return Ok((0, String::from_utf8_lossy(&buf).into_owned()));
        }
        let aligned = first_header(&buf).or_else(|| {
            (size * 2 > MAX_WINDOW).then(|| {
                buf.iter()
                    .position(|&b| b == b'\n')
                    .map_or(buf.len(), |i| i + 1)
            })
        });
        if let Some(i) = aligned {
            return Ok((
                start + i as u64,
                String::from_utf8_lossy(&buf[i..]).into_owned(),
            ));
        }
        size *= 2;
    }
}

/// Offset of the first line in `buf` (not counting a possibly cut first line) that starts an entry.
fn first_header(buf: &[u8]) -> Option<usize> {
    let mut i = 0;
    while let Some(n) = buf[i..].iter().position(|&b| b == b'\n') {
        i += n + 1;
        let line_end = buf[i..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(buf.len(), |n| i + n);
        if header(&String::from_utf8_lossy(&buf[i..line_end.min(i + 200)])).is_some() {
            return Some(i);
        }
    }
    None
}

struct Header<'a> {
    time: String,
    env: Option<&'a str>,
    level: &'a str,
    rest: &'a str,
}

/// `[2026-05-30 15:12:42] local.DEBUG: message`, or PHP-FPM's `[30-May-2026 15:12:42] NOTICE: message`.
fn header(line: &str) -> Option<Header<'_>> {
    let inner_end = line.strip_prefix('[')?.find(']')? + 1;
    let inner = &line[1..inner_end];
    if inner.len() > 40 || !inner.starts_with(|c: char| c.is_ascii_digit()) || !inner.contains(':')
    {
        return None;
    }
    let after = line[inner_end + 1..].strip_prefix(' ')?;
    let colon = after
        .find(": ")
        .or_else(|| after.ends_with(':').then(|| after.len() - 1))?;
    let tag = &after[..colon];
    if tag.len() > 60 || tag.contains(' ') {
        return None;
    }
    let (env, level) = match tag.rsplit_once('.') {
        Some((e, l)) => (Some(e), l),
        None => (None, tag),
    };
    if level.is_empty() || !level.chars().all(|c| c.is_ascii_uppercase()) {
        return None;
    }
    Some(Header {
        time: fpm_time(inner).unwrap_or_else(|| inner.to_string()),
        env,
        level,
        rest: after.get(colon + 2..).unwrap_or(""),
    })
}

/// `30-May-2026 15:12:42` as `2026-05-30 15:12:42`.
fn fpm_time(s: &str) -> Option<String> {
    let (date, time) = s.split_once(' ')?;
    let mut p = date.split('-');
    let (d, m, y) = (p.next()?, p.next()?, p.next()?);
    let months = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let m = months.iter().position(|x| *x == m)? + 1;
    Some(format!("{y}-{m:02}-{d:0>2} {time}"))
}

/// Raw entries in file order: a header line with what follows it, or single
/// lines when the text has no headers at all.
pub fn split(text: &str) -> Vec<&str> {
    let monolog = text.lines().any(|l| header(l).is_some());
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let mut pos = 0;
    for line in text.split_inclusive('\n') {
        let here = pos;
        pos += line.len();
        if monolog && header(line).is_none() {
            if start.is_none() && !line.trim().is_empty() {
                out.push(line);
            }
            continue;
        }
        if let Some(s) = start.take() {
            out.push(&text[s..here]);
        }
        if monolog {
            start = Some(here);
        } else if !strip_ansi(line).trim().is_empty() {
            out.push(line);
        }
    }
    if let Some(s) = start {
        out.push(&text[s..]);
    }
    out
}

/// Lowercase level of a raw entry, guessed from keywords when it has no header.
fn level_of(raw: &str) -> String {
    let first = raw.lines().next().unwrap_or("");
    if let Some(h) = header(first) {
        return h.level.to_ascii_lowercase();
    }
    let l = strip_ansi(first).to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| l.contains(w));
    if has(&[
        "error",
        "fatal",
        "panic",
        "exception",
        "failed",
        "err!",
        "\u{2a2f}",
    ]) {
        "error"
    } else if has(&["warn", "\u{26a0}"]) {
        "warning"
    } else if has(&["debug"]) {
        "debug"
    } else {
        "info"
    }
    .into()
}

/// The filter a level falls under: error, warning, info or debug.
pub fn group(level: &str) -> &'static str {
    match level {
        "emergency" | "alert" | "critical" | "error" | "fatal" => "error",
        "warning" | "warn" => "warning",
        "debug" => "debug",
        _ => "info",
    }
}

pub fn entry(raw: &str) -> Entry {
    let raw = raw.trim_end();
    let Some(h) = header(raw.lines().next().unwrap_or("")) else {
        let line = strip_ansi(raw);
        let (time, message) = iso_time(&line);
        return Entry {
            time,
            level: level_of(raw),
            message,
            ..Entry::default()
        };
    };
    let more = raw.find('\n').map_or("", |i| &raw[i..]);
    let mut text = format!("{}{more}", h.rest);
    // Monolog appends the extra data last, usually empty.
    if let Some(t) = text.strip_suffix(" []") {
        text = t.trim_end().to_string();
    }
    let mut e = Entry {
        time: Some(h.time),
        env: h.env.map(str::to_string),
        level: h.level.to_ascii_lowercase(),
        ..Entry::default()
    };
    if let Some((msg, mut ctx)) = context(&text) {
        if let Some(ex) = ctx.get("exception").and_then(|v| v.as_str())
            && let Some((head, trace)) = ex.split_once("[stacktrace]")
        {
            e.trace = trace
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .collect();
            ctx["exception"] = head.trim().into();
        }
        e.context = Some(ctx.to_string());
        text = msg.to_string();
    }
    let mut lines = Vec::new();
    for line in text.lines() {
        let t = line.trim_start();
        let frame = t.starts_with('#') && t[1..].starts_with(|c: char| c.is_ascii_digit());
        if frame || t == "[stacktrace]" || t == "Stack trace:" {
            if t.starts_with('#') {
                e.trace.push(t.to_string());
            }
        } else {
            lines.push(line);
        }
    }
    e.message = lines.join("\n").trim().to_string();
    e
}

/// Split `message {"json":"context"}` at the earliest ` {` whose rest is valid JSON.
fn context(text: &str) -> Option<(&str, serde_json::Value)> {
    if !text.ends_with('}') {
        return None;
    }
    for (i, _) in text.match_indices(" {").take(50) {
        let json = &text[i + 1..];
        // Laravel writes stack traces with real newlines inside the JSON string.
        let parsed = serde_json::from_str::<serde_json::Value>(json)
            .or_else(|_| serde_json::from_str(&json.replace('\n', "\\n")));
        if let Ok(v @ serde_json::Value::Object(_)) = parsed {
            return Some((&text[..i], v));
        }
    }
    None
}

/// A leading ISO timestamp (`2026-05-30T15:12:42.123Z` or with a space), split off.
fn iso_time(line: &str) -> (Option<String>, String) {
    let s = line.trim_start_matches('[');
    let b = s.as_bytes();
    let shape = b.len() >= 19
        && b[4] == b'-'
        && b[7] == b'-'
        && (b[10] == b'T' || b[10] == b' ')
        && b[13] == b':'
        && b[16] == b':'
        && b[..4].iter().all(u8::is_ascii_digit);
    if !shape {
        return (None, line.trim_end().to_string());
    }
    let end = s[19..].find([' ', ']']).map_or(s.len(), |i| i + 19);
    let rest = s[end..].trim_start_matches(']').trim();
    (Some(s[..end].to_string()), rest.to_string())
}

/// Drop terminal colour and cursor codes; keep what follows the last carriage return.
pub fn strip_ansi(s: &str) -> String {
    let s = s.trim_end_matches(['\n', '\r']);
    let s = s.rsplit('\r').find(|p| !p.is_empty()).unwrap_or("");
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('[') => {
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
            Some(']') => {
                while let Some(c) = chars.next() {
                    if c == '\x07' || (c == '\x1b' && chars.next_if_eq(&'\\').is_some()) {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const LARAVEL: &str = "[2026-05-30 15:12:40] local.INFO: Started {\"user\":1} []
[2026-05-30 15:12:42] local.ERROR: Boom {\"userId\":7,\"exception\":\"[object] (RuntimeException(code: 0): Boom at /app/Foo.php:3)
[stacktrace]
#0 /app/vendor/x.php(1): a()
#1 {main}
\"}
[2026-05-30 15:12:43] testing.WARNING: The width(-1) of chart,
       please check the style {\"url\":\"http://x\"}
";

    #[test]
    fn process_log_is_named_by_id() {
        let p = crate::processes::Process {
            id: "docs-dev".into(),
            name: "docs:dev".into(),
            command: String::new(),
            source: "package.json",
            server: true,
            port: None,
        };
        let (id, path) = process_log("shop", &p);
        assert_eq!(id, "proc:docs-dev");
        assert_eq!(path.file_name().unwrap(), "app-shop@docs-dev.log");
    }

    #[test]
    fn once_log_is_named_by_id() {
        let (id, path) = once_log("shop", "docs:dev");
        assert_eq!(id, "once:docs-dev");
        assert_eq!(path.file_name().unwrap(), "app-shop#docs-dev.log");
    }

    #[test]
    fn parses_monolog() {
        let e: Vec<Entry> = split(LARAVEL).into_iter().map(entry).collect();
        assert_eq!(e.len(), 3);
        assert_eq!(e[0].message, "Started");
        assert_eq!(e[0].context.as_deref(), Some("{\"user\":1}"));
        assert_eq!(e[1].level, "error");
        assert_eq!(e[1].env.as_deref(), Some("local"));
        assert_eq!(e[1].time.as_deref(), Some("2026-05-30 15:12:42"));
        assert_eq!(e[1].message, "Boom");
        assert_eq!(e[1].trace, ["#0 /app/vendor/x.php(1): a()", "#1 {main}"]);
        let ctx: serde_json::Value = serde_json::from_str(e[1].context.as_ref().unwrap()).unwrap();
        assert_eq!(ctx["userId"], 7);
        assert!(ctx["exception"].as_str().unwrap().ends_with("Foo.php:3)"));
        assert_eq!(e[2].level, "warning");
        assert_eq!(
            e[2].message,
            "The width(-1) of chart,\n       please check the style"
        );
        assert_eq!(group("critical"), "error");
        assert_eq!(group("notice"), "info");
    }

    #[test]
    fn plain_trace_without_json() {
        let raw = "[2026-05-30 15:12:42] production.CRITICAL: Oops\nStack trace:\n#0 /a.php(2): b()\n#1 {main}\n";
        let e = entry(split(raw)[0]);
        assert_eq!(e.message, "Oops");
        assert_eq!(e.trace.len(), 2);
    }

    #[test]
    fn fpm_and_plain_lines() {
        let e = entry("[17-Oct-2026 09:05:35] NOTICE: ready to handle connections");
        assert_eq!(e.time.as_deref(), Some("2026-10-17 09:05:35"));
        assert_eq!(e.level, "notice");
        assert_eq!(group(&e.level), "info");

        let text = "\x1b[32m\u{2713}\x1b[39m Ready in 1.2s\n\n2026-05-30T15:12:42.120Z WARN slow query\nError: listen EADDRINUSE\n debug: cache hit\n";
        let e: Vec<Entry> = split(text).into_iter().map(entry).collect();
        assert_eq!(e.len(), 4);
        assert_eq!(e[0].message, "\u{2713} Ready in 1.2s");
        assert_eq!(e[0].level, "info");
        assert_eq!(e[1].time.as_deref(), Some("2026-05-30T15:12:42.120Z"));
        assert_eq!(e[1].message, "WARN slow query");
        assert_eq!(e[1].level, "warning");
        assert_eq!(e[2].level, "error");
        assert_eq!(e[3].level, "debug");
    }

    #[test]
    fn clear_only_touches_own_sources() {
        let dir = std::env::temp_dir().join(format!("anvil-logs-site-{}", std::process::id()));
        fs::create_dir_all(dir.join("storage/logs")).unwrap();
        fs::write(
            dir.join("storage/logs/laravel.log"),
            "[2026-05-30 15:00:00] local.INFO: x\n",
        )
        .unwrap();
        fs::write(dir.join("other.log"), "keep").unwrap();
        // A link to a file outside the logs is not a log.
        std::os::unix::fs::symlink(dir.join("other.log"), dir.join("storage/logs/evil.log"))
            .unwrap();
        let site = Site {
            name: "shop".into(),
            host: String::new(),
            secure: false,
            path: dir.clone(),
            root: dir.clone(),
            php: None,
            php_pinned: false,
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
        };
        let ids: Vec<String> = sources(&site).into_iter().map(|s| s.id).collect();
        assert_eq!(ids, ["laravel.log"]);
        assert!(clear(&site, "evil.log").is_err());
        assert!(clear(&site, "../../other.log").is_err());
        assert!(clear(&site, dir.join("other.log").to_str().unwrap()).is_err());
        clear(&site, "laravel.log").unwrap();
        assert_eq!(
            fs::metadata(dir.join("storage/logs/laravel.log"))
                .unwrap()
                .len(),
            0
        );
        assert_eq!(fs::read_to_string(dir.join("other.log")).unwrap(), "keep");

        // A log folder that links out of the project is not read at all.
        let elsewhere = dir.join("elsewhere");
        fs::create_dir_all(&elsewhere).unwrap();
        fs::write(elsewhere.join("secret.log"), "keep").unwrap();
        let linked = dir.join("linked");
        fs::create_dir_all(linked.join("storage")).unwrap();
        std::os::unix::fs::symlink(&elsewhere, linked.join("storage/logs")).unwrap();
        let other = Site {
            path: linked,
            ..site
        };
        assert!(sources(&other).is_empty());
        fs::remove_dir_all(dir).unwrap();
    }

    fn temp(name: &str, body: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("anvil-logs-{}-{name}", std::process::id()));
        File::create(&p)
            .unwrap()
            .write_all(body.as_bytes())
            .unwrap();
        p
    }

    #[test]
    fn reads_backwards_without_cutting_entries() {
        let mut body = String::new();
        for i in 0..300 {
            body += &format!(
                "[2026-05-30 15:00:00] local.ERROR: entry {i}\n#0 /app/a.php(1): x()\n#1 {{main}}\n"
            );
        }
        let p = temp("back", &body);
        // A small window forces entries to straddle chunk boundaries.
        let mut seen = Vec::new();
        let mut before = None;
        loop {
            let page = read(&p, before, 500).unwrap();
            for e in &page.entries {
                assert_eq!(e.trace.len(), 2, "{}", e.message);
                seen.push(e.message.clone());
            }
            if !page.truncated {
                break;
            }
            before = Some(page.start_offset);
        }
        let want: Vec<String> = (0..300).rev().map(|i| format!("entry {i}")).collect();
        assert_eq!(seen, want);

        let page = search(&p, "ENTRY 29", Some("error"), None, 5).unwrap();
        let got: Vec<&str> = page.entries.iter().map(|e| e.message.as_str()).collect();
        assert_eq!(
            got,
            [
                "entry 299",
                "entry 298",
                "entry 297",
                "entry 296",
                "entry 295"
            ]
        );
        assert!(
            search(&p, "", Some("debug"), None, 5)
                .unwrap()
                .entries
                .is_empty()
        );

        let end = read(&p, None, PAGE).unwrap().end_offset;
        fs::OpenOptions::new()
            .append(true)
            .open(&p)
            .unwrap()
            .write_all(b"[2026-05-30 15:00:01] local.INFO: new one\n")
            .unwrap();
        let t = tail(&p, end).unwrap();
        assert_eq!(t.entries.len(), 1);
        assert_eq!(t.entries[0].message, "new one");
        fs::remove_file(p).unwrap();
    }
}
