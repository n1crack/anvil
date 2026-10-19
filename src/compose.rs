//! Docker Compose projects: the services of a folder's compose file and their
//! containers. Anvil only ever runs `up -d`, `stop` and `restart` on one service
//! or `up -d` and `stop` on the whole project; it never kills a container, so quitting Anvil leaves them running.

use anyhow::{Result, bail};
use serde::Serialize;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Serialize)]
pub struct Container {
    pub service: String,
    pub state: String,
    pub health: Option<String>,
    pub ports: Vec<u16>,
    /// How an exited container ended; None while it runs or when it never ran.
    pub exit_code: Option<i64>,
}

pub fn file(dir: &Path) -> Option<PathBuf> {
    [
        "compose.yaml",
        "compose.yml",
        "docker-compose.yaml",
        "docker-compose.yml",
    ]
    .iter()
    .map(|f| dir.join(f))
    .find(|p| p.exists())
}

/// The docker binary: GUI apps don't get the shell's PATH.
pub(crate) fn docker_bin() -> PathBuf {
    ["/usr/local/bin/docker", "/opt/homebrew/bin/docker"]
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.exists())
        .unwrap_or_else(|| "docker".into())
}

/// Run `docker compose <args>` in `dir`; stdout on success, a readable error otherwise.
fn compose(docker: &Path, dir: &Path, args: &[&str], timeout: Duration) -> Result<String> {
    run(docker, dir, &[&["compose"], args].concat(), timeout)
}

/// Run `docker <args>` in `dir`; stdout on success, a readable error otherwise.
fn run(docker: &Path, dir: &Path, args: &[&str], timeout: Duration) -> Result<String> {
    let mut child = match Command::new(docker)
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => bail!("Docker isn't installed"),
        Err(e) => bail!("Couldn't run docker: {e}"),
    };
    // Read on threads so a chatty command can't fill the pipe and stall.
    let reader = |r: Box<dyn Read + Send>| std::thread::spawn(move || crate::read_lossy(r));
    let out = reader(Box::new(child.stdout.take().unwrap()));
    let err = reader(Box::new(child.stderr.take().unwrap()));
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(st) = child.try_wait()? {
            break st;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!("Docker didn't answer in time");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let (out, err) = (
        out.join().unwrap_or_default(),
        err.join().unwrap_or_default(),
    );
    if status.success() {
        return Ok(out);
    }
    if err.contains("Cannot connect to the Docker daemon") || err.contains("docker.sock") {
        bail!("Docker isn't running");
    }
    bail!("{}", readable(&err, dir))
}

/// docker's stderr without its warnings (`level=warning` lines: unset variables, the
/// obsolete `version`), so the error that stopped it shows. When variables were unset,
/// a hint after it: which ones (3 at most), and the `.env.*.example` to copy when one
/// defines them and its env file is missing.
fn readable(err: &str, dir: &Path) -> String {
    let mut unset: Vec<String> = vec![];
    let mut kept = vec![];
    for l in err.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if !l.contains("level=warning") && !l.contains("level=warn ") {
            kept.push(l);
            continue;
        }
        let l = l.replace('\\', "");
        if let Some(name) = l
            .split_once("The \"")
            .and_then(|(_, r)| r.split_once("\" variable is not set"))
            .map(|(n, _)| n.to_string())
            && !unset.contains(&name)
        {
            unset.push(name);
        }
    }
    let mut msg = if kept.is_empty() {
        "docker compose failed".to_string()
    } else {
        kept.join("\n")
    };
    if unset.is_empty() {
        return msg;
    }
    let compose = file(dir)
        .and_then(|f| Some(f.file_name()?.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "The compose file".into());
    let more = if unset.len() > 3 { "…" } else { "" };
    msg += &format!(
        "\n{compose} needs variables that aren't set ({}{more}).",
        unset[..unset.len().min(3)].join(", ")
    );
    // The example whose env file is missing that defines the most of them.
    let defines = |text: &str| {
        unset
            .iter()
            .filter(|n| {
                text.lines()
                    .any(|l| l.trim_start().starts_with(&format!("{n}=")))
            })
            .count()
    };
    let example = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let target = name.strip_prefix(".env")?.strip_suffix(".example")?;
            let target = format!(".env{target}");
            if dir.join(&target).exists() {
                return None;
            }
            let n = defines(&std::fs::read_to_string(e.path()).ok()?);
            (n > 0).then_some((n, name, target))
        })
        .max_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
    if let Some((_, name, target)) = example {
        msg += &format!(" Copy {name} to {target}?");
    }
    msg
}

