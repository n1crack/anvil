//! Settings › Plugins › Mail catcher: an SMTP listener on 127.0.0.1 that only accepts and
//! stores. Each app's processes and PHP get `MAIL_*` with the app's name as the SMTP
//! username (`env`), so its mail lands in its own inbox, `~/.anvil/mail/<app>/`.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub const DEFAULT_PORT: u16 = 2525;
/// Mail whose SMTP username is no app's. Never a site name: those hold no `_`.
pub const OTHER: &str = "_other";

/// One stored message, as its inbox's index lists it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Summary {
    pub id: String,
    /// Milliseconds since 1970, when Anvil got it.
    pub ts: u64,
    pub from: String,
    pub to: Vec<String>,
    pub subject: String,
    pub size: u64,
    pub attachments: usize,
}

#[derive(Serialize, Debug, Clone)]
pub struct Attachment {
    pub index: usize,
    pub name: String,
    pub mime: String,
    pub size: usize,
    /// Shown inside the HTML (`cid:`) rather than offered as a file.
    pub inline: bool,
}

/// One message, read for the Mail tab, the CLI and agents.
#[derive(Serialize, Debug, Clone)]
pub struct Message {
    #[serde(flatten)]
    pub summary: Summary,
    pub cc: Vec<String>,
    /// The Date header, as written.
    pub date: Option<String>,
    pub text: Option<String>,
    /// With inline images (`cid:`) as `data:` URLs.
    pub html: Option<String>,
    pub headers: Vec<(String, String)>,
    pub attachments: Vec<Attachment>,
    /// http(s) links in the HTML, else in the text, in order, once each.
    pub links: Vec<String>,
}

pub fn port(cfg: &crate::Config) -> u16 {
    cfg.mail_port.unwrap_or(DEFAULT_PORT)
}

/// Whether the app's mail goes to the catcher: it's on and the app isn't left out.
pub fn caught(cfg: &crate::Config, site: &str) -> bool {
    cfg.mail_catcher && !cfg.sites.get(site).is_some_and(|s| s.mail_uncaught)
}

/// The `MAIL_*` an app gets while the catcher is on and the app isn't left out. Set
/// variables win over `.env` in Laravel (immutable Dotenv); Symfony reads `MAILER_DSN`.
pub fn env(cfg: &crate::Config, site: &crate::Site) -> Vec<(String, String)> {
    if !caught(cfg, &site.name) {
        return vec![];
    }
    let (port, user) = (port(cfg), &site.name);
    let url = format!("smtp://{user}:anvil@127.0.0.1:{port}");
    [
        ("MAIL_MAILER", "smtp".to_string()),
        ("MAIL_HOST", "127.0.0.1".into()),
        ("MAIL_PORT", port.to_string()),
        ("MAIL_USERNAME", user.clone()),
        ("MAIL_PASSWORD", "anvil".into()),
        ("MAIL_ENCRYPTION", "null".into()),
        ("MAIL_SCHEME", "null".into()),
        // Laravel reads its URL over the parts when one is set.
        ("MAIL_URL", url.clone()),
        ("MAILER_DSN", url),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

/// Where inboxes live.
pub fn root() -> PathBuf {
    crate::home().join("mail")
}

/// An inbox's folder; `None` for a name that can't be an app's (`site_name`) or `OTHER`.
pub fn inbox(root: &Path, name: &str) -> Option<PathBuf> {
    let ok = name == OTHER
        || (!name.is_empty() && !name.starts_with('.') && crate::site_name(name) == name);
    ok.then(|| root.join(name))
}

/// The inbox an SMTP username stores into.
fn inbox_for(root: &Path, user: Option<&str>, apps: &BTreeSet<String>) -> PathBuf {
    let app = user.filter(|u| apps.contains(*u));
    root.join(app.unwrap_or(OTHER))
}

pub(crate) const MAX_SIZE: usize = 25 * 1024 * 1024;
const KEEP: usize = 500;
const KEEP_BYTES: u64 = 50 * 1024 * 1024;
const INDEX: &str = "index.jsonl";

/// Stores, prunes and deletes one at a time, so the index and the files agree.
/// ponytail: one lock per process; a Delete in the app while `anvil start` owns the
/// listener (another process) may race a store and drop that one index line.
static WRITE: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn file(dir: &Path, id: &str) -> Result<PathBuf> {
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit() || b == b'-') {
        bail!("No message {id}");
    }
    Ok(dir.join(format!("{id}.eml")))
}

fn addr(a: &mail_parser::Addr) -> String {
    let email = a.address().unwrap_or_default();
    match a.name() {
        Some(n) if !n.is_empty() && n != email => format!("{n} <{email}>"),
        _ => email.to_string(),
    }
}

fn addrs(a: Option<&mail_parser::Address>) -> Vec<String> {
    a.map(|a| a.iter().map(addr).collect()).unwrap_or_default()
}

fn parse(raw: &[u8]) -> Option<mail_parser::Message<'_>> {
    mail_parser::MessageParser::default().parse(raw)
}

