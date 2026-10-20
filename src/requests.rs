//! The requests Caddy served for an app, from its JSON access log
//! (`log/requests/<site>.log`, rolled by Caddy at 2 MiB), newest first.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

#[derive(Serialize, Debug, Clone, PartialEq, Default)]
pub struct Request {
    /// Seconds since 1970.
    pub ts: f64,
    pub method: String,
    pub uri: String,
    pub host: String,
    pub proto: String,
    pub status: u16,
    pub duration_ms: f64,
    pub size: u64,
    /// Caddy writes `REDACTED` for Cookie, Set-Cookie, Authorization and Proxy-Authorization.
    pub req_headers: BTreeMap<String, Vec<String>>,
    pub resp_headers: BTreeMap<String, Vec<String>>,
    pub remote: String,
    pub tls: bool,
}

#[derive(Serialize, Debug, Default)]
pub struct Page {
    /// Newest first.
    pub requests: Vec<Request>,
    /// Pass as `before` to get older requests.
    pub start_offset: u64,
    /// Where new requests will start; pass to `tail`.
    pub end_offset: u64,
    /// True when there is more before `start_offset`.
    pub truncated: bool,
}

/// What the list shows: a status class (`4xx`), a method, text in the path or host, and
/// whether static assets (Vite's, Next's, source maps, the favicon) count.
#[derive(Deserialize, Debug, Default, Clone)]
#[serde(default)]
pub struct Filter {
    pub status: Option<String>,
    pub method: Option<String>,
    pub query: String,
    pub assets: bool,
}

/// The access log Caddy writes for an app (its extra server rows included).
pub fn file(site: &str) -> PathBuf {
    crate::home()
        .join("log")
        .join("requests")
        .join(format!("{site}.log"))
}

#[derive(Deserialize)]
struct Line {
    ts: f64,
    request: Req,
    status: u16,
    #[serde(default)]
    duration: f64,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    resp_headers: BTreeMap<String, Vec<String>>,
}

#[derive(Deserialize)]
struct Req {
    #[serde(default)]
    remote_ip: String,
    #[serde(default)]
    client_ip: String,
    #[serde(default)]
    proto: String,
    #[serde(default)]
    method: String,
    #[serde(default)]
    host: String,
    #[serde(default)]
    uri: String,
    #[serde(default)]
    headers: BTreeMap<String, Vec<String>>,
    tls: Option<serde::de::IgnoredAny>,
}

/// One line of the access log; anything else Caddy writes there is not a request. The
/// zeros before it (the file was emptied while Caddy wrote to it) don't count.
pub fn parse(line: &str) -> Option<Request> {
    let l: Line = serde_json::from_str(line.trim_start_matches('\0').trim()).ok()?;
    let r = l.request;
    Some(Request {
        ts: l.ts,
        remote: if r.client_ip.is_empty() {
            r.remote_ip
        } else {
            r.client_ip
        },
        method: r.method,
        uri: r.uri,
        host: r.host,
        proto: r.proto,
        status: l.status,
        duration_ms: l.duration * 1000.0,
        size: l.size,
        req_headers: r.headers,
        resp_headers: l.resp_headers,
        tls: r.tls.is_some(),
    })
}

/// Files a page loads by the dozen (Vite's, Next's, source maps, the favicon): hidden
/// unless asked for, so the page's own requests stand out.
pub fn is_asset(uri: &str) -> bool {
    let path = uri.split(['?', '#']).next().unwrap_or("");
    [
        "/build/",
        "/_next/static/",
        "/@vite/",
        "/node_modules/.vite/",
        "/favicon",
    ]
    .iter()
    .any(|p| path.starts_with(p))
        || path.ends_with(".map")
}

impl Filter {
    pub fn matches(&self, r: &Request) -> bool {
        let set = |o: &Option<String>| o.clone().filter(|s| !s.is_empty() && s != "all");
        let q = self.query.trim().to_lowercase();
        (self.assets || !is_asset(&r.uri))
            && set(&self.status).is_none_or(|s| s == format!("{}xx", r.status / 100))
            && set(&self.method).is_none_or(|m| m.eq_ignore_ascii_case(&r.method))
            && (q.is_empty()
                || r.uri.to_lowercase().contains(&q)
                || r.host.to_lowercase().contains(&q))
    }
}

/// A line's start in `buf` after a possibly cut first line, past the zeros of a cleared file.
fn line_start(buf: &[u8]) -> Option<usize> {
    let i = buf.iter().position(|&b| b == b'\n' || b == 0)? + 1;
    Some(i + buf[i..].iter().take_while(|&&b| b == 0).count())
}