type Row = (String, String, Option<String>, Vec<u16>, Option<i64>);

/// Read `docker compose ps --format json`: one object per line (v2+) or a JSON array (older).
pub(crate) fn parse_ps(json_lines: &str) -> Vec<Row> {
    let mut values: Vec<serde_json::Value> = Vec::new();
    for v in serde_json::Deserializer::from_str(json_lines)
        .into_iter::<serde_json::Value>()
        .map_while(|v| v.ok())
    {
        match v {
            serde_json::Value::Array(a) => values.extend(a),
            v => values.push(v),
        }
    }
    values
        .iter()
        .filter_map(|v| {
            let text = |k: &str| v.get(k)?.as_str().map(str::to_string);
            let mut ports: Vec<u16> = v["Publishers"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|p| p["PublishedPort"].as_u64()?.try_into().ok())
                .filter(|p| *p != 0)
                .collect();
            // IPv4 and IPv6 publish the same port twice.
            ports.sort_unstable();
            ports.dedup();
            Some((
                text("Service")?,
                text("State").unwrap_or_default(),
                text("Health").filter(|h| !h.is_empty()),
                ports,
                v["ExitCode"]
                    .as_i64()
                    .filter(|_| text("State").as_deref() == Some("exited")),
            ))
        })
        .collect()
}

const READ: Duration = Duration::from_secs(5);

fn containers_with(docker: &Path, dir: &Path) -> Result<Vec<Container>> {
    let services = compose(docker, dir, &["config", "--services"], READ)?;
    let ps = compose(docker, dir, &["ps", "--all", "--format", "json"], READ)?;
    let ps = parse_ps(&ps);
    // By name: `config --services` gives dependency order, which changes as services do.
    let mut services: Vec<&str> = services
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    services.sort_unstable();
    services.dedup();
    Ok(services
        .into_iter()
        .map(|name| match ps.iter().find(|r| r.0 == name) {
            Some((_, state, health, ports, exit_code)) => Container {
                service: name.into(),
                state: state.clone(),
                health: health.clone(),
                ports: ports.clone(),
                exit_code: *exit_code,
            },
            None => Container {
                service: name.into(),
                state: "stopped".into(),
                health: None,
                ports: vec![],
                exit_code: None,
            },
        })
        .collect())
}

pub fn containers(dir: &Path) -> Result<Vec<Container>> {
    containers_with(&docker_bin(), dir)
}

/// Each line of `docker ps` as one JSON object, with the compose labels.
const PS_FORMAT: &str = r#"{"dir":{{json (.Label "com.docker.compose.project.working_dir")}},"service":{{json (.Label "com.docker.compose.service")}},"state":{{json .State}},"status":{{json .Status}},"ports":{{json .Ports}}}"#;

/// Read `docker ps --format PS_FORMAT`: the project folder and the container of each line.
pub(crate) fn parse_running(lines: &str) -> Vec<(PathBuf, Container)> {
    #[derive(serde::Deserialize)]
    struct Line {
        dir: String,
        service: String,
        state: String,
        status: String,
        ports: String,
    }
    let mut rows = lines
        .lines()
        .filter_map(|l| serde_json::from_str::<Line>(l).ok())
        .filter(|l| !l.dir.is_empty() && !l.service.is_empty())
        .map(|l| {
            // "0.0.0.0:5432->5432/tcp, [::]:5432->5432/tcp": the published port is before "->".
            let mut ports: Vec<u16> = l
                .ports
                .split(", ")
                .filter_map(|p| p.split_once("->")?.0.rsplit(':').next()?.parse().ok())
                .collect();
            ports.sort_unstable();
            ports.dedup();
            // "Up 3 days (healthy)", "Up 5 seconds (health: starting)".
            let health = ["unhealthy", "healthy", "starting"]
                .into_iter()
                .find(|h| l.status.ends_with(&format!("{h})")))
                .map(str::to_string);
            let c = Container {
                service: l.service,
                state: l.state,
                health,
                ports,
                exit_code: None,
            };
            (PathBuf::from(l.dir), c)
        })
        .collect::<Vec<_>>();
    // `docker ps` lists by creation, so a restart moves a container: by folder and name.
    rows.sort_by(|a, b| (&a.0, &a.1.service).cmp(&(&b.0, &b.1.service)));
    rows
}

