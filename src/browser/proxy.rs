//! The browser bridge for apps that run their own server (Vite, Next, Nuxt, a server row,
//! Octane): Caddy sends their page loads here (`browser::doc_route`), and only those;
//! assets, HMR websockets and API calls go straight to the app. This asks the app on the
//! loopback port Caddy names in `X-Anvil-Upstream`, without compression, and puts the
//! same script tag PHP's `inject.php` does before `</head>`, streaming. When the app
//! doesn't answer, the connection closes unanswered, so Caddy's 502 handling (the "not
//! running" page, Octane's PHP-FPM fallback) runs as without the bridge.

use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// The header Caddy names the app's port in; never passed on.
pub const UPSTREAM: &str = "X-Anvil-Upstream";
/// The longest head of a request or an answer.
const MAX_HEAD: u64 = 64 * 1024;
/// An open tag at the end of what arrived is held back at most this long (a `<` in an
/// inline script); then the page goes on as it is.
const HOLD: usize = 16 * 1024;
const BLOCKED: &[u8] = b"X-Anvil-Bridge: blocked by Content-Security-Policy\r\n";

#[cfg(unix)]
fn socket() -> std::path::PathBuf {
    crate::home().join("run").join("browser.sock")
}

/// Windows: a loopback port of its own, given out as php-cgi's pools get theirs.
#[cfg(windows)]
fn port() -> u16 {
    crate::cgi_ports("browser").start
}

/// Where Caddy sends the page loads: a socket in Anvil's `run` folder that only this
/// user can open; on Windows a loopback port.
pub fn address() -> String {
    #[cfg(unix)]
    return format!("unix/{}", crate::caddy_path(&socket()));
    #[cfg(windows)]
    return format!("127.0.0.1:{}", port());
}

/// The proxy, listening until dropped. It lives in the process that runs Caddy, while an
/// app it serves has its bridge on (`browser::proxied`).
pub struct Proxy {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    #[cfg(unix)]
    path: std::path::PathBuf,
}

impl Proxy {
    pub fn start() -> io::Result<Proxy> {
        #[cfg(unix)]
        let (l, path) = {
            let p = socket();
            // One a crashed Anvil left behind.
            let _ = std::fs::remove_file(&p);
            let l = crate::platform::UnixListener::bind(&p)?;
            crate::platform::set_mode(&p, 0o600)?;
            (l, p)
        };
        #[cfg(windows)]
        let l = std::net::TcpListener::bind(("127.0.0.1", port()))?;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = std::thread::spawn(move || {
            for c in l.incoming() {
                if flag.load(Ordering::SeqCst) {
                    return;
                }
                let Ok(c) = c else { continue };
                std::thread::spawn(move || {
                    let _ = serve(&c);
                });
            }
        });
        Ok(Proxy {
            stop,
            thread: Some(thread),
            #[cfg(unix)]
            path,
        })
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wakes `accept`; the thread then returns and closes the listener. Page loads
        // under way finish on their own threads.
        #[cfg(unix)]
        let _ = crate::platform::UnixStream::connect(&self.path);
        #[cfg(windows)]
        let _ = TcpStream::connect(("127.0.0.1", port()));
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        #[cfg(unix)]
        let _ = std::fs::remove_file(&self.path);
    }
}

/// A head's lines without their line ends: the request or status line, then the
/// fields. Empty at the end of the stream.
fn head(r: &mut impl BufRead) -> io::Result<Vec<Vec<u8>>> {
    let mut lines = vec![];
    let mut r = r.take(MAX_HEAD);
    loop {
        let mut l = vec![];
        if r.read_until(b'\n', &mut l)? == 0 {
            return match lines.is_empty() {
                true => Ok(lines),
                false => Err(io::ErrorKind::UnexpectedEof.into()),
            };
        }
        while matches!(l.last(), Some(b'\n' | b'\r')) {
            l.pop();
        }
        match (l.is_empty(), lines.is_empty()) {
            (true, true) => continue,
            (true, false) => return Ok(lines),
            _ => lines.push(l),
        }
    }
}