/// A file to offer for saving, not an image the HTML shows itself (`cid:`).
fn is_file(p: &mail_parser::MessagePart) -> bool {
    use mail_parser::MimeHeaders;
    p.content_id().is_none() || p.content_disposition().is_some_and(|d| d.is_attachment())
}

fn name_of(p: &mail_parser::MessagePart, index: usize) -> String {
    use mail_parser::MimeHeaders;
    p.attachment_name()
        .map(String::from)
        .unwrap_or_else(|| format!("attachment-{}", index + 1))
}

pub fn store(dir: &Path, raw: &[u8], envelope_from: &str, rcpts: &[String]) -> Result<Summary> {
    store_capped(dir, raw, envelope_from, rcpts, KEEP, KEEP_BYTES)
}

/// `store`, then the oldest dropped until at most `keep` messages and `keep_bytes` remain.
fn store_capped(
    dir: &Path,
    raw: &[u8],
    envelope_from: &str,
    rcpts: &[String],
    keep: usize,
    keep_bytes: u64,
) -> Result<Summary> {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let m = parse(raw);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let to = m.as_ref().map(|m| addrs(m.to())).unwrap_or_default();
    let s = Summary {
        id: format!("{ts}-{n}"),
        ts,
        from: m
            .as_ref()
            .and_then(|m| addrs(m.from()).into_iter().next())
            .unwrap_or_else(|| envelope_from.to_string()),
        to: if to.is_empty() { rcpts.to_vec() } else { to },
        subject: m
            .as_ref()
            .and_then(|m| m.subject())
            .unwrap_or_default()
            .to_string(),
        size: raw.len() as u64,
        attachments: m
            .as_ref()
            .map_or(0, |m| m.attachments().filter(|p| is_file(p)).count()),
    };
    let _one = crate::lock(&WRITE);
    std::fs::create_dir_all(dir)?;
    std::fs::write(file(dir, &s.id)?, raw)?;
    let mut all = read_index(dir);
    all.push(s.clone());
    let mut bytes: u64 = all.iter().map(|s| s.size).sum();
    let mut old = 0;
    // The newest always stays, however big.
    while all.len() - old > 1 && (all.len() - old > keep || bytes > keep_bytes) {
        bytes -= all[old].size;
        old += 1;
    }
    if old == 0 {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join(INDEX))?;
        writeln!(f, "{}", serde_json::to_string(&s)?)?;
    } else {
        for o in all.drain(..old) {
            let _ = std::fs::remove_file(file(dir, &o.id)?);
        }
        write_index(dir, &all)?;
    }
    Ok(s)
}