fn running_with(docker: &Path) -> Result<Vec<(PathBuf, Container)>> {
    let args = [
        "ps",
        "--filter",
        "label=com.docker.compose.service",
        "--format",
        PS_FORMAT,
    ];
    Ok(parse_running(&run(docker, Path::new("/"), &args, READ)?))
}

/// Every running compose container, by the folder its project was started from:
/// one `docker ps` for all projects.
pub fn running() -> Result<Vec<(PathBuf, Container)>> {
    running_with(&docker_bin())
}

fn action_with(docker: &Path, dir: &Path, service: &str, action: &str) -> Result<()> {
    let args: &[&str] = match action {
        "start" => &["up", "-d", "--", service],
        "stop" => &["stop", "--", service],
        "restart" => &["restart", "--", service],
        _ => bail!("Unknown action: {action}"),
    };
    // Starting may pull an image or wait for a slow shutdown (a game server saving its world).
    compose(docker, dir, args, Duration::from_secs(600)).map(drop)
}

pub fn action(dir: &Path, service: &str, action: &str) -> Result<()> {
    action_with(&docker_bin(), dir, service, action)
}

/// Start or stop the whole project with one command, so compose keeps the
/// dependency order (a database stops after the server that saves to it).
fn project_action_with(docker: &Path, dir: &Path, action: &str) -> Result<()> {
    let args: &[&str] = match action {
        "start" => &["up", "-d"],
        "stop" => &["stop"],
        _ => bail!("Unknown action: {action}"),
    };
    compose(docker, dir, args, Duration::from_secs(600)).map(drop)
}

pub fn project_action(dir: &Path, action: &str) -> Result<()> {
    project_action_with(&docker_bin(), dir, action)
}

/// Lines kept per followed service.
const KEEP: usize = 2000;
/// What a first read gets, as many as the stream starts with (`--tail`).
const FIRST: u64 = 500;
/// A stream that ended (Docker restarted, the CLI failed) is started again after this.
const REFOLLOW: Duration = Duration::from_secs(5);

/// docker's fixed-width RFC 3339 time at the start of a `--timestamps` line, which
/// sorts as text (`2026-10-17T10:00:01.112345678Z`).
fn stamp(line: &str) -> Option<&str> {
    let t = line.split(' ').next()?;
    let b = t.as_bytes();
    (b.len() >= 20 && b[4] == b'-' && b[10] == b'T' && t.ends_with('Z')).then_some(t)
}

/// One service's newest log lines as `docker compose logs -f` writes them; line `n` is
/// the `n`th since the buffer began (1-based), so a reader asks for what came after
/// the last number it saw.
#[derive(Default)]
pub struct LogBuffer {
    lines: std::collections::VecDeque<String>,
    /// How many lines were ever pushed: the number of the newest.
    end: u64,
    /// A new follow repeats the old one's tail: its lines up to this time are skipped.
    replayed: Option<String>,
}

impl LogBuffer {
    fn push(&mut self, line: String) {
        if let Some(until) = &self.replayed {
            match stamp(&line) {
                Some(t) if t > until.as_str() => self.replayed = None,
                // Seen already, or a line continuing one seen.
                _ => return,
            }
        }
        if self.lines.len() == KEEP {
            self.lines.pop_front();
        }
        self.lines.push_back(line);
        self.end += 1;
    }

    /// The lines after line `after` (0, or a number this buffer never reached: the
    /// newest `FIRST`), joined, and the number to ask after next time.
    pub fn after(&self, after: u64) -> (String, u64) {
        let first = self.end - self.lines.len() as u64;
        let from = if after == 0 || after > self.end {
            self.end.saturating_sub(FIRST)
        } else {
            after
        }
        .max(first);
        let mut text = String::new();
        for l in self.lines.range((from - first) as usize..) {
            text.push_str(l);
            text.push('\n');
        }
        (text, self.end)
    }
}