fn name(line: &[u8]) -> String {
    let n = line.split(|&b| b == b':').next().unwrap_or_default();
    String::from_utf8_lossy(n).trim().to_ascii_lowercase()
}

/// The values of a head's field `name` (lowercase).
fn values<'a>(h: &'a [Vec<u8>], name: &'a str) -> impl Iterator<Item = String> + 'a {
    h.iter().skip(1).filter_map(move |l| {
        let at = l.iter().position(|&b| b == b':')?;
        (self::name(l) == name).then(|| String::from_utf8_lossy(&l[at + 1..]).trim().to_string())
    })
}

fn chunked(h: &[Vec<u8>]) -> bool {
    values(h, "transfer-encoding").any(|v| v.to_ascii_lowercase().contains("chunked"))
}

/// The app's address: one loopback address and port, or nothing.
fn upstream(req: &[Vec<u8>]) -> Option<SocketAddr> {
    let named: Vec<String> = values(req, "x-anvil-upstream").collect();
    let [one] = named.as_slice() else {
        return None;
    };
    one.parse::<SocketAddr>()
        .ok()
        .filter(|a| a.ip().is_loopback())
}

/// Answer one request from Caddy.
fn serve<S>(conn: &S) -> io::Result<()>
where
    for<'a> &'a S: Read + Write,
{
    let mut client = BufReader::new(conn);
    let mut w = conn;
    let req = head(&mut client)?;
    let Some(first) = req.first() else {
        return Ok(());
    };
    let line = String::from_utf8_lossy(first).into_owned();
    let mut parts = line.split(' ');
    let (Some(method), Some(target), Some(_), Some(addr)) =
        (parts.next(), parts.next(), parts.next(), upstream(&req))
    else {
        return w.write_all(
            b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
    };
    let Ok(up) = TcpStream::connect(addr) else {
        // Unanswered: Caddy then answers 502 with the app's "not running" page, or
        // Octane's PHP-FPM fallback, as it does without the bridge.
        return Ok(());
    };
    up.set_nodelay(true)?;
    let mut upw = &up;

    // HTTP/1.1, closed after one answer; uncompressed, so the page can be read.
    // ponytail: a connection per page load on each side (+0.3 ms on Vite's index.html) and
    // no gzip from the app (Next's dev server renders ~1/3 fewer pages a second at 8 at
    // once); keep-alive to the app, or gunzipping here, if page-load rates ever matter.
    let body_chunked = chunked(&req);
    let mut ask = format!("{method} {target} HTTP/1.1\r\n").into_bytes();
    for l in &req[1..] {
        let n = name(l);
        let hop = matches!(
            n.as_str(),
            "x-anvil-upstream"
                | "accept-encoding"
                | "connection"
                | "keep-alive"
                | "proxy-connection"
                | "te"
                | "trailer"
                | "upgrade"
                | "expect"
        );
        if !hop && !(body_chunked && n == "content-length") {
            ask.extend_from_slice(l);
            ask.extend_from_slice(b"\r\n");
        }
    }
    ask.extend_from_slice(b"Connection: close\r\n\r\n");
    upw.write_all(&ask)?;
    if body_chunked {
        let mut body = Chunked::new(&mut client);
        let mut buf = [0u8; 16384];
        loop {
            let n = body.read(&mut buf)?;
            if n == 0 {
                break;
            }
            upw.write_all(format!("{n:x}\r\n").as_bytes())?;
            upw.write_all(&buf[..n])?;
            upw.write_all(b"\r\n")?;
        }
        upw.write_all(b"0\r\n\r\n")?;
    } else if let Some(n) = values(&req, "content-length").find_map(|v| v.parse::<u64>().ok()) {
        io::copy(&mut (&mut client).take(n), &mut upw)?;
    }

    let mut ur = BufReader::new(&up);
    let status = |h: &[Vec<u8>]| {
        let l = String::from_utf8_lossy(h.first()?).into_owned();
        l.split(' ').nth(1)?.parse::<u16>().ok()
    };
    let mut resp = head(&mut ur)?;
    // An interim answer (103 Early Hints) is left out; the final one follows.
    while status(&resp).is_some_and(|s| (100..200).contains(&s)) {
        resp = head(&mut ur)?;
    }
    let Some(code) = status(&resp) else {
        return Ok(());
    };
    let https = values(&req, "x-forwarded-proto").any(|p| p.eq_ignore_ascii_case("https"));
    let host = values(&req, "host")
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let bodyless = method == "HEAD" || code == 204 || code == 304;
    // As `inject.php`: an HTML page, an error page too, but no redirect or download.
    let mut page = !bodyless
        && ((200..300).contains(&code) || code >= 400)
        && values(&resp, "content-type").any(|t| t.to_ascii_lowercase().starts_with("text/html"))
        && !values(&resp, "content-encoding").any(|e| !e.eq_ignore_ascii_case("identity"))
        && values(&resp, "content-disposition").next().is_none();
    let mut blocked = false;
    if page && !values(&resp, "content-security-policy").all(|p| allows(&p, https, &host)) {
        (page, blocked) = (false, true);
    }
    let resp_chunked = chunked(&resp);
    let len = values(&resp, "content-length").find_map(|v| v.parse::<u64>().ok());

    // The answer as the app gave it, but for its framing (close-delimited, unchunked) and,
    // for a page that grows, its length.
    let mut h = b"HTTP/1.1".to_vec();
    h.extend_from_slice(
        &resp[0][resp[0]
            .iter()
            .position(|&b| b == b' ')
            .unwrap_or(resp[0].len())..],
    );
    h.extend_from_slice(b"\r\n");
    for l in &resp[1..] {
        let n = name(l);
        let framing = matches!(
            n.as_str(),
            "connection" | "keep-alive" | "proxy-connection" | "transfer-encoding"
        );
        if !framing && !(n == "content-length" && (page || resp_chunked)) {
            h.extend_from_slice(l);
            h.extend_from_slice(b"\r\n");
        }
    }
    let mut body: Box<dyn Read + '_> = match (bodyless, resp_chunked, len) {
        (true, ..) => Box::new(io::empty()),
        (_, true, _) => Box::new(Chunked::new(ur)),
        (_, _, Some(n)) => Box::new(ur.take(n)),
        _ => Box::new(ur),
    };
    if !page {
        if blocked {
            h.extend_from_slice(BLOCKED);
        }
        h.extend_from_slice(b"Connection: close\r\n\r\n");
        w.write_all(&h)?;
        io::copy(&mut body, &mut w)?;
        return Ok(());
    }
    let mut inject = Inject::new(crate::browser::tag(), https, &host);
    let mut buf = vec![0u8; 16384];
    // The head goes with the page's first bytes, so a `<meta>` policy there still says so.
    let mut head = Some(h);
    loop {
        let n = body.read(&mut buf)?;
        let mut out = match n {
            0 => inject.finish(),
            n => inject.push(&buf[..n]),
        };
        if let Some(mut h) = head.take() {
            if inject.blocked {
                h.extend_from_slice(BLOCKED);
            }
            h.extend_from_slice(b"Connection: close\r\n\r\n");
            h.append(&mut out);
            out = h;
        }
        w.write_all(&out)?;
        if n == 0 {
            return Ok(());
        }
    }
}

