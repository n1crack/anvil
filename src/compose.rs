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
    let reader = |mut r: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut s = String::new();
            let _ = r.read_to_string(&mut s);
            s
        })
    };
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
    let err = err.trim();
    bail!(
        "{}",
        if err.is_empty() {
            "docker compose failed"
        } else {
            err
        }
    )
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
    Ok(services
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
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
    lines
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
        .collect()
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

/// A service's log lines, each after docker's RFC 3339 timestamp: the newest 500, or
/// those since `since` (a timestamp from an earlier answer). The caller drops the lines
/// it already has, as `--since` keeps the one at that very time.
fn logs_since_with(
    docker: &Path,
    dir: &Path,
    service: &str,
    since: Option<&str>,
) -> Result<String> {
    let mut args = vec!["logs", "--no-color", "--no-log-prefix", "--timestamps"];
    match since {
        Some(t)
            if !t.is_empty()
                && t.chars()
                    .all(|c| c.is_ascii_digit() || "-:.TZ+".contains(c)) =>
        {
            args.extend(["--since", t])
        }
        Some(t) => bail!("Not a timestamp: {t}"),
        None => args.extend(["--tail", "500"]),
    }
    args.extend(["--", service]);
    compose(docker, dir, &args, Duration::from_secs(10))
}

pub fn logs_since(dir: &Path, service: &str, since: Option<&str>) -> Result<String> {
    logs_since_with(&docker_bin(), dir, service, since)
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
                (
                    PathBuf::from("/Users/x/elpida"),
                    s("sphere"),
                    s("running"),
                    Some(s("healthy")),
                    vec![2593]
                ),
                (
                    PathBuf::from("/Users/x/elpida"),
                    s("nobetci"),
                    s("running"),
                    None,
                    vec![]
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
        assert_eq!(
            (c[0].service.as_str(), c[0].state.as_str(), &c[0].ports[..]),
            ("web", "running", &[8080][..])
        );
        assert_eq!(
            (c[1].service.as_str(), c[1].state.as_str()),
            ("db", "stopped")
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
    fn reads_logs_with_timestamps_and_refuses_other_since_values() {
        let d = tmp("logs-dir");
        let bin = tmp("logs-bin");
        let log = d.join("args");
        let docker = fake_docker(&bin, &format!("echo \"$@\" >> {}", log.display()));
        logs_since_with(&docker, &d, "db", None).unwrap();
        logs_since_with(&docker, &d, "db", Some("2026-10-17T10:00:01.5Z")).unwrap();
        assert!(logs_since_with(&docker, &d, "db", Some("--follow")).is_err());
        assert_eq!(
            fs::read_to_string(log).unwrap(),
            "compose logs --no-color --no-log-prefix --timestamps --tail 500 -- db\n\
             compose logs --no-color --no-log-prefix --timestamps --since 2026-10-17T10:00:01.5Z -- db\n"
        );
    }

    #[test]
    fn a_silent_failure_still_has_a_message() {
        let d = tmp("silent-dir");
        let bin = tmp("silent-bin");
        let docker = fake_docker(&bin, "exit 1");
        let e = action_with(&docker, &d, "web", "stop").unwrap_err();
        assert_eq!(e.to_string(), "docker compose failed");
    }
}