type Shared = std::sync::Arc<std::sync::Mutex<LogBuffer>>;

struct Follow {
    child: std::process::Child,
    buf: Shared,
    at: Instant,
}

/// `docker compose logs -f` for one service, its lines pushed into `buf` by a thread
/// that ends with the stream. Its own process group, so the compose plugin under the
/// CLI stops with it.
fn follow(docker: &Path, dir: &Path, service: &str, buf: Shared) -> Result<std::process::Child> {
    use std::io::BufRead;
    use std::os::unix::process::CommandExt;
    let mut child = Command::new(docker)
        .args(["compose", "logs", "-f", "--no-color", "--no-log-prefix"])
        .args(["--timestamps", "--tail", &FIRST.to_string(), "--", service])
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()?;
    let mut out = std::io::BufReader::new(child.stdout.take().unwrap());
    std::thread::spawn(move || {
        let mut line = Vec::new();
        while out.read_until(b'\n', &mut line).is_ok_and(|n| n > 0) {
            let text = String::from_utf8_lossy(&line);
            let text = text.strip_suffix('\n').unwrap_or(&text).to_string();
            crate::lock(&buf).push(text);
            line.clear();
        }
    });
    Ok(child)
}

/// The Containers tab's logs: one `docker compose logs -f` per service of the app it
/// shows, read from memory, so a poll spawns nothing (`--since` at every ask made the
/// daemon scan the whole log).
#[derive(Default)]
pub struct Logs {
    /// The app followed and its folder.
    site: String,
    dir: PathBuf,
    follows: std::collections::BTreeMap<String, Follow>,
}

impl Logs {
    /// `service`'s lines after line `after` (see `LogBuffer::after`) and the number to
    /// ask after next. Its stream starts on the first ask; another app's streams stop.
    /// `dir` finds the app's folder, asked only when the app changes.
    pub fn read(
        &mut self,
        site: &str,
        dir: impl FnOnce() -> Result<PathBuf>,
        service: &str,
        after: u64,
    ) -> Result<(String, u64)> {
        self.read_with(&docker_bin(), site, dir, service, after)
    }

    fn read_with(
        &mut self,
        docker: &Path,
        site: &str,
        dir: impl FnOnce() -> Result<PathBuf>,
        service: &str,
        after: u64,
    ) -> Result<(String, u64)> {
        if self.site != site || self.follows.is_empty() {
            self.stop();
            self.dir = dir()?;
            self.site = site.to_string();
        }
        let f = match self.follows.entry(service.to_string()) {
            std::collections::btree_map::Entry::Occupied(e) => e.into_mut(),
            std::collections::btree_map::Entry::Vacant(e) => {
                let buf = Shared::default();
                let child = follow(docker, &self.dir, service, buf.clone())?;
                e.insert(Follow {
                    child,
                    buf,
                    at: Instant::now(),
                })
            }
        };
        if !matches!(f.child.try_wait(), Ok(None)) && f.at.elapsed() >= REFOLLOW {
            {
                let mut b = crate::lock(&f.buf);
                b.replayed = b
                    .lines
                    .iter()
                    .rev()
                    .find_map(|l| stamp(l))
                    .map(String::from);
            }
            f.child = follow(docker, &self.dir, service, f.buf.clone())?;
            f.at = Instant::now();
        }
        Ok(crate::lock(&f.buf).after(after))
    }

    /// End every stream: their process groups are killed and reaped.
    pub fn stop(&mut self) {
        for (_, mut f) in std::mem::take(&mut self.follows) {
            crate::terminate(&mut f.child, Duration::ZERO);
        }
    }
}

