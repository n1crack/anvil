//! What's inside a running service: databases in MySQL and PostgreSQL, keys in
//! Redis, messages in Mailpit, buckets in S3 storage. Talks to the server with
//! its own bundled client.

use crate::Config;
use crate::services::{kind_of, port, program_dir, web_port};
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Serialize, Default)]
pub struct Contents {
    /// Databases with their size in bytes (MySQL, PostgreSQL).
    pub databases: Vec<(String, u64)>,
    /// Keys per Redis database, e.g. ("db0", 120).
    pub keys: Vec<(String, u64)>,
    /// Memory the server reports using (Redis).
    pub memory: Option<String>,
    /// Caught messages (Mailpit).
    pub messages: Option<u64>,
    /// Buckets with their size in bytes (S3 storage).
    pub buckets: Vec<(String, u64)>,
}

/// Databases the server creates for itself; they can't be dropped from Anvil.
const SYSTEM: [&str; 6] = [
    "information_schema",
    "mysql",
    "performance_schema",
    "sys",
    "postgres",
    "template1",
];

pub(crate) fn check_db(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name.len() <= 63
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if !ok {
        bail!("Use letters, numbers, dashes and underscores for the database name");
    }
    Ok(())
}

/// Run one statement or command with the service's client; stdout as text.
fn client(cfg: &Config, id: &str, query: &str) -> Result<String> {
    let (kind, p) = (kind_of(cfg, id)?, port(cfg, id).to_string());
    let bin = program_dir(cfg, id)?.join("bin");
    let mut c = match kind {
        "mysql" => {
            let mut c = crate::platform::command(bin.join("mysql"));
            c.args([
                "-h",
                "127.0.0.1",
                "-P",
                &p,
                "-u",
                "root",
                "-N",
                "-B",
                "-e",
                query,
            ]);
            c
        }
        "postgres" => {
            let mut c = crate::platform::command(bin.join("psql"));
            c.args([
                "-h",
                "127.0.0.1",
                "-p",
                &p,
                "-U",
                "postgres",
                "-tA",
                "-F",
                "\t",
            ])
            .args(["-v", "ON_ERROR_STOP=1", "-c", query]);
            c
        }
        "redis" => {
            let mut c = crate::platform::command(bin.join("redis-cli"));
            c.args(["-h", "127.0.0.1", "-p", &p])
                .args(query.split_whitespace());
            c
        }
        _ => bail!("{id} has no databases"),
    };
    let out = c.output().with_context(|| format!("couldn't reach {id}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        bail!(
            "{}",
            err.trim().lines().last().unwrap_or("the server refused")
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// What a running service holds. Mailpit is asked over its HTTP API.
pub fn contents(cfg: &Config, id: &str) -> Result<Contents> {
    let mut c = Contents::default();
    match kind_of(cfg, id)? {
        "mysql" => {
            let names = client(cfg, id, "SHOW DATABASES")?;
            let sizes = client(
                cfg,
                id,
                "SELECT table_schema, SUM(data_length + index_length) FROM information_schema.tables GROUP BY table_schema",
            )?;
            let size = |n: &str| {
                parse_rows(&sizes)
                    .into_iter()
                    .find(|r| r.0 == n)
                    .map_or(0, |r| r.1)
            };
            c.databases = names
                .lines()
                .filter(|n| !n.is_empty())
                .map(|n| (n.to_string(), size(n)))
                .collect();
        }
        "postgres" => {
            let rows = client(
                cfg,
                id,
                "SELECT datname, pg_database_size(datname) FROM pg_database WHERE NOT datistemplate ORDER BY 1",
            )?;
            c.databases = parse_rows(&rows);
        }
        "redis" => {
            let info = client(cfg, id, "INFO")?;
            c.keys = parse_keyspace(&info);
            c.memory = info
                .lines()
                .find_map(|l| l.strip_prefix("used_memory_human:"))
                .map(|m| m.trim().to_string());
        }
        // A bucket is a folder of the data folder; read there, no S3 signing needed.
        "versitygw" | "silo" => {
            let dirs: Vec<_> = std::fs::read_dir(crate::services::data(id))?
                .flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
                .map(|e| e.path())
                .collect();
            let sizes = crate::resources::du(&dirs);
            c.buckets = dirs
                .iter()
                .map(|d| {
                    let name = d.file_name().unwrap_or_default().to_string_lossy();
                    (name.into_owned(), sizes.get(d).copied().unwrap_or(0))
                })
                .collect();
            c.buckets.sort();
        }
        _ => {
            let web = web_port(cfg, id).context("no inbox port")?;
            let url = format!("http://127.0.0.1:{web}/api/v1/messages?limit=1");
            let out = crate::curl(&["-fsS", "--max-time", "3"], &url)
                .map_err(|_| anyhow::anyhow!("{id} isn't answering"))?;
            let v: serde_json::Value = serde_json::from_slice(&out)?;
            c.messages = v["total"].as_u64();
        }
    }
    Ok(c)
}

/// Rows of `name<TAB>number`.
fn parse_rows(out: &str) -> Vec<(String, u64)> {
    out.lines()
        .filter_map(|l| {
            let (n, v) = l.split_once('\t')?;
            Some((n.to_string(), v.trim().parse::<f64>().unwrap_or(0.0) as u64))
        })
        .collect()
}

/// `db0:keys=12,expires=0,avg_ttl=0` lines from Redis INFO.
fn parse_keyspace(info: &str) -> Vec<(String, u64)> {
    info.lines()
        .filter_map(|l| {
            let (db, rest) = l.split_once(':')?;
            if !db.starts_with("db") {
                return None;
            }
            let keys = rest.split(',').find_map(|p| p.strip_prefix("keys="))?;
            Some((db.to_string(), keys.trim().parse().ok()?))
        })
        .collect()
}

fn create_mysql(name: &str) -> String {
    format!("CREATE DATABASE `{name}` CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci")
}

pub fn create(cfg: &Config, id: &str, name: &str) -> Result<()> {
    check_db(name)?;
    let q = match kind_of(cfg, id)? {
        "mysql" => create_mysql(name),
        "postgres" => format!("CREATE DATABASE \"{name}\" ENCODING 'UTF8'"),
        _ => bail!("{id} has no databases"),
    };
    client(cfg, id, &q).map(drop)
}

/// Whether the server has a database by that name.
pub fn exists(cfg: &Config, id: &str, name: &str) -> Result<bool> {
    check_db(name)?;
    let q = match kind_of(cfg, id)? {
        "mysql" => {
            format!(
                "SELECT SCHEMA_NAME FROM information_schema.SCHEMATA WHERE SCHEMA_NAME = '{name}'"
            )
        }
        "postgres" => format!("SELECT 1 FROM pg_database WHERE datname = '{name}'"),
        _ => bail!("{id} has no databases"),
    };
    Ok(!client(cfg, id, &q)?.trim().is_empty())
}

/// A program and its arguments.
#[derive(Debug, PartialEq)]
pub(crate) struct Cmd {
    pub program: PathBuf,
    pub args: Vec<String>,
}

/// One step of a clone: a command, or one whose output feeds the other's input.
#[derive(Debug, PartialEq)]
pub(crate) enum Step {
    Run(Cmd),
    Pipe(Cmd, Cmd),
}

/// The commands that make `to` a copy of `from` on a `kind` server at `port`, with
/// the clients in `bin`. The first step creates `to`. PostgreSQL copies through a
/// template, which needs `from` to have no other connections; `busy` dumps it instead.
pub(crate) fn clone_steps(
    kind: &str,
    bin: &Path,
    port: u16,
    from: &str,
    to: &str,
    busy: bool,
) -> Result<Vec<Step>> {
    for name in [from, to] {
        check_db(name)?;
        if name.starts_with('-') {
            bail!("{name} reads as an option to the database tools");
        }
    }
    if from == to {
        bail!("{from} can't be a copy of itself");
    }
    let p = port.to_string();
    let cmd = |program: &str, args: &[&str]| Cmd {
        program: bin.join(program),
        args: args.iter().map(|a| a.to_string()).collect(),
    };
    Ok(match kind {
        "postgres" => {
            let conn = ["-h", "127.0.0.1", "-p", &p, "-U", "postgres"];
            let psql = |sql: &str| {
                cmd(
                    "psql",
                    &[&conn[..], &["-v", "ON_ERROR_STOP=1", "-c", sql]].concat(),
                )
            };
            if !busy {
                vec![Step::Run(psql(&format!(
                    "CREATE DATABASE \"{to}\" TEMPLATE \"{from}\""
                )))]
            } else {
                let (src, dst) = (format!("--dbname={from}"), format!("--dbname={to}"));
                vec![
                    Step::Run(psql(&format!(
                        "CREATE DATABASE \"{to}\" TEMPLATE template0 ENCODING 'UTF8'"
                    ))),
                    Step::Pipe(
                        cmd("pg_dump", &[&conn[..], &["--no-owner", &src]].concat()),
                        cmd(
                            "psql",
                            &[&conn[..], &["-v", "ON_ERROR_STOP=1", "-q", &dst]].concat(),
                        ),
                    ),
                ]
            }
        }
        "mysql" => {
            let conn = ["-h", "127.0.0.1", "-P", &p, "-u", "root"];
            let dump = ["--single-transaction", "--routines", "--triggers", from];
            let dst = format!("--database={to}");
            vec![
                Step::Run(cmd(
                    "mysql",
                    &[&conn[..], &["-e", &create_mysql(to)]].concat(),
                )),
                Step::Pipe(
                    cmd("mysqldump", &[&conn[..], &dump].concat()),
                    cmd("mysql", &[&conn[..], &[dst.as_str()]].concat()),
                ),
            ]
        }
        _ => bail!("{kind} has no databases to copy"),
    })
}

fn command(c: &Cmd) -> Command {
    let mut command = crate::platform::command(&c.program);
    command.args(&c.args);
    command
}

/// What a failed client said on stderr.
fn failed(name: &Path, stderr: &[u8]) -> anyhow::Error {
    let err = String::from_utf8_lossy(stderr);
    let err = err.trim();
    let name = name.file_name().unwrap_or_default().to_string_lossy();
    if err.is_empty() {
        anyhow::anyhow!("{name} failed")
    } else {
        anyhow::anyhow!(
            "{}",
            err.lines().map(str::trim).collect::<Vec<_>>().join(" ")
        )
    }
}

fn run_step(step: &Step) -> Result<()> {
    let reach = |c: &Cmd| format!("couldn't run {}", c.program.display());
    match step {
        Step::Run(c) => {
            let out = command(c)
                .stdin(Stdio::null())
                .output()
                .with_context(|| reach(c))?;
            if !out.status.success() {
                return Err(failed(&c.program, &out.stderr));
            }
        }
        Step::Pipe(a, b) => {
            // ponytail: the dump's stderr is read once the load ends; fine for a few
            // warnings, a thread reading it if a dump ever writes 64 KB of them.
            let mut dump = command(a)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .with_context(|| reach(a))?;
            let out = dump.stdout.take().context("no output to read")?;
            let load = command(b)
                .stdin(out)
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .with_context(|| reach(b));
            let load = match load {
                Ok(l) => l.wait_with_output()?,
                Err(e) => {
                    let _ = dump.kill();
                    let _ = dump.wait();
                    return Err(e);
                }
            };
            let dumped = dump.wait_with_output()?;
            // The load first: when it stops on an error, the dump only reports a broken pipe.
            if !load.status.success() {
                return Err(failed(&b.program, &load.stderr));
            }
            if !dumped.status.success() {
                return Err(failed(&a.program, &dumped.stderr));
            }
        }
    }
    Ok(())
}

/// Make `to` a copy of `from` (a new database; `from` stays as it is). A half-made
/// `to` is dropped again.
pub fn clone_database(cfg: &Config, id: &str, from: &str, to: &str) -> Result<()> {
    let (kind, p) = (kind_of(cfg, id)?, port(cfg, id));
    let bin = program_dir(cfg, id)?.join("bin");
    let run = |busy| -> Result<()> {
        let steps = clone_steps(kind, &bin, p, from, to, busy)?;
        let (create, rest) = steps.split_first().context("nothing to run")?;
        run_step(create)?;
        if let Err(e) = rest.iter().try_for_each(run_step) {
            let _ = drop_database(cfg, id, to);
            return Err(e);
        }
        Ok(())
    };
    match run(false) {
        Err(e)
            if kind == "postgres" && e.to_string().contains("is being accessed by other users") =>
        {
            run(true)
        }
        r => r,
    }
}

/// Drop a database. System databases are refused.
pub fn drop_database(cfg: &Config, id: &str, name: &str) -> Result<()> {
    check_db(name)?;
    if SYSTEM.contains(&name) {
        bail!("{name} belongs to the server and can't be dropped");
    }
    let q = match kind_of(cfg, id)? {
        "mysql" => format!("DROP DATABASE `{name}`"),
        "postgres" => format!("DROP DATABASE \"{name}\" WITH (FORCE)"),
        _ => bail!("{id} has no databases"),
    };
    client(cfg, id, &q).map(drop)
}

/// Empty Redis or Mailpit.
pub fn empty(cfg: &Config, id: &str) -> Result<()> {
    match kind_of(cfg, id)? {
        "redis" => client(cfg, id, "FLUSHALL").map(drop),
        "mailpit" => {
            let web = web_port(cfg, id).context("no inbox port")?;
            let url = format!("http://127.0.0.1:{web}/api/v1/messages");
            crate::curl(&["-fsS", "--max-time", "3", "-X", "DELETE"], &url)
                .map(drop)
                .map_err(|_| anyhow::anyhow!("{id} isn't answering"))
        }
        _ => bail!("Drop databases one by one instead"),
    }
}

/// Database apps installed on this Mac that can open a service by URL.
pub fn clients(kind: &str) -> Vec<&'static str> {
    let apps: &[&str] = match kind {
        "mysql" => &[
            "TablePlus",
            "Sequel Ace",
            "DBeaver",
            "DataGrip",
            "Beekeeper Studio",
        ],
        "postgres" => &[
            "TablePlus",
            "Postico 2",
            "DBeaver",
            "DataGrip",
            "Beekeeper Studio",
        ],
        "redis" => &["TablePlus", "RedisInsight", "Another Redis Desktop Manager"],
        _ => &[],
    };
    let home = crate::user_home();
    apps.iter()
        .copied()
        .filter(|a| {
            std::path::Path::new(&format!("/Applications/{a}.app")).exists()
                || home.join(format!("Applications/{a}.app")).exists()
        })
        .collect()
}

/// A connection URL database apps understand.
pub fn url(cfg: &Config, id: &str, database: Option<&str>) -> Result<String> {
    let p = port(cfg, id);
    let db = database.unwrap_or("");
    Ok(match kind_of(cfg, id)? {
        "mysql" => format!("mysql://root@127.0.0.1:{p}/{db}"),
        "postgres" => format!(
            "postgresql://postgres@127.0.0.1:{p}/{}",
            database.unwrap_or("postgres")
        ),
        "redis" => format!("redis://127.0.0.1:{p}"),
        _ => bail!("{id} has no connection URL"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(c: &Cmd) -> Vec<&str> {
        c.args.iter().map(String::as_str).collect()
    }

    #[test]
    fn clone_commands_are_built_safely() {
        let bin = std::path::Path::new("/s/bin");
        let steps = clone_steps("postgres", bin, 5433, "my-app", "my-app_x", false).unwrap();
        let [Step::Run(c)] = &steps[..] else {
            panic!("{steps:?}")
        };
        assert_eq!(c.program, bin.join("psql"));
        assert_eq!(
            args(c),
            [
                "-h",
                "127.0.0.1",
                "-p",
                "5433",
                "-U",
                "postgres",
                "-v",
                "ON_ERROR_STOP=1",
                "-c",
                "CREATE DATABASE \"my-app_x\" TEMPLATE \"my-app\""
            ]
        );
        // Busy source: a fresh database, then pg_dump | psql.
        let steps = clone_steps("postgres", bin, 5433, "shop", "shop_x", true).unwrap();
        let [Step::Run(create), Step::Pipe(dump, load)] = &steps[..] else {
            panic!("{steps:?}")
        };
        assert!(
            args(create).contains(&"CREATE DATABASE \"shop_x\" TEMPLATE template0 ENCODING 'UTF8'")
        );
        assert_eq!(dump.program, bin.join("pg_dump"));
        assert!(args(dump).ends_with(&["--no-owner", "--dbname=shop"]));
        assert_eq!(load.program, bin.join("psql"));
        assert!(args(load).ends_with(&["-v", "ON_ERROR_STOP=1", "-q", "--dbname=shop_x"]));
        // MySQL: create, then mysqldump | mysql.
        let steps = clone_steps("mysql", bin, 3306, "shop", "shop_x", false).unwrap();
        let [Step::Run(create), Step::Pipe(dump, load)] = &steps[..] else {
            panic!("{steps:?}")
        };
        assert_eq!(create.program, bin.join("mysql"));
        assert_eq!(
            args(create).last(),
            Some(&"CREATE DATABASE `shop_x` CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci")
        );
        assert_eq!(dump.program, bin.join("mysqldump"));
        assert_eq!(
            args(dump),
            [
                "-h",
                "127.0.0.1",
                "-P",
                "3306",
                "-u",
                "root",
                "--single-transaction",
                "--routines",
                "--triggers",
                "shop"
            ]
        );
        assert_eq!(load.program, bin.join("mysql"));
        assert_eq!(args(load).last(), Some(&"--database=shop_x"));
        // Refused: bad names, a leading dash (read as an option), the same name, other kinds.
        for (from, to) in [
            ("shop", "x\"; DROP"),
            ("a`b", "x"),
            ("-shop", "x"),
            ("shop", "-x"),
            ("shop", "shop"),
        ] {
            assert!(
                clone_steps("mysql", bin, 1, from, to, false).is_err(),
                "{from} {to}"
            );
            assert!(
                clone_steps("postgres", bin, 1, from, to, true).is_err(),
                "{from} {to}"
            );
        }
        assert!(clone_steps("redis", bin, 1, "a", "b", false).is_err());
    }

    #[test]
    fn parsing_and_names() {
        assert_eq!(
            parse_rows("app\t16384\nmysql\t2.5e6\n"),
            [("app".to_string(), 16384), ("mysql".to_string(), 2_500_000)]
        );
        let info =
            "# Keyspace\r\ndb0:keys=12,expires=1,avg_ttl=0\r\ndb3:keys=4,expires=0,avg_ttl=0\r\n";
        assert_eq!(
            parse_keyspace(info),
            [("db0".to_string(), 12), ("db3".to_string(), 4)]
        );
        assert!(check_db("shop_test").is_ok() && check_db("my-app").is_ok());
        for bad in ["", "a`b", "x\"; DROP", "a b", "../x"] {
            assert!(check_db(bad).is_err(), "{bad}");
        }
    }
}