/// A chunked body, read as the bytes it carries.
struct Chunked<R> {
    r: R,
    /// Of the chunk being read.
    left: u64,
    /// A chunk was read whose line end isn't yet.
    crlf: bool,
    done: bool,
}

impl<R: BufRead> Chunked<R> {
    fn new(r: R) -> Chunked<R> {
        Chunked {
            r,
            left: 0,
            crlf: false,
            done: false,
        }
    }

    fn line(&mut self) -> io::Result<String> {
        let mut l = vec![];
        if (&mut self.r).take(MAX_HEAD).read_until(b'\n', &mut l)? == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        Ok(String::from_utf8_lossy(&l).trim().to_string())
    }
}

impl<R: BufRead> Read for Chunked<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.done || buf.is_empty() {
            return Ok(0);
        }
        if self.left == 0 {
            // Read here, not after the chunk, so a chunk goes on before its line end comes.
            if self.crlf {
                self.line()?;
                self.crlf = false;
            }
            let size = self.line()?;
            let size = size.split(';').next().unwrap_or_default().trim();
            self.left = u64::from_str_radix(size, 16)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "bad chunk size"))?;
            if self.left == 0 {
                // Trailers, up to the empty line.
                while !self.line()?.is_empty() {}
                self.done = true;
                return Ok(0);
            }
        }
        let max = buf
            .len()
            .min(usize::try_from(self.left).unwrap_or(usize::MAX));
        let n = self.r.read(&mut buf[..max])?;
        if n == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        self.left -= n as u64;
        self.crlf = self.left == 0;
        Ok(n)
    }
}