impl Drop for Logs {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn logs(dir: &Path, service: &str) -> Result<String> {
    compose(
        &docker_bin(),
        dir,
        &["logs", "--no-color", "--tail", "200", "--", service],
        Duration::from_secs(10),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("anvil-compose-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn fake_docker(dir: &Path, body: &str) -> PathBuf {
        let p = dir.join("docker");
        fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    #[test]
    fn parses_ps_lines_and_arrays() {
        let lines = concat!(
            r#"{"Service":"sphere","State":"running","Health":"healthy","Publishers":[{"PublishedPort":2593},{"PublishedPort":2593}]}"#,
            "\n",
            r#"{"Service":"db","State":"exited","Health":"","Publishers":[],"ExitCode":137}"#,
            "\n"
        );
        let want = vec![
            (
                "sphere".into(),
                "running".into(),
                Some("healthy".into()),
                vec![2593],
                None,
            ),
            ("db".into(), "exited".into(), None, vec![], Some(137)),
        ];
        assert_eq!(parse_ps(lines), want);
        let array = format!("[{}]", lines.trim().replace('\n', ","));
        assert_eq!(parse_ps(&array), want);
        assert!(parse_ps("").is_empty());
    }

    #[test]
    fn reads_running_containers_by_project_folder() {
        let lines = concat!(
            r#"{"dir":"/Users/x/elpida","service":"sphere","state":"running","status":"Up 3 days (healthy)","ports":"0.0.0.0:2593->2593/tcp, [::]:2593->2593/tcp"}"#,
            "\n",
            r#"{"dir":"/Users/x/elpida","service":"nobetci","state":"running","status":"Up 3 days","ports":""}"#,
            "\n",
            r#"{"dir":"/Users/x/shop","service":"db","state":"running","status":"Up 5 seconds (health: starting)","ports":"127.0.0.1:3307->3306/tcp, 8080/tcp"}"#,
            "\n",
            // Not from compose: no labels.
            r#"{"dir":"","service":"","state":"running","status":"Up 1 hour","ports":""}"#,
            "\nnot json\n"
        );
        let got: Vec<_> = parse_running(lines)
            .into_iter()
            .map(|(d, c)| (d, c.service, c.state, c.health, c.ports))
            .collect();
        let s = |v: &str| v.to_string();
        assert_eq!(
            got,
            vec![
                // By folder and name, not docker's order.
                (
                    PathBuf::from("/Users/x/elpida"),
                    s("nobetci"),
                    s("running"),
                    None,
                    vec![]
                ),
                (
                    PathBuf::from("/Users/x/elpida"),
                    s("sphere"),
                    s("running"),
                    Some(s("healthy")),
                    vec![2593]
                ),
                (
                    PathBuf::from("/Users/x/shop"),
                    s("db"),
                    s("running"),
                    Some(s("starting")),
                    vec![3307]
                ),
            ]
        );
    }

    #[test]
    fn asks_docker_ps_once_with_the_compose_labels() {
        let bin = tmp("running-bin");
        let log = bin.join("args");
        let docker = fake_docker(
            &bin,
            &format!(
                "echo \"$@\" >> {}\necho '{{\"dir\":\"/s\",\"service\":\"web\",\"state\":\"running\",\"status\":\"Up\",\"ports\":\"\"}}'",
                log.display()
            ),
        );
        let got = running_with(&docker).unwrap();
        assert_eq!(
            (got[0].0.as_path(), got[0].1.service.as_str()),
            (Path::new("/s"), "web")
        );
        let args = fs::read_to_string(log).unwrap();
        assert!(
            args.starts_with("ps --filter label=com.docker.compose.service --format "),
            "{args}"
        );
        assert_eq!(args.lines().count(), 1);
    }

    #[test]
    fn finds_the_compose_file() {
        let d = tmp("file");
        assert_eq!(file(&d), None);
        fs::write(d.join("docker-compose.yml"), "").unwrap();
        assert_eq!(file(&d), Some(d.join("docker-compose.yml")));
    }

    #[test]
    fn refuses_actions_other_than_start_stop_restart() {
        let d = tmp("action");
        assert!(action(&d, "x", "down").is_err());
        assert!(action(&d, "x", "kill").is_err());
    }

    #[test]
    fn explains_a_stopped_daemon_and_a_missing_docker() {
        let d = tmp("daemon-dir");
        let bin = tmp("daemon-bin");
        let docker = fake_docker(
            &bin,
            "echo 'Cannot connect to the Docker daemon at unix:///x.sock' >&2; exit 1",
        );
        let e = containers_with(&docker, &d).err().unwrap();
        assert_eq!(e.to_string(), "Docker isn't running");
        let e = containers_with(&bin.join("nope"), &d).err().unwrap();
        assert_eq!(e.to_string(), "Docker isn't installed");
    }

    #[test]
    fn merges_services_with_containers() {
        let d = tmp("merge-dir");
        let bin = tmp("merge-bin");
        let docker = fake_docker(
            &bin,
            r#"case "$2" in
config) printf 'web\ndb\n';;
ps) echo '{"Service":"web","State":"running","Health":"","Publishers":[{"PublishedPort":8080}]}';;
esac"#,
        );
        let c = containers_with(&docker, &d).unwrap();
        // By name, not in the order compose gives them.
        assert_eq!(
            (c[0].service.as_str(), c[0].state.as_str()),
            ("db", "stopped")
        );
        assert_eq!(
            (c[1].service.as_str(), c[1].state.as_str(), &c[1].ports[..]),
            ("web", "running", &[8080][..])
        );
    }

