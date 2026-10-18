//! What's inside a running service: databases in MySQL and PostgreSQL, keys in
//! Redis, messages in Mailpit. Talks to the server with its own bundled client.

use crate::Config;
use crate::services::{kind_of, port, program_dir, web_port};
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::process::Command;

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

fn check_db(name: &str) -> Result<()> {
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
            let mut c = Command::new(bin.join("mysql"));
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
            let mut c = Command::new(bin.join("psql"));
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
            let mut c = Command::new(bin.join("redis-cli"));
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

pub fn create(cfg: &Config, id: &str, name: &str) -> Result<()> {
    check_db(name)?;
    let q = match kind_of(cfg, id)? {
        "mysql" => {
            format!("CREATE DATABASE `{name}` CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci")
        }
        "postgres" => format!("CREATE DATABASE \"{name}\" ENCODING 'UTF8'"),
        _ => bail!("{id} has no databases"),
    };
    client(cfg, id, &q).map(drop)
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