/// True when a Content-Security-Policy lets a same-origin script run and send to its
/// origin, as `inject.php` decides it. A nonce or hash with 'strict-dynamic' ignores 'self'.
fn allows(policy: &str, https: bool, host: &str) -> bool {
    let mut d: HashMap<String, Vec<String>> = HashMap::new();
    for part in policy.split(';') {
        let mut words = part.split_whitespace().map(str::to_ascii_lowercase);
        if let Some(name) = words.next() {
            d.entry(name).or_insert_with(|| words.collect());
        }
    }
    let ok = |names: &[&str]| {
        let Some(src) = names.iter().find_map(|n| d.get(*n)) else {
            return true;
        };
        if src.iter().any(|s| s == "'strict-dynamic'") {
            return false;
        }
        src.iter().any(|s| {
            if s == "'self'"
                || s == "*"
                || s == if https { "https:" } else { "http:" }
                || (https && s == "http:")
            {
                return true;
            }
            // A host source naming the page's host (Host has a port only when it isn't
            // the default one, as a source must then too).
            let s = s
                .strip_prefix("https://")
                .or_else(|| s.strip_prefix("http://"))
                .unwrap_or(s)
                .trim_end_matches('/');
            s == host
                || s.strip_prefix('*')
                    .is_some_and(|d| d.starts_with('.') && host.ends_with(d))
        })
    };
    ok(&["script-src-elem", "script-src", "default-src"]) && ok(&["connect-src", "default-src"])
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len())
        .position(|w| w.eq_ignore_ascii_case(needle))
}

/// An attribute's value in a tag, by its lowercase name; `None` when it isn't there.
fn attr(tag: &[u8], name: &[u8]) -> Option<String> {
    let mut at = 0;
    while let Some(i) = find(&tag[at..], name) {
        let start = at + i;
        at = start + name.len();
        if !tag
            .get(start.wrapping_sub(1))
            .is_some_and(|b| b.is_ascii_whitespace() || *b == b'"' || *b == b'\'')
        {
            continue;
        }
        let rest = &tag[at..];
        let skip = |r: &[u8]| {
            r.iter()
                .position(|b| !b.is_ascii_whitespace())
                .unwrap_or(r.len())
        };
        let rest = &rest[skip(rest)..];
        let Some(rest) = rest.strip_prefix(b"=") else {
            continue;
        };
        let rest = &rest[skip(rest)..];
        let value = match rest.first() {
            Some(&q @ (b'"' | b'\'')) => {
                let v = &rest[1..];
                &v[..v.iter().position(|&b| b == q).unwrap_or(v.len())]
            }
            _ => {
                &rest[..rest
                    .iter()
                    .position(|b| b.is_ascii_whitespace() || *b == b'>')
                    .unwrap_or(rest.len())]
            }
        };
        return Some(
            String::from_utf8_lossy(value)
                .replace("&quot;", "\"")
                .replace("&#39;", "'")
                .replace("&apos;", "'")
                .replace("&amp;", "&"),
        );
    }
    None
}

/// The script tag put into one page, streaming: bytes go on as they come, but for a tag
/// still open at the end of what arrived (`</head>` or a `<meta>` split across reads).
struct Inject {
    tag: String,
    https: bool,
    host: String,
    held: Vec<u8>,
    done: bool,
    /// The page's own `<meta>` policy keeps the script out.
    blocked: bool,
}