    #[test]
    fn maps_actions_to_compose_commands() {
        let d = tmp("args-dir");
        let bin = tmp("args-bin");
        let log = d.join("args");
        let docker = fake_docker(&bin, &format!("echo \"$@\" >> {}", log.display()));
        for a in ["start", "stop", "restart"] {
            action_with(&docker, &d, "web", a).unwrap();
        }
        assert_eq!(
            fs::read_to_string(log).unwrap(),
            "compose up -d -- web\ncompose stop -- web\ncompose restart -- web\n"
        );
    }

    #[test]
    fn whole_project_actions_leave_the_order_to_compose() {
        let d = tmp("project-dir");
        let bin = tmp("project-bin");
        let log = d.join("args");
        let docker = fake_docker(&bin, &format!("echo \"$@\" >> {}", log.display()));
        project_action_with(&docker, &d, "start").unwrap();
        project_action_with(&docker, &d, "stop").unwrap();
        assert_eq!(
            fs::read_to_string(log).unwrap(),
            "compose up -d\ncompose stop\n"
        );
        for a in ["restart", "down", "kill"] {
            assert!(project_action_with(&docker, &d, a).is_err());
        }
    }

    #[test]
    fn a_log_buffer_numbers_lines_keeps_the_newest_and_skips_a_replay() {
        let mut b = LogBuffer::default();
        for i in 0..(KEEP as u64 + 10) {
            b.push(format!("2026-10-17T10:00:{:02}.{i:09}Z line {i}", i % 60));
        }
        let (all, next) = b.after(0);
        assert_eq!(next, KEEP as u64 + 10);
        assert_eq!(all.lines().count() as u64, FIRST);
        assert!(all.ends_with(&format!("line {}\n", KEEP + 9)));
        assert_eq!(b.after(next - 2).0.lines().count(), 2);
        assert_eq!(b.after(next), (String::new(), next));
        // Older than what is kept: from the oldest kept line.
        assert_eq!(b.after(3).0.lines().count(), KEEP);
        // A new follow's first lines repeat the tail: skipped up to the last time seen.
        let mut b = LogBuffer::default();
        b.push("2026-10-17T10:00:01.000000000Z a".into());
        b.push("2026-10-17T10:00:02.000000000Z b".into());
        b.replayed = Some("2026-10-17T10:00:02.000000000Z".into());
        for l in [
            "2026-10-17T10:00:01.000000000Z a",
            "  more of a",
            "2026-10-17T10:00:02.000000000Z b",
            "2026-10-17T10:00:03.000000000Z c",
            "  more of c",
        ] {
            b.push(l.into());
        }
        assert_eq!(
            b.after(2),
            ("2026-10-17T10:00:03.000000000Z c\n  more of c\n".into(), 4)
        );
    }