/// Oldest first.
fn read_index(dir: &Path) -> Vec<Summary> {
    std::fs::read_to_string(dir.join(INDEX))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

fn write_index(dir: &Path, all: &[Summary]) -> Result<()> {
    let mut text = String::new();
    for s in all {
        text += &serde_json::to_string(s)?;
        text.push('\n');
    }
    let tmp = dir.join(format!("{INDEX}.tmp"));
    std::fs::write(&tmp, text)?;
    Ok(std::fs::rename(tmp, dir.join(INDEX))?)
}

/// Newest first.
pub fn list(dir: &Path) -> Vec<Summary> {
    let mut all = read_index(dir);
    all.reverse();
    all
}

pub fn raw(dir: &Path, id: &str) -> Result<Vec<u8>> {
    std::fs::read(file(dir, id)?).with_context(|| format!("No message {id}"))
}

/// Standard base64, for the `data:` URLs of inline images.
pub(crate) fn b64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            out.push(if i <= c.len() {
                A[(n >> (18 - 6 * i) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    out
}

/// The http(s) `href`s of an HTML body, in order, once each: what a reader can follow,
/// not its images (a tracking pixel).
fn hrefs(html: &str) -> Vec<String> {
    let lower = html.to_ascii_lowercase();
    let mut out: Vec<String> = vec![];
    let mut at = 0;
    while let Some(i) = lower[at..].find("href=") {
        let start = at + i + 5;
        let rest = &html[start..];
        let (value, len) = match rest.chars().next() {
            Some(q @ ('"' | '\'')) => {
                let v = &rest[1..];
                let end = v.find(q).unwrap_or(v.len());
                (&v[..end], end + 1)
            }
            _ => {
                let end = rest
                    .find(|c: char| c.is_whitespace() || c == '>')
                    .unwrap_or(rest.len());
                (&rest[..end], end)
            }
        };
        let link = value.trim().replace("&amp;", "&");
        if (link.starts_with("http://") || link.starts_with("https://")) && !out.contains(&link) {
            out.push(link);
        }
        at = start + len;
    }
    out
}

/// http(s) links in plain text, in order, once each.
fn links(text: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    let mut rest = text;
    while let Some(i) = [rest.find("http://"), rest.find("https://")]
        .into_iter()
        .flatten()
        .min()
    {
        let tail = &rest[i..];
        let end = tail
            .find(|c: char| c.is_whitespace() || "\"'<>()[]{}`".contains(c))
            .unwrap_or(tail.len());
        let link = tail[..end].to_string();
        if !out.contains(&link) {
            out.push(link);
        }
        rest = &tail[end..];
    }
    out
}

pub fn read(dir: &Path, id: &str) -> Result<Message> {
    use mail_parser::MimeHeaders;
    let bytes = raw(dir, id)?;
    let summary = read_index(dir)
        .into_iter()
        .find(|s| s.id == id)
        .with_context(|| format!("No message {id}"))?;
    let Some(m) = parse(&bytes) else {
        return Ok(Message {
            summary,
            cc: vec![],
            date: None,
            text: Some(String::from_utf8_lossy(&bytes).into()),
            html: None,
            headers: vec![],
            attachments: vec![],
            links: vec![],
        });
    };
    let attachments: Vec<Attachment> = m
        .attachments()
        .enumerate()
        .map(|(index, p)| Attachment {
            index,
            name: name_of(p, index),
            mime: p
                .content_type()
                .map_or("application/octet-stream".into(), |c| {
                    format!("{}/{}", c.ctype(), c.subtype().unwrap_or("octet-stream"))
                }),
            size: p.contents().len(),
            inline: !is_file(p),
        })
        .collect();
    // The HTML part as written, not one made from the text part.
    let html = m
        .html_bodies()
        .any(|p| p.is_text_html())
        .then(|| m.body_html(0))
        .flatten()
        .map(|h| {
            let mut h = h.into_owned();
            for (p, a) in m.attachments().zip(&attachments) {
                if let Some(cid) = p.content_id().filter(|_| a.inline) {
                    let url = format!("data:{};base64,{}", a.mime, b64(p.contents()));
                    h = h.replace(&format!("cid:{cid}"), &url);
                }
            }
            h
        });
    let text = m
        .text_bodies()
        .any(|p| p.is_text() && !p.is_text_html())
        .then(|| m.body_text(0).map(|t| t.into_owned()))
        .flatten();
    Ok(Message {
        cc: addrs(m.cc()),
        date: m.header_raw("Date").map(|d| d.trim().to_string()),
        links: match (&html, &text) {
            (Some(h), _) => hrefs(h),
            (None, t) => links(t.as_deref().unwrap_or_default()),
        },
        headers: m
            .headers_raw()
            .map(|(k, v)| (k.to_string(), v.trim().to_string()))
            .collect(),
        text,
        html,
        attachments,
        summary,
    })
}

/// An attachment's file name and bytes.
pub fn attachment(dir: &Path, id: &str, index: usize) -> Result<(String, Vec<u8>)> {
    let bytes = raw(dir, id)?;
    let m = parse(&bytes).context("The message can't be read")?;
    let p = m
        .attachments()
        .nth(index)
        .with_context(|| format!("No attachment {index}"))?;
    Ok((name_of(p, index), p.contents().to_vec()))
}

pub fn delete(dir: &Path, id: &str) -> Result<()> {
    let f = file(dir, id)?;
    let _one = crate::lock(&WRITE);
    let mut all = read_index(dir);
    all.retain(|s| s.id != id);
    write_index(dir, &all)?;
    match std::fs::remove_file(f) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

pub fn clear(dir: &Path) -> Result<()> {
    let _one = crate::lock(&WRITE);
    match std::fs::remove_dir_all(dir) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

/// The SMTP listener; stops (and frees its port) when dropped.
pub struct Listener {
    pub port: u16,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        // Wakes `accept`; the thread then returns and closes the socket.
        let _ = std::net::TcpStream::connect(("127.0.0.1", self.port));
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Listen on 127.0.0.1:`port` (0: any free port), storing under `root`; `apps` are the
/// usernames with an inbox of their own.
pub fn listen(port: u16, root: PathBuf, apps: BTreeSet<String>) -> Result<Listener> {
    let sock = std::net::TcpListener::bind(("127.0.0.1", port))
        .with_context(|| format!("The mail catcher can't use port {port}"))?;
    let port = sock.local_addr()?.port();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = stop.clone();
    let apps = std::sync::Arc::new(apps);
    let thread = std::thread::spawn(move || {
        for conn in sock.incoming() {
            if flag.load(std::sync::atomic::Ordering::SeqCst) {
                return;
            }
            let Ok(conn) = conn else { continue };
            let (root, apps) = (root.clone(), apps.clone());
            std::thread::spawn(move || {
                let _ = session(conn, &root, &apps);
            });
        }
    });
    Ok(Listener {
        port,
        stop,
        thread: Some(thread),
    })
}

/// A line of at most `max` bytes; `None` at the end. A longer one comes back cut, without
/// its line break, and the rest of it is skipped.
fn line(r: &mut impl std::io::BufRead, max: usize) -> std::io::Result<Option<Vec<u8>>> {
    use std::io::{BufRead, Read};
    let mut buf = vec![];
    if r.by_ref().take(max as u64).read_until(b'\n', &mut buf)? == 0 {
        return Ok(None);
    }
    while !buf.ends_with(b"\n") {
        let chunk = r.fill_buf()?;
        if chunk.is_empty() {
            break;
        }
        let (n, end) = chunk
            .iter()
            .position(|&b| b == b'\n')
            .map_or((chunk.len(), false), |i| (i + 1, true));
        r.consume(n);
        if end {
            break;
        }
    }
    Ok(Some(buf))
}

fn decode(s: &str) -> String {
    mail_parser::decoders::base64::base64_decode(s.trim().as_bytes())
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default()
}

/// The address in `FROM:<a@b> SIZE=1` / `TO:<a@b>`.
fn path_of(arg: &str) -> String {
    let a = arg.split_once(':').map_or(arg, |(_, a)| a).trim();
    let a = a.split_whitespace().next().unwrap_or_default();
    a.trim_start_matches('<').trim_end_matches('>').to_string()
}

fn say(w: &mut impl std::io::Write, s: &str) -> std::io::Result<()> {
    w.write_all(format!("{s}\r\n").as_bytes())
}

/// One SMTP conversation: any login is accepted, what DATA brings is stored, nothing relayed.
fn session(conn: std::net::TcpStream, root: &Path, apps: &BTreeSet<String>) -> Result<()> {
    conn.set_read_timeout(Some(std::time::Duration::from_secs(300)))?;
    let mut r = std::io::BufReader::new(conn.try_clone()?);
    let mut w = conn;
    // A prompt and the client's answer to it.
    let ask = |w: &mut std::net::TcpStream, r: &mut std::io::BufReader<_>, prompt: &str| {
        say(w, prompt)?;
        let l = line(r, 4096)?.unwrap_or_default();
        anyhow::Ok(String::from_utf8_lossy(&l).trim().to_string())
    };
    say(&mut w, "220 anvil ESMTP Anvil mail catcher")?;
    let (mut user, mut from, mut rcpts) = (None::<String>, None::<String>, Vec::<String>::new());
    while let Some(l) = line(&mut r, 4096)? {
        let l = String::from_utf8_lossy(&l);
        let l = l.trim_end_matches(['\r', '\n']);
        let (verb, arg) = l.split_once(' ').unwrap_or((l, ""));
        let arg = arg.trim();
        match verb.to_ascii_uppercase().as_str() {
            "EHLO" => say(
                &mut w,
                &format!(
                    "250-anvil\r\n250-AUTH PLAIN LOGIN\r\n250-SIZE {MAX_SIZE}\r\n250-8BITMIME\r\n250-SMTPUTF8\r\n250 PIPELINING"
                ),
            )?,
            "HELO" => say(&mut w, "250 anvil")?,
            "AUTH" => {
                let (mech, first) = arg.split_once(' ').unwrap_or((arg, ""));
                let given = |w: &mut _, r: &mut _, prompt| match first {
                    "" => ask(w, r, prompt),
                    f => Ok(f.to_string()),
                };
                user = match mech.to_ascii_uppercase().as_str() {
                    // authzid \0 authcid \0 password
                    "PLAIN" => decode(&given(&mut w, &mut r, "334 ")?)
                        .split('\0')
                        .nth(1)
                        .map(String::from),
                    "LOGIN" => {
                        let u = decode(&given(&mut w, &mut r, "334 VXNlcm5hbWU6")?);
                        ask(&mut w, &mut r, "334 UGFzc3dvcmQ6")?;
                        Some(u)
                    }
                    _ => {
                        say(&mut w, "504 5.5.4 Use PLAIN or LOGIN")?;
                        continue;
                    }
                };
                say(&mut w, "235 2.7.0 Authentication successful")?;
            }
            "MAIL" => {
                let size = arg.split_whitespace().find_map(|p| {
                    p.to_ascii_uppercase()
                        .strip_prefix("SIZE=")?
                        .parse::<usize>()
                        .ok()
                });
                if size.is_some_and(|s| s > MAX_SIZE) {
                    say(&mut w, "552 5.3.4 Message too big")?;
                    continue;
                }
                from = Some(path_of(arg));
                rcpts.clear();
                say(&mut w, "250 2.1.0 Ok")?;
            }
            "RCPT" if from.is_none() => say(&mut w, "503 5.5.1 MAIL first")?,
            "RCPT" => {
                rcpts.push(path_of(arg));
                say(&mut w, "250 2.1.5 Ok")?;
            }
            "DATA" if rcpts.is_empty() => say(&mut w, "503 5.5.1 RCPT first")?,
            "DATA" => {
                say(&mut w, "354 End data with <CR><LF>.<CR><LF>")?;
                let mut body = vec![];
                let mut too_big = false;
                loop {
                    let Some(l) = line(&mut r, 1 << 20)? else {
                        return Ok(());
                    };
                    if l == b".\r\n" || l == b".\n" {
                        break;
                    }
                    let l = l.strip_prefix(b".").unwrap_or(&l);
                    // A line over 1 MiB (SMTP allows 1000 bytes) counts as too big as well.
                    too_big |= body.len() + l.len() > MAX_SIZE || !l.ends_with(b"\n");
                    if !too_big {
                        body.extend_from_slice(l);
                    }
                }
                let envelope = from.take().unwrap_or_default();
                let to = std::mem::take(&mut rcpts);
                if too_big {
                    say(&mut w, "552 5.3.4 Message too big")?;
                    continue;
                }
                match store(
                    &inbox_for(root, user.as_deref(), apps),
                    &body,
                    &envelope,
                    &to,
                ) {
                    Ok(s) => say(&mut w, &format!("250 2.0.0 Ok: queued as {}", s.id))?,
                    Err(e) => say(&mut w, &format!("451 4.3.0 {e}"))?,
                }
            }
            "RSET" => {
                (from, rcpts) = (None, vec![]);
                say(&mut w, "250 2.0.0 Ok")?;
            }
            "NOOP" => say(&mut w, "250 2.0.0 Ok")?,
            "VRFY" => say(&mut w, "252 2.5.0 Send some mail")?,
            "STARTTLS" => say(&mut w, "454 4.7.0 No TLS here: MAIL_ENCRYPTION=null")?,
            "QUIT" => {
                say(&mut w, "221 2.0.0 Bye")?;
                return Ok(());
            }
            _ => say(&mut w, "500 5.5.2 Unknown command")?,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpStream;

    struct Client {
        r: BufReader<TcpStream>,
        w: TcpStream,
    }

    impl Client {
        fn new(port: u16) -> Client {
            let w = TcpStream::connect(("127.0.0.1", port)).unwrap();
            let mut c = Client {
                r: BufReader::new(w.try_clone().unwrap()),
                w,
            };
            assert!(c.reply().starts_with("220"));
            c
        }
        /// The whole reply, continuation lines too.
        fn reply(&mut self) -> String {
            let mut all = String::new();
            loop {
                let mut l = String::new();
                self.r.read_line(&mut l).unwrap();
                all += &l;
                if l.len() < 4 || l.as_bytes()[3] != b'-' {
                    return all;
                }
            }
        }
        fn send(&mut self, line: &str) -> String {
            write!(self.w, "{line}\r\n").unwrap();
            self.reply()
        }
    }

    fn b64(s: &[u8]) -> String {
        super::b64(s)
    }

    fn server() -> (tempfile::TempDir, Listener) {
        let dir = tempfile::tempdir().unwrap();
        let l = listen(0, dir.path().into(), BTreeSet::from(["shop".to_string()])).unwrap();
        (dir, l)
    }

    fn send_mail(c: &mut Client, body: &str) -> String {
        assert!(c.send("MAIL FROM:<app@example.com>").starts_with("250"));
        assert!(c.send("RCPT TO:<user@example.com>").starts_with("250"));
        assert!(c.send("DATA").starts_with("354"));
        write!(c.w, "{body}\r\n").unwrap();
        c.send(".")
    }

    const PLAIN: &str = "From: Shop <hello@example.com>\r\nTo: user@example.com\r\nSubject: Welcome\r\n\r\nHi there\r\n..dotted\r\n";

    #[test]
    fn a_mail_sent_with_the_apps_name_lands_in_its_inbox() {
        let (dir, l) = server();
        let mut c = Client::new(l.port);
        let ehlo = c.send("EHLO laptop");
        assert!(ehlo.contains("AUTH PLAIN LOGIN"), "{ehlo}");
        assert!(
            c.send(&format!("AUTH PLAIN {}", b64(b"\0shop\0anvil")))
                .starts_with("235")
        );
        assert!(send_mail(&mut c, PLAIN).starts_with("250"));
        assert!(c.send("QUIT").starts_with("221"));
        let inbox = dir.path().join("shop");
        let got = list(&inbox);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].subject, "Welcome");
        assert_eq!(got[0].from, "Shop <hello@example.com>");
        assert_eq!(got[0].to, ["user@example.com"]);
        let m = read(&inbox, &got[0].id).unwrap();
        // Dot-stuffing undone.
        assert_eq!(
            m.text.as_deref().map(str::trim_end),
            Some("Hi there\r\n.dotted")
        );
        assert!(raw(&inbox, &got[0].id).unwrap().starts_with(b"From: Shop"));
    }

    #[test]
    fn auth_login_works_and_an_unknown_or_missing_user_goes_to_other() {
        let (dir, l) = server();
        let mut c = Client::new(l.port);
        c.send("EHLO x");
        assert!(c.send("AUTH LOGIN").starts_with("334"));
        assert!(c.send(&b64(b"shop")).starts_with("334"));
        assert!(c.send(&b64(b"secret")).starts_with("235"));
        send_mail(&mut c, PLAIN);
        // RSET keeps the login; a new session without one is Other's.
        c.send("RSET");
        send_mail(&mut c, PLAIN);
        assert_eq!(list(&dir.path().join("shop")).len(), 2);
        let mut c = Client::new(l.port);
        c.send("HELO x");
        send_mail(&mut c, PLAIN);
        let mut c = Client::new(l.port);
        c.send("EHLO x");
        c.send(&format!("AUTH PLAIN {}", b64(b"\0../etc\0x")));
        send_mail(&mut c, PLAIN);
        assert_eq!(list(&dir.path().join(OTHER)).len(), 2);
    }

    #[test]
    fn commands_out_of_order_and_too_big_mail_are_refused() {
        let (dir, l) = server();
        let mut c = Client::new(l.port);
        c.send("EHLO x");
        assert!(c.send("RCPT TO:<a@b>").starts_with("503"));
        assert!(c.send("DATA").starts_with("503"));
        assert!(c.send("MAIL FROM:<a@b> SIZE=999999999").starts_with("552"));
        assert!(c.send("NOOP").starts_with("250"));
        assert!(c.send("BOGUS").starts_with("500"));
        let lines = format!("{}\r\n", "x".repeat(998)).repeat(MAX_SIZE / 1000 + 1);
        assert!(send_mail(&mut c, &format!("Subject: big\r\n\r\n{lines}")).starts_with("552"));
        // One line over 1 MiB too.
        let long = format!("Subject: long\r\n\r\n{}", "x".repeat(2 << 20));
        assert!(send_mail(&mut c, &long).starts_with("552"));
        // The session goes on after it.
        assert!(send_mail(&mut c, PLAIN).starts_with("250"));
        assert_eq!(list(&dir.path().join(OTHER)).len(), 1);
    }

    #[test]
    fn dropping_the_listener_frees_its_port() {
        let (_d, l) = server();
        let port = l.port;
        drop(l);
        let again = listen(port, std::env::temp_dir(), BTreeSet::new());
        assert!(again.is_ok());
        let taken = listen(port, std::env::temp_dir(), BTreeSet::new());
        let e = taken.err().unwrap().to_string();
        assert!(e.contains(&port.to_string()), "{e}");
    }

    /// HTML with an inline image, a PDF attachment and an ISO-8859-9 encoded-word subject.
    const MIME: &str = "From: =?ISO-8859-9?Q?D=FCkkan?= <shop@example.com>\r\n\
To: a@example.com, B <b@example.com>\r\n\
Cc: c@example.com\r\n\
Subject: =?ISO-8859-9?Q?=DEifre_s=FDf=FDrlama?=\r\n\
Date: Tue, 20 Oct 2026 10:00:00 +0300\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/mixed; boundary=\"outer\"\r\n\
\r\n\
--outer\r\n\
Content-Type: multipart/related; boundary=\"rel\"\r\n\
\r\n\
--rel\r\n\
Content-Type: multipart/alternative; boundary=\"alt\"\r\n\
\r\n\
--alt\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
Content-Transfer-Encoding: quoted-printable\r\n\
\r\n\
Reset: https://shop.test/reset?t=1&u=2\r\n\
--alt\r\n\
Content-Type: text/html; charset=utf-8\r\n\
\r\n\
<p><img src=\"cid:logo@x\"><a href=\"https://shop.test/reset?t=1&amp;u=2\">Reset</a><img src=\"https://t.example/p.gif\"><a href='http://shop.test/a'>A</a></p>\r\n\
--alt--\r\n\
--rel\r\n\
Content-Type: image/png\r\n\
Content-ID: <logo@x>\r\n\
Content-Disposition: inline\r\n\
Content-Transfer-Encoding: base64\r\n\
\r\n\
iVBORw0KGgo=\r\n\
--rel--\r\n\
--outer\r\n\
Content-Type: application/pdf; name=\"invoice.pdf\"\r\n\
Content-Disposition: attachment; filename=\"invoice.pdf\"\r\n\
Content-Transfer-Encoding: base64\r\n\
\r\n\
JVBERi0xLjQ=\r\n\
--outer--\r\n";

    #[test]
    fn mime_parts_charsets_and_attachments_are_read() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path(), MIME.as_bytes(), "x@y", &["z@y".into()]).unwrap();
        assert_eq!(s.subject, "Şifre sıfırlama");
        assert_eq!(s.from, "Dükkan <shop@example.com>");
        assert_eq!(s.to, ["a@example.com", "B <b@example.com>"]);
        assert_eq!(s.attachments, 1, "the inline image isn't counted");
        let m = read(dir.path(), &s.id).unwrap();
        assert_eq!(m.cc, ["c@example.com"]);
        assert_eq!(m.date.as_deref(), Some("Tue, 20 Oct 2026 10:00:00 +0300"));
        let html = m.html.unwrap();
        assert!(
            html.contains("src=\"data:image/png;base64,iVBORw0KGgo=\""),
            "{html}"
        );
        assert!(m.text.unwrap().contains("Reset: https://shop.test/reset"));
        // Links the reader can follow: not an image (a tracking pixel), not the text's.
        assert_eq!(
            m.links,
            ["https://shop.test/reset?t=1&u=2", "http://shop.test/a"]
        );
        assert!(m.headers.iter().any(|(k, _)| k == "Content-Type"));
        let pdf = m.attachments.iter().find(|a| !a.inline).unwrap();
        assert_eq!(
            (pdf.name.as_str(), pdf.mime.as_str()),
            ("invoice.pdf", "application/pdf")
        );
        let (name, bytes) = attachment(dir.path(), &s.id, pdf.index).unwrap();
        assert_eq!(name, "invoice.pdf");
        assert_eq!(bytes, b"%PDF-1.4");
    }

    #[test]
    fn a_mail_without_headers_falls_back_to_the_envelope() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path(), b"just text\r\n", "x@y", &["z@y".into()]).unwrap();
        assert_eq!(
            (s.from.as_str(), s.to.clone()),
            ("x@y", vec!["z@y".to_string()])
        );
        assert_eq!(s.subject, "");
    }

    #[test]
    fn inboxes_keep_the_newest_and_delete_and_clear() {
        let dir = tempfile::tempdir().unwrap();
        let ids: Vec<String> = (0..5)
            .map(|i| {
                let m = format!("Subject: m{i}\r\n\r\nx\r\n");
                store_capped(dir.path(), m.as_bytes(), "a", &[], 3, u64::MAX)
                    .unwrap()
                    .id
            })
            .collect();
        let subjects = |d: &Path| list(d).into_iter().map(|s| s.subject).collect::<Vec<_>>();
        assert_eq!(subjects(dir.path()), ["m4", "m3", "m2"]);
        assert!(raw(dir.path(), &ids[0]).is_err(), "dropped from disk too");
        delete(dir.path(), &ids[3]).unwrap();
        assert_eq!(subjects(dir.path()), ["m4", "m2"]);
        assert!(raw(dir.path(), &ids[3]).is_err());
        // By size: each message is 18 bytes.
        store_capped(dir.path(), b"Subject: m5\r\n\r\nx\r\n", "a", &[], 100, 40).unwrap();
        assert_eq!(subjects(dir.path()), ["m5", "m4"]);
        clear(dir.path()).unwrap();
        assert!(list(dir.path()).is_empty());
    }

    #[test]
    fn ids_and_inbox_names_cant_leave_the_mail_folder() {
        let dir = tempfile::tempdir().unwrap();
        for id in ["../index", "", "a/b", "..", "1-2.eml"] {
            assert!(raw(dir.path(), id).is_err(), "{id}");
            assert!(delete(dir.path(), id).is_err(), "{id}");
        }
        let root = Path::new("/r");
        assert_eq!(inbox(root, "shop"), Some(root.join("shop")));
        assert_eq!(inbox(root, OTHER), Some(root.join(OTHER)));
        for bad in ["", "..", "../x", "a/b", "Shop", "a_b"] {
            assert_eq!(inbox(root, bad), None, "{bad}");
        }
    }

    #[test]
    fn apps_get_the_smtp_env_while_the_catcher_is_on() {
        let site = crate::tests::site("shop", "/s/shop/public", Some("8.4"));
        let mut cfg = crate::Config::default();
        assert!(env(&cfg, &site).is_empty());
        cfg.mail_catcher = true;
        let e = env(&cfg, &site);
        let get = |k: &str| e.iter().find(|(x, _)| x == k).map(|(_, v)| v.as_str());
        assert_eq!(get("MAIL_MAILER"), Some("smtp"));
        assert_eq!(get("MAIL_HOST"), Some("127.0.0.1"));
        assert_eq!(get("MAIL_PORT"), Some("2525"));
        assert_eq!(get("MAIL_USERNAME"), Some("shop"));
        assert_eq!(get("MAIL_ENCRYPTION"), Some("null"));
        assert_eq!(get("MAILER_DSN"), Some("smtp://shop:anvil@127.0.0.1:2525"));
        assert!(e.iter().all(|(k, v)| crate::worktrees::fpm_safe(k, v)));
        cfg.mail_port = Some(2600);
        assert_eq!(port(&cfg), 2600);
        cfg.sites.entry("shop".into()).or_default().mail_uncaught = true;
        assert!(env(&cfg, &site).is_empty());
    }
}
