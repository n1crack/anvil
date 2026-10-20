//! The control socket: the running app answers the CLI and agents on
//! `run/anvil.sock`, one JSON object per line each way.

use anyhow::{Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// macOS `sun_path` holds 104 bytes with the closing NUL.
const MAX_PATH: usize = 103;

#[derive(Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    pub method: String,
    #[serde(default)]
    pub params: Value,
    #[serde(default)]
    pub client: String,
}

pub type Handler = Arc<dyn Fn(&Request) -> Result<Value, String> + Send + Sync>;

pub fn socket_path() -> PathBuf {
    crate::home().join("run").join("anvil.sock")
}

fn checked_path() -> Result<PathBuf> {
    let p = socket_path();
    if p.as_os_str().len() > MAX_PATH {
        bail!(
            "the control socket path is over {MAX_PATH} bytes: {}",
            p.display()
        );
    }
    Ok(p)
}

/// Binds the socket, owner-only, replacing a file a crashed app left behind.
pub fn bind() -> Result<UnixListener> {
    use std::os::unix::fs::PermissionsExt;
    let p = checked_path()?;
    crate::state_dirs()?;
    if UnixStream::connect(&p).is_ok() {
        bail!("Anvil is already running ({} answers)", p.display());
    }
    let _ = std::fs::remove_file(&p);
    let l = UnixListener::bind(&p).map_err(|e| anyhow!("couldn't bind {}: {e}", p.display()))?;
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600))?;
    Ok(l)
}

/// Accepts on the caller's thread; each connection gets a thread and may send many requests.
pub fn serve(l: UnixListener, h: Handler) {
    for conn in l.incoming().flatten() {
        let h = h.clone();
        std::thread::spawn(move || answer(conn, &h));
    }
}

fn answer(conn: UnixStream, h: &Handler) {
    let Ok(mut out) = conn.try_clone() else {
        return;
    };
    for line in BufReader::new(conn).lines() {
        let Ok(line) = line else { return };
        let reply = match serde_json::from_str::<Request>(&line) {
            Ok(r) => match h(&r) {
                Ok(v) => json!({"id": r.id, "result": v}),
                Err(e) => json!({"id": r.id, "error": e}),
            },
            Err(e) => json!({"id": 0, "error": format!("bad request: {e}")}),
        };
        if writeln!(out, "{reply}").is_err() {
            return;
        }
    }
}

/// Asks the running app. `Ok(None)` when no app listens.
pub fn request(method: &str, params: Value, client: &str) -> Result<Option<Value>> {
    let p = checked_path()?;
    let mut conn = match UnixStream::connect(&p) {
        Ok(c) => c,
        Err(e) if matches!(e.kind(), ErrorKind::NotFound | ErrorKind::ConnectionRefused) => {
            return Ok(None);
        }
        Err(e) => bail!("couldn't reach Anvil at {}: {e}", p.display()),
    };
    conn.set_read_timeout(Some(Duration::from_secs(5)))?;
    conn.set_write_timeout(Some(Duration::from_secs(5)))?;
    let req = Request {
        id: 1,
        method: method.into(),
        params,
        client: client.into(),
    };
    writeln!(conn, "{}", serde_json::to_string(&req)?)?;
    let mut line = String::new();
    match BufReader::new(conn).read_line(&mut line) {
        Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
            bail!("Anvil didn't answer")
        }
        Err(e) => bail!("couldn't read Anvil's answer: {e}"),
        Ok(0) => bail!("Anvil closed the connection"),
        Ok(_) => {}
    }
    let mut reply: Value = serde_json::from_str(&line)?;
    if let Some(e) = reply.get("error") {
        bail!("{}", e.as_str().unwrap_or("Anvil sent an error"));
    }
    Ok(Some(reply["result"].take()))
}

/// Tells a running app the config changed; nothing when none runs.
pub fn notify_changed() {
    let _ = request("config_changed", json!({}), "Terminal");
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::{Duration, Instant};

    /// Holds the env lock with ANVIL_HOME at a fresh, short dir; removes it on drop.
    struct Home(
        PathBuf,
        #[allow(dead_code)] std::sync::MutexGuard<'static, ()>,
    );
    impl Drop for Home {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn home(pad: usize) -> Home {
        static N: AtomicU32 = AtomicU32::new(0);
        let guard = crate::lock(&crate::ENV_LOCK);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = PathBuf::from(format!(
            "/tmp/anvil-ctl-{}-{n:03}{}",
            std::process::id(),
            "x".repeat(pad)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // SAFETY: every test that sets ANVIL_HOME holds ENV_LOCK.
        unsafe { std::env::set_var("ANVIL_HOME", &dir) };
        Home(dir, guard)
    }
    fn serving(h: impl Fn(&Request) -> Result<Value, String> + Send + Sync + 'static) {
        let l = bind().unwrap();
        std::thread::spawn(move || serve(l, Arc::new(h)));
    }

    #[test]
    fn round_trip() {
        let _h = home(0);
        serving(|r| Ok(r.params.clone()));
        let got = request("echo", json!({"a":1}), "t").unwrap();
        assert_eq!(got, Some(json!({"a":1})));
    }

    #[test]
    fn error_reply_is_err() {
        let _h = home(0);
        serving(|_| Err("nope".into()));
        let e = request("x", json!({}), "t").unwrap_err();
        assert!(e.to_string().contains("nope"), "{e}");
    }

    #[test]
    fn no_app_is_none() {
        let _h = home(0);
        assert_eq!(request("ping", json!({}), "t").unwrap(), None);
    }

    #[test]
    fn stale_socket_is_replaced_and_reads_as_not_running() {
        let _h = home(0);
        crate::state_dirs().unwrap();
        drop(UnixListener::bind(socket_path()).unwrap());
        assert!(socket_path().exists());
        let t = Instant::now();
        assert_eq!(request("ping", json!({}), "t").unwrap(), None);
        assert!(t.elapsed() < Duration::from_millis(100));
        bind().unwrap();
    }

    #[test]
    fn socket_is_owner_only() {
        let h = home(0);
        let _l = bind().unwrap();
        let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&socket_path()), 0o600);
        assert_eq!(mode(&h.0.join("run")), 0o700);
    }

    #[test]
    fn long_socket_path_is_an_error() {
        let base = format!("/tmp/anvil-ctl-{}-000/run/anvil.sock", std::process::id()).len();
        let _h = home(110 - base);
        let path = socket_path();
        assert_eq!(path.as_os_str().len(), 110);
        let shown = path.display().to_string();
        assert!(bind().unwrap_err().to_string().contains(&shown));
        let e = request("ping", json!({}), "t").unwrap_err();
        assert!(e.to_string().contains(&shown), "{e}");
    }

    #[test]
    fn request_times_out() {
        let _h = home(0);
        serving(|_| {
            std::thread::sleep(Duration::from_secs(6));
            Ok(Value::Null)
        });
        let t = Instant::now();
        let e = request("slow", json!({}), "t").unwrap_err();
        assert!(e.to_string().contains("Anvil didn't answer"), "{e}");
        assert!(t.elapsed() < Duration::from_secs(6));
    }
}