impl Inject {
    fn new(tag: String, https: bool, host: &str) -> Inject {
        Inject {
            tag,
            https,
            host: host.to_ascii_lowercase(),
            held: vec![],
            done: false,
            blocked: false,
        }
    }

    /// What of the page can go on now.
    fn push(&mut self, data: &[u8]) -> Vec<u8> {
        if self.done {
            return data.to_vec();
        }
        self.held.extend_from_slice(data);
        let cut = match self.held.iter().rposition(|&b| b == b'<') {
            Some(i) if !self.held[i..].contains(&b'>') && self.held.len() - i <= HOLD => i,
            _ => self.held.len(),
        };
        let rest = self.held.split_off(cut);
        let mut out = std::mem::replace(&mut self.held, rest);
        let end = find(&out, b"</head>");
        if self.meta_blocks(&out[..end.unwrap_or(out.len())]) {
            self.blocked = true;
        } else if let Some(i) = end {
            out.splice(i..i, self.tag.bytes());
        } else if find(&out, b"<body").is_none() {
            return out;
        }
        // Put in, kept out, or no head to put it in: the rest is passed on as it is.
        self.done = true;
        out.append(&mut self.held);
        out
    }

    fn finish(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.held)
    }

    /// A `<meta http-equiv="Content-Security-Policy">` in `html` keeps the script out.
    fn meta_blocks(&self, html: &[u8]) -> bool {
        let mut at = 0;
        while let Some(i) = find(&html[at..], b"<meta") {
            let start = at + i;
            let end = html[start..]
                .iter()
                .position(|&b| b == b'>')
                .map_or(html.len(), |e| start + e);
            let tag = &html[start..end];
            at = end;
            let csp = attr(tag, b"http-equiv")
                .is_some_and(|v| v.trim().eq_ignore_ascii_case("content-security-policy"));
            if csp
                && !allows(
                    &attr(tag, b"content").unwrap_or_default(),
                    self.https,
                    &self.host,
                )
            {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::time::{Duration, Instant};

    const TAG: &str = r#"<script src="/__anvil/a.js?v=1"></script>"#;

    #[test]
    fn a_policy_lets_the_script_in_as_inject_php_decides() {
        for (policy, https, yes) in [
            ("", false, true),
            ("default-src 'self'", false, true),
            ("default-src 'none'", false, false),
            ("default-src 'none'; script-src 'self'", false, false),
            (
                "default-src 'none'; script-src 'self'; connect-src 'self'",
                false,
                true,
            ),
            ("script-src 'nonce-abc' 'strict-dynamic'", false, false),
            ("script-src 'self' 'strict-dynamic'", false, false),
            ("script-src https:; connect-src *", true, true),
            ("script-src https:; connect-src *", false, false),
            ("script-src http:", true, true),
            ("default-src shop.test", false, true),
            ("default-src https://shop.test/", true, true),
            ("default-src *.test", false, true),
            ("default-src other.test", false, false),
            // The first of a directive counts; script-src-elem before script-src.
            ("script-src 'none'; script-src 'self'", false, false),
            ("script-src 'none'; script-src-elem 'self'", false, true),
            ("SCRIPT-SRC 'SELF'", false, true),
        ] {
            assert_eq!(
                allows(policy, https, "shop.test"),
                yes,
                "{policy} https={https}"
            );
        }
    }

    fn run(chunks: &[&str]) -> (String, Inject) {
        let mut i = Inject::new(TAG.into(), false, "shop.test");
        let mut out = vec![];
        for c in chunks {
            out.extend(i.push(c.as_bytes()));
        }
        out.extend(i.finish());
        (String::from_utf8(out).unwrap(), i)
    }

    #[test]
    fn the_tag_goes_before_the_head_end_even_split_across_reads() {
        let page = "<!doctype html><html><head><title>x</title></HEAD><body>hi</body></html>";
        let want = page.replace("</HEAD>", &format!("{TAG}</HEAD>"));
        for at in 1..page.len() {
            assert_eq!(run(&[&page[..at], &page[at..]]).0, want, "split at {at}");
        }
        // Once: a second </head> later is left alone.
        let two = "<head></head><body>`</head>`</body>";
        assert_eq!(run(&[two]).0.matches(TAG).count(), 1);
    }

    #[test]
    fn vites_client_stays_first() {
        let page = r#"<html><head><script type="module" src="/@vite/client"></script><title>v</title></head>"#;
        let out = run(&[page]).0;
        assert!(
            out.find("/@vite/client").unwrap() < out.find(TAG).unwrap(),
            "{out}"
        );
    }

    #[test]
    fn it_streams_holding_back_only_an_open_tag() {
        let mut i = Inject::new(TAG.into(), false, "shop.test");
        assert_eq!(i.push(b"<html><head><title>x</ti"), b"<html><head><title>x");
        assert_eq!(
            i.push(b"tle><meta charset=utf-8></he"),
            b"</title><meta charset=utf-8>"
        );
        assert_eq!(
            i.push(b"ad><body>"),
            format!("{TAG}</head><body>").as_bytes()
        );
        // Done: the rest goes as it comes.
        assert_eq!(i.push(b"<p>late</p"), b"<p>late</p");
        assert!(i.finish().is_empty());
    }

    #[test]
    fn a_page_without_a_head_end_is_left_as_it_is() {
        let page = "<html><body><p>no head</p></body></html>";
        assert_eq!(run(&[page]).0, page);
        // And the body is no longer looked through.
        let mut i = Inject::new(TAG.into(), false, "shop.test");
        i.push(b"<body>");
        assert!(i.done);
        assert_eq!(run(&["plain text, no tags"]).0, "plain text, no tags");
    }

    #[test]
    fn a_meta_policy_that_blocks_keeps_the_tag_out() {
        let page = r#"<head><meta http-equiv="Content-Security-Policy" content="script-src 'nonce-x' 'strict-dynamic'"></head>"#;
        let (out, i) = run(&[&page[..30], &page[30..]]);
        assert_eq!(out, page);
        assert!(i.blocked);
        let ok = r#"<head><meta http-equiv='content-security-policy' content='default-src &apos;self&apos;'></head>"#;
        let (out, i) = run(&[ok]);
        assert!(out.contains(TAG) && !i.blocked, "{out}");
    }

    // A fake app: answers each connection with `reply`, after sending back what it got.
    fn app(
        reply: &'static [&'static [u8]],
        gap: Duration,
    ) -> (u16, std::sync::mpsc::Receiver<String>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for c in l.incoming() {
                let mut c = c.unwrap();
                let mut r = BufReader::new(c.try_clone().unwrap());
                let mut got = String::new();
                loop {
                    let mut l = String::new();
                    if r.read_line(&mut l).unwrap() == 0 || l == "\r\n" {
                        got += &l;
                        break;
                    }
                    got += &l;
                }
                let len = got
                    .lines()
                    .find_map(|l| l.strip_prefix("Content-Length: "))
                    .and_then(|n| n.trim().parse::<usize>().ok());
                if got.contains("Transfer-Encoding: chunked") {
                    let mut body = String::new();
                    loop {
                        let mut l = String::new();
                        r.read_line(&mut l).unwrap();
                        body += &l;
                        if l == "0\r\n" {
                            r.read_line(&mut l).unwrap();
                            break;
                        }
                    }
                    got += &body;
                } else if let Some(n) = len {
                    let mut b = vec![0; n];
                    r.read_exact(&mut b).unwrap();
                    got += &String::from_utf8(b).unwrap();
                }
                tx.send(got).unwrap();
                for part in reply {
                    c.write_all(part).unwrap();
                    std::thread::sleep(gap);
                }
            }
        });
        (port, rx)
    }

    /// One request through `serve`, as Caddy sends it; the answer as it arrived, with when
    /// each part did.
    fn ask(req: String) -> Vec<(Duration, Vec<u8>)> {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        std::thread::spawn(move || {
            let (c, _) = l.accept().unwrap();
            let _ = serve(&c);
        });
        let mut c = TcpStream::connect(addr).unwrap();
        c.write_all(req.as_bytes()).unwrap();
        let start = Instant::now();
        let mut parts = vec![];
        let mut buf = [0; 65536];
        loop {
            let n = c.read(&mut buf).unwrap_or(0);
            if n == 0 {
                return parts;
            }
            parts.push((start.elapsed(), buf[..n].to_vec()));
        }
    }

    fn text(parts: &[(Duration, Vec<u8>)]) -> String {
        String::from_utf8(parts.iter().flat_map(|(_, b)| b.clone()).collect()).unwrap()
    }

    fn get(port: u16, extra: &str) -> String {
        format!(
            "GET /p?q=1 HTTP/1.1\r\nHost: shop.test\r\nAccept: text/html\r\nAccept-Encoding: gzip, br\r\nX-Forwarded-Proto: https\r\n{UPSTREAM}: 127.0.0.1:{port}\r\n{extra}\r\n"
        )
    }

    #[test]
    fn a_page_gets_the_tag_and_the_app_never_sees_anvils_header_or_compression() {
        let (port, seen) = app(
            &[b"HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: 41\r\nSet-Cookie: a=1\r\nSet-Cookie: b=2\r\n\r\n<html><head></head><body>hi</body></html>"],
            Duration::ZERO,
        );
        let out = text(&ask(get(port, "")));
        let got = seen.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(got.starts_with("GET /p?q=1 HTTP/1.1\r\n"), "{got}");
        assert!(got.contains("Host: shop.test\r\n"), "{got}");
        assert!(!got.to_ascii_lowercase().contains("x-anvil"), "{got}");
        assert!(!got.contains("Accept-Encoding"), "{got}");
        assert!(got.contains("Connection: close\r\n"), "{got}");
        let (head, body) = out.split_once("\r\n\r\n").unwrap();
        assert!(head.starts_with("HTTP/1.1 200 OK\r\n"), "{head}");
        assert!(
            head.contains("Set-Cookie: a=1\r\nSet-Cookie: b=2\r\n"),
            "{head}"
        );
        assert!(!head.contains("Content-Length"), "{head}");
        assert_eq!(
            body,
            format!("<html><head>{TAG}</head><body>hi</body></html>")
                .replace("v=1", &format!("v={}", crate::browser::version()))
        );
    }

    #[test]
    fn a_chunked_streaming_page_is_unchunked_and_keeps_its_timing() {
        let (port, _seen) = app(
            &[
                b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nTransfer-Encoding: chunked\r\n\r\n",
                b"13\r\n<html><head></head>\r\n",
                b"6\r\n<body>\r\n",
                b"7\r\n</body>\r\n0\r\n\r\n",
            ],
            Duration::from_millis(150),
        );
        let parts = ask(get(port, ""));
        let out = text(&parts);
        let body = out.split_once("\r\n\r\n").unwrap().1;
        assert!(body.starts_with("<html><head><script"), "{body}");
        assert!(body.ends_with("</head><body></body>"), "{body}");
        // The page's start came before its end did.
        let first = parts
            .iter()
            .find(|(_, b)| b.windows(6).any(|w| w == b"<head>"))
            .unwrap()
            .0;
        assert!(first < Duration::from_millis(250), "{first:?}");
        assert!(parts.last().unwrap().0 >= Duration::from_millis(300));
    }

    #[test]
    fn redirects_json_and_downloads_pass_unchanged() {
        for reply in [
            &b"HTTP/1.1 302 Found\r\nLocation: /login\r\nSet-Cookie: s=1\r\nContent-Type: text/html\r\nContent-Length: 13\r\n\r\n<head></head>"[..],
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 15\r\n\r\n{\"a\":\"</head>\"}",
            b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Disposition: attachment\r\nContent-Length: 13\r\n\r\n<head></head>",
        ] {
            let r: &'static [u8] = Box::leak(reply.to_vec().into_boxed_slice());
            let (port, _seen) = app(Box::leak(Box::new([r])), Duration::ZERO);
            let out = text(&ask(get(port, "")));
            assert_eq!(out.split_once("\r\n\r\n").unwrap().1, String::from_utf8_lossy(reply).split_once("\r\n\r\n").unwrap().1);
            assert!(out.contains("Content-Length: "), "{out}");
            assert!(!out.contains("<script"), "{out}");
        }
    }

    #[test]
    fn a_policy_header_that_blocks_says_so() {
        let (port, _seen) = app(
            &[b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Security-Policy: script-src 'nonce-a' 'strict-dynamic'\r\n\r\n<head></head>"],
            Duration::ZERO,
        );
        let out = text(&ask(get(port, "")));
        assert!(
            out.contains("\r\nX-Anvil-Bridge: blocked by Content-Security-Policy\r\n"),
            "{out}"
        );
        assert!(out.ends_with("\r\n\r\n<head></head>"), "{out}");
    }

    #[test]
    fn a_form_post_reaches_the_app_with_its_body() {
        let (port, seen) = app(
            &[b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n<head></head>ok"],
            Duration::ZERO,
        );
        let req = get(
            port,
            "Content-Type: application/x-www-form-urlencoded\r\nContent-Length: 7\r\n",
        )
        .replacen("GET", "POST", 1)
            + "a=1&b=2";
        let out = text(&ask(req));
        assert!(
            seen.recv_timeout(Duration::from_secs(5))
                .unwrap()
                .ends_with("\r\n\r\na=1&b=2")
        );
        assert!(out.ends_with("</head>ok"), "{out}");
        // A chunked body goes on chunked.
        let (port, seen) = app(&[b"HTTP/1.1 204 No Content\r\n\r\n"], Duration::ZERO);
        let req = get(port, "Transfer-Encoding: chunked\r\n").replacen("GET", "POST", 1)
            + "3\r\na=1\r\n0\r\n\r\n";
        let out = text(&ask(req));
        let got = seen.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(got.ends_with("\r\n\r\n3\r\na=1\r\n0\r\n"), "{got:?}");
        assert!(out.starts_with("HTTP/1.1 204"), "{out}");
    }

    #[cfg(unix)]
    #[test]
    fn the_proxy_listens_on_its_socket_until_dropped() {
        let _g = crate::lock(&crate::ENV_LOCK);
        let home = tempfile::tempdir().unwrap();
        // SAFETY: every test that sets ANVIL_HOME holds ENV_LOCK.
        unsafe { std::env::set_var("ANVIL_HOME", home.path()) };
        std::fs::create_dir_all(home.path().join("run")).unwrap();
        let (port, _seen) = app(
            &[b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n<head></head>"],
            Duration::ZERO,
        );
        let p = Proxy::start().unwrap();
        assert_eq!(address(), format!("unix/{}", socket().display()));
        let mut c = crate::platform::UnixStream::connect(socket()).unwrap();
        c.write_all(get(port, "").as_bytes()).unwrap();
        let mut out = String::new();
        c.read_to_string(&mut out).unwrap();
        assert!(
            out.ends_with(&format!("{}</head>", crate::browser::tag())),
            "{out}"
        );
        drop(p);
        assert!(!socket().exists());
    }

    #[test]
    fn only_a_loopback_port_is_asked_and_a_stopped_app_gets_no_answer() {
        for bad in ["10.0.0.1:80", "example.com:80", "127.0.0.1", ""] {
            let out = text(&ask(format!(
                "GET / HTTP/1.1\r\nHost: shop.test\r\n{UPSTREAM}: {bad}\r\n\r\n"
            )));
            assert!(out.starts_with("HTTP/1.1 400"), "{bad}: {out}");
        }
        let out = text(&ask("GET / HTTP/1.1\r\nHost: shop.test\r\n\r\n".into()));
        assert!(out.starts_with("HTTP/1.1 400"), "{out}");
        // Two of them: refused too.
        let out = text(&ask(format!(
            "GET / HTTP/1.1\r\n{UPSTREAM}: 127.0.0.1:1\r\n{UPSTREAM}: 127.0.0.1:2\r\n\r\n"
        )));
        assert!(out.starts_with("HTTP/1.1 400"), "{out}");
        // Nothing listens (on port 1): closed unanswered, so Caddy runs its 502 handling.
        assert!(ask(get(1, "")).is_empty());
    }
}