    #[test]
    fn follows_one_stream_per_service_and_stops_it() {
        let d = tmp("follow-dir");
        let bin = tmp("follow-bin");
        let args = d.join("args");
        let docker = fake_docker(
            &bin,
            &format!(
                "echo \"$@\" >> {}\nprintf '2026-10-17T10:00:01.000000000Z up \\377\\n'\nexec sleep 30",
                args.display()
            ),
        );
        let mut logs = Logs::default();
        let deadline = Instant::now() + Duration::from_secs(5);
        let (text, next) = loop {
            let r = logs
                .read_with(&docker, "shop", || Ok(d.clone()), "db", 0)
                .unwrap();
            if r.1 > 0 || Instant::now() > deadline {
                break r;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        // A byte that isn't UTF-8 doesn't blank the line.
        assert_eq!(
            (text.as_str(), next),
            ("2026-10-17T10:00:01.000000000Z up \u{fffd}\n", 1)
        );
        // Asked again: from memory, no second docker, no folder lookup.
        logs.read_with(&docker, "shop", || unreachable!(), "db", next)
            .unwrap();
        assert_eq!(
            fs::read_to_string(&args).unwrap(),
            "compose logs -f --no-color --no-log-prefix --timestamps --tail 500 -- db\n"
        );
        let pid = logs.follows["db"].child.id().to_string();
        logs.stop();
        assert!(logs.follows.is_empty());
        // Killed and reaped: the pid names no process, not even a zombie.
        let ps = Command::new("ps").args(["-p", &pid]).output().unwrap();
        assert!(!ps.status.success(), "{pid} is still there");
    }

    #[test]
    fn a_silent_failure_still_has_a_message() {
        let d = tmp("silent-dir");
        let bin = tmp("silent-bin");
        let docker = fake_docker(&bin, "exit 1");
        let e = action_with(&docker, &d, "web", "stop").unwrap_err();
        assert_eq!(e.to_string(), "docker compose failed");
    }

    // my-store's `docker compose config --services`, its .env.docker not made yet.
    const UNSET: &str = r#"time="2026-10-18T12:36:05+03:00" level=warning msg="The \"MEILISEARCH_UI_PORT\" variable is not set. Defaulting to a blank string."
time="2026-10-18T12:36:05+03:00" level=warning msg="The \"DB_PASSWORD\" variable is not set. Defaulting to a blank string."
time="2026-10-18T12:36:05+03:00" level=warning msg="The \"DB_USERNAME\" variable is not set. Defaulting to a blank string."
time="2026-10-18T12:36:05+03:00" level=warning msg="The \"DB_PASSWORD\" variable is not set. Defaulting to a blank string."
time="2026-10-18T12:36:05+03:00" level=warning msg="The \"ADMIN_FIRSTNAME\" variable is not set. Defaulting to a blank string."
time="2026-10-18T12:36:05+03:00" level=warning msg="The \"MEILISEARCH_PORT\" variable is not set. Defaulting to a blank string."
time="2026-10-18T12:36:05+03:00" level=warning msg="/Users/x/my-store/docker-compose.yml: the attribute `version` is obsolete, it will be ignored, please remove it to avoid potential confusion"
no port specified: :<empty>
"#;

    #[test]
    fn unset_variables_are_a_hint_not_a_wall_of_warnings() {
        let d = tmp("unset-dir");
        fs::write(d.join("docker-compose.yml"), "services: {}\n").unwrap();
        fs::write(d.join(".env"), "APP_NAME=shop\n").unwrap();
        fs::write(d.join(".env.example"), "APP_NAME=\n").unwrap();
        fs::write(d.join(".env.lando.example"), "DB_PASSWORD=\n").unwrap();
        fs::write(
            d.join(".env.docker.example"),
            "DB_USERNAME=\nDB_PASSWORD=\nMEILISEARCH_PORT=7700\n",
        )
        .unwrap();
        let bin = tmp("unset-bin");
        fs::write(bin.join("stderr"), UNSET).unwrap();
        let docker = fake_docker(
            &bin,
            &format!("cat '{}' >&2; exit 1", bin.join("stderr").display()),
        );
        let e = containers_with(&docker, &d).err().unwrap().to_string();
        assert_eq!(
            e,
            "no port specified: :<empty>\ndocker-compose.yml needs variables that aren't set \
             (MEILISEARCH_UI_PORT, DB_PASSWORD, DB_USERNAME…). Copy .env.docker.example to .env.docker?"
        );
        // Once .env.docker exists, the next example that defines one of them.
        fs::write(d.join(".env.docker"), "").unwrap();
        assert!(readable(UNSET, &d).ends_with(" Copy .env.lando.example to .env.lando?"));
        assert_eq!(readable("level=warning msg=\"x\"\nboom", &d), "boom");
    }
}