fn open(path: &Path) -> Result<Option<File>> {
    match File::open(path) {
        Ok(f) => Ok(Some(f)),
        // Caddy hasn't served the app yet.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Where the last whole line ends: Caddy may be halfway through writing one.
fn lines_end(f: &mut File, len: u64) -> Result<u64> {
    let from = len.saturating_sub(64 * 1024);
    f.seek(SeekFrom::Start(from))?;
    let mut buf = Vec::new();
    f.take(len - from).read_to_end(&mut buf)?;
    Ok(match buf.iter().rposition(|&b| b == b'\n') {
        Some(i) => from + i as u64 + 1,
        None if from == 0 => 0,
        None => len,
    })
}

/// Up to `limit` requests matching `filter` that end before `before` (default: the end),
/// newest first. The file is at most Caddy's roll size, so a filter may scan all of it.
pub fn read(path: &Path, filter: &Filter, before: Option<u64>, limit: usize) -> Result<Page> {
    let Some(mut f) = open(path)? else {
        return Ok(Page::default());
    };
    let len = f.metadata()?.len();
    let end_offset = lines_end(&mut f, len)?;
    let mut end = before.unwrap_or(end_offset).min(end_offset);
    let mut requests = Vec::new();
    'pages: while end > 0 {
        let (start, text) = crate::logs::window(&mut f, end, crate::logs::PAGE, line_start)?;
        if start >= end {
            // ponytail: only zeros (or one line over 2 MiB) before here; read as the start.
            end = 0;
            break;
        }
        let mut lines = Vec::new();
        let mut at = start;
        for l in text.split_inclusive('\n') {
            lines.push((at, l));
            at += l.len() as u64;
        }
        for (at, l) in lines.into_iter().rev() {
            if requests.len() >= limit {
                end = at + l.len() as u64;
                break 'pages;
            }
            requests.extend(parse(l).filter(|r| filter.matches(r)));
        }
        end = start;
    }
    Ok(Page {
        requests,
        start_offset: end,
        end_offset,
        truncated: end > 0,
    })
}

/// Requests written since `from` that match `filter`, whole lines only. A file that shrank
/// (Caddy rolled it, or it was emptied) starts over.
/// ponytail: a roll whose new file grew past `from` between two reads goes unnoticed.
pub fn tail(path: &Path, from: u64, filter: &Filter) -> Result<Page> {
    let Some(mut f) = open(path)? else {
        return Ok(Page::default());
    };
    let len = f.metadata()?.len();
    let from = if from > len { 0 } else { from };
    f.seek(SeekFrom::Start(from))?;
    let mut buf = Vec::new();
    f.take(len - from).read_to_end(&mut buf)?;
    let end = buf.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
    let mut requests: Vec<Request> = String::from_utf8_lossy(&buf[..end])
        .lines()
        .filter_map(parse)
        .filter(|r| filter.matches(r))
        .collect();
    requests.reverse();
    Ok(Page {
        requests,
        start_offset: from,
        end_offset: from + end as u64,
        truncated: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    // Written by Anvil's Caddy 2.11.7 (an HTTPS request with a cookie and a bearer token).
    const LINE: &str = r#"{"level":"info","ts":1792502407.35184,"logger":"http.log.access.log0","msg":"handled request","request":{"remote_ip":"127.0.0.1","remote_port":"53658","client_ip":"127.0.0.1","proto":"HTTP/2.0","method":"GET","host":"a.test:18443","uri":"/index.php?q=1","headers":{"Authorization":["REDACTED"],"Proxy-Authorization":["REDACTED"],"User-Agent":["curl/8.7.1"],"Accept":["*/*"],"Cookie":["REDACTED"]},"tls":{"resumed":false,"version":772,"cipher_suite":4867,"proto":"h2","server_name":"a.test","ech":false}},"bytes_read":0,"user_id":"","duration":0.00216425,"size":17,"status":200,"resp_headers":{"X-Powered-By":["PHP/8.4.23"],"Content-Type":["text/html; charset=UTF-8"],"Via":["0.0 Caddy"]}}"#;

    fn line(uri: &str, status: u16, method: &str) -> String {
        format!(
            r#"{{"level":"info","ts":1792502416.2,"logger":"http.log.access.log0","msg":"handled request","request":{{"remote_ip":"127.0.0.1","proto":"HTTP/1.1","method":"{method}","host":"shop.test","uri":"{uri}","headers":{{}}}},"duration":0.001,"size":1,"status":{status},"resp_headers":{{}}}}"#
        )
    }

    fn temp(body: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("shop.log");
        std::fs::write(&p, body).unwrap();
        (dir, p)
    }

    #[test]
    fn parses_caddys_access_log_line() {
        let r = parse(LINE).unwrap();
        assert_eq!(r.method, "GET");
        assert_eq!(r.uri, "/index.php?q=1");
        assert_eq!(r.host, "a.test:18443");
        assert_eq!(r.proto, "HTTP/2.0");
        assert_eq!(r.status, 200);
        assert_eq!(r.size, 17);
        assert!((r.duration_ms - 2.16425).abs() < 1e-9);
        assert_eq!(r.ts, 1792502407.35184);
        assert_eq!(r.remote, "127.0.0.1");
        assert!(r.tls);
        assert_eq!(r.req_headers["Cookie"], ["REDACTED"]);
        assert_eq!(r.resp_headers["X-Powered-By"], ["PHP/8.4.23"]);
        assert!(!parse(&line("/", 200, "GET")).unwrap().tls);
    }

    #[test]
    fn other_lines_are_not_requests() {
        assert!(parse("").is_none());
        assert!(parse("not json").is_none());
        assert!(parse(r#"{"level":"info","msg":"server running"}"#).is_none());
        // What a cleared file holds before Caddy's next line: its old length in zeros.
        assert!(parse(&format!("\0\0\0{}", line("/", 200, "GET"))).is_some());
    }

    #[test]
    fn assets_are_vites_nexts_maps_and_the_favicon() {
        for a in [
            "/build/assets/app-x.js",
            "/_next/static/chunks/a.js",
            "/@vite/client",
            "/node_modules/.vite/deps/vue.js?v=1",
            "/js/app.js.map",
            "/favicon.ico",
        ] {
            assert!(is_asset(a), "{a}");
        }
        for p in ["/", "/login?next=/build/", "/api/users", "/builder"] {
            assert!(!is_asset(p), "{p}");
        }
    }

    #[test]
    fn filter_by_status_class_method_text_and_assets() {
        let r = |uri, status, method| parse(&line(uri, status, method)).unwrap();
        let all = Filter {
            assets: true,
            ..Filter::default()
        };
        assert!(all.matches(&r("/build/a.js", 200, "GET")));
        assert!(!Filter::default().matches(&r("/build/a.js", 200, "GET")));
        let f = Filter {
            status: Some("4xx".into()),
            ..all.clone()
        };
        assert!(f.matches(&r("/", 419, "POST")) && !f.matches(&r("/", 200, "GET")));
        let f = Filter {
            method: Some("post".into()),
            ..all.clone()
        };
        assert!(f.matches(&r("/", 419, "POST")) && !f.matches(&r("/", 419, "GET")));
        let f = Filter {
            query: "Login".into(),
            ..all.clone()
        };
        assert!(f.matches(&r("/login?x=1", 200, "GET")) && !f.matches(&r("/", 200, "GET")));
    }

    #[test]
    fn reads_newest_first_and_pages_back_without_skipping() {
        let body: String = (0..10)
            .map(|i| line(&format!("/p{i}"), 200, "GET") + "\n")
            .collect();
        let (_d, p) = temp(&body);
        let f = Filter::default();
        let first = read(&p, &f, None, 4).unwrap();
        let uris = |pg: &Page| {
            pg.requests
                .iter()
                .map(|r| r.uri.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(uris(&first), ["/p9", "/p8", "/p7", "/p6"]);
        assert!(first.truncated);
        assert_eq!(first.end_offset, body.len() as u64);
        let next = read(&p, &f, Some(first.start_offset), 100).unwrap();
        assert_eq!(uris(&next), ["/p5", "/p4", "/p3", "/p2", "/p1", "/p0"]);
        assert!(!next.truncated);
        let errors = read(
            &p,
            &Filter {
                status: Some("5xx".into()),
                ..f
            },
            None,
            4,
        )
        .unwrap();
        assert!(errors.requests.is_empty() && !errors.truncated);
    }

    #[test]
    fn a_missing_log_has_no_requests() {
        let page = read(
            Path::new("/nonexistent/anvil/x.log"),
            &Filter::default(),
            None,
            5,
        );
        assert!(page.unwrap().requests.is_empty());
    }

    #[test]
    fn tail_reads_whole_lines_and_starts_over_when_the_file_rolled() {
        let (_d, p) = temp(&(line("/a", 200, "GET") + "\n"));
        let f = Filter::default();
        let end = read(&p, &f, None, 10).unwrap().end_offset;
        let mut file = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
        let half = line("/c", 200, "GET");
        write!(file, "{}\n{}", line("/b", 200, "GET"), &half[..20]).unwrap();
        let t = tail(&p, end, &f).unwrap();
        assert_eq!(t.requests.len(), 1);
        assert_eq!(t.requests[0].uri, "/b");
        writeln!(file, "{}", &half[20..]).unwrap();
        let t2 = tail(&p, t.end_offset, &f).unwrap();
        assert_eq!(t2.requests[0].uri, "/c");
        // Rolled: a new, shorter file.
        std::fs::write(&p, line("/new", 200, "GET") + "\n").unwrap();
        assert_eq!(tail(&p, t2.end_offset, &f).unwrap().requests[0].uri, "/new");
    }

    #[test]
    fn a_cleared_log_reads_past_the_zeros_caddy_leaves() {
        // Caddy keeps writing at its old offset after the file is emptied.
        let mut body = "\0".repeat(300_000);
        body += &(line("/first", 200, "GET") + "\n");
        body += &(line("/second", 200, "GET") + "\n");
        let (_d, p) = temp(&body);
        let f = Filter::default();
        let mut uris = vec![];
        let mut before = None;
        loop {
            let pg = read(&p, &f, before, 1).unwrap();
            uris.extend(pg.requests.into_iter().map(|r| r.uri));
            if !pg.truncated {
                break;
            }
            before = Some(pg.start_offset);
        }
        assert_eq!(uris, ["/second", "/first"]);
    }
}
