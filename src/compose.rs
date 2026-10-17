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
    let mut child = match Command::new(docker)
        .arg("compose")
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

type Row = (String, String, Option<String>, Vec<u16>);

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
            Some((_, state, health, ports)) => Container {
                service: name.into(),
                state: state.clone(),
                health: health.clone(),
                ports: ports.clone(),
            },
            None => Container {
                service: name.into(),
                state: "stopped".into(),
                health: None,
                ports: vec![],
            },
        })
        .collect())
}

pub fn containers(dir: &Path) -> Result<Vec<Container>> {
    containers_with(&docker_bin(), dir)
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
            r#"{"Service":"db","State":"exited","Health":"","Publishers":[]}"#,
            "\n"
        );
        let want = vec![
            (
                "sphere".into(),
                "running".into(),
                Some("healthy".into()),
                vec![2593],
            ),
            ("db".into(), "exited".into(), None, vec![]),
        ];
        assert_eq!(parse_ps(lines), want);
        let array = format!("[{}]", lines.trim().replace('\n', ","));
        assert_eq!(parse_ps(&array), want);
        assert!(parse_ps("").is_empty());
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
    fn a_silent_failure_still_has_a_message() {
        let d = tmp("silent-dir");
        let bin = tmp("silent-bin");
        let docker = fake_docker(&bin, "exit 1");
        let e = action_with(&docker, &d, "web", "stop").unwrap_err();
        assert_eq!(e.to_string(), "docker compose failed");
    }
}
