//! Settings › Plugins › Database viewer: Adminer at `db.<tld>`, served by Anvil's default
//! PHP, that logs in without a password to Anvil's own MySQL and PostgreSQL services and
//! opens the SQLite files of the apps Anvil knows (`dbviewer.php`, Adminer's documented
//! `adminer_object()` customization). Off: nothing of it is served or downloaded.

use crate::worktrees::{MainDb, encoded};
use crate::{Config, Site, home, services};
use anyhow::{Context, Result, bail};
use std::fs;
use std::path::PathBuf;

pub const TURNED_OFF: &str = "Turn on Database viewer in Settings › Plugins";

/// Adminer, pinned: the English single-file build. A newer pin replaces the older download.
pub const VERSION: &str = "6.1.2";
const FILE: &str = "adminer-6.1.2-en.php";
/// GitHub's SHA-256 of the release asset, confirmed by hashing the download.
const SHA256: &str = "56fae7a3bdaae6ff12c404378b1045c02dbe9529a5afda0b901ded0e1f63302f";
/// The `index.php` in front of Adminer.
const WRAPPER: &str = include_str!("dbviewer.php");

/// Where it answers: `db.<tld>`.
pub fn host(cfg: &Config) -> String {
    format!("db.{}", cfg.tld)
}

/// `~/.anvil/adminer`: a folder per version, and `servers.json` beside them.
pub fn root() -> PathBuf {
    home().join("adminer")
}

fn dir() -> PathBuf {
    root().join(VERSION)
}

/// The PHP version serving it while it's on: Anvil's default.
pub fn php(cfg: &Config) -> Option<String> {
    cfg.db_viewer.then(|| cfg.php.clone()).flatten()
}

/// Why it is on but not served: no PHP, or an app that has its address (the app keeps it).
pub fn problem(cfg: &Config, sites: &[Site]) -> Option<String> {
    if !cfg.db_viewer {
        return None;
    }
    if cfg.php.is_none() {
        return Some("Needs PHP: install a version in Settings › PHP.".into());
    }
    let host = host(cfg);
    let app = sites.iter().find(|s| {
        crate::apps::serves(&s.kind)
            && (format!("{}.{}", s.name, cfg.tld) == host || s.aliases.contains(&host))
    })?;
    Some(format!(
        "{} answers at {host}, so the Database viewer isn't served there. Rename the app or its linked domain to use it.",
        app.name
    ))
}

/// Its site block, served by `upstream` (the PHP version's shared pool). Every request
/// goes to the wrapper, so nothing else in the folder is reachable.
pub fn caddy_block(cfg: &Config, upstream: &str) -> String {
    let host = host(cfg);
    format!(
        "\nhttp://{host}, https://{host} {{\n\troot * \"{}\"\n\tencode\n\trewrite * /index.php\n\tphp_fastcgi {upstream}\n}}\n",
        crate::caddy_path(&dir())
    )
}

/// Download the pinned Adminer once, checked against its digest, write the wrapper, and
/// drop older versions.
pub fn ensure() -> Result<()> {
    let adminer = dir().join("adminer.php");
    if !adminer.exists() {
        let url = format!("https://github.com/vrana/adminer/releases/download/v{VERSION}/{FILE}");
        crate::replace_dir(&dir(), |d| {
            let got = crate::download(&url, Some(SHA256))?;
            Ok(fs::rename(got, d.join("adminer.php"))?)
        })?;
    }
    let index = dir().join("index.php");
    if fs::read_to_string(&index).ok().as_deref() != Some(WRAPPER) {
        fs::write(&index, WRAPPER)?;
    }
    for old in fs::read_dir(root()).into_iter().flatten().flatten() {
        let name = old.file_name();
        if old.path().is_dir() && name != VERSION && !name.to_string_lossy().starts_with('.') {
            let _ = fs::remove_dir_all(old.path());
        }
    }
    Ok(())
}

/// What an Open link logs in to.
#[derive(Debug, PartialEq)]
pub enum Target {
    /// An Anvil MySQL or PostgreSQL service, at one of its databases or its list.
    Server {
        service: String,
        database: Option<String>,
    },
    /// An app's SQLite file: there, inside the app's folder (symlinks followed).
    Sqlite(PathBuf),
}

/// The database an app uses, as its `.env` says, when the viewer can open it: on an Anvil
/// service, or a SQLite file in its folder. A worktree's is its own.
pub fn target(cfg: &Config, site: &Site) -> Option<Target> {
    use crate::worktrees as w;
    let found = match site.worktree_of {
        Some(_) => w::main_db(cfg, site),
        None => w::main_db_at(cfg, &site.path, &site.path),
    };
    match found {
        MainDb::Anvil { service, database } => Some(Target::Server {
            database: Some(match site.worktree_of {
                Some(_) => w::own_db(cfg, site),
                None => database,
            }),
            service,
        }),
        MainDb::Sqlite { from, to } => {
            let file = if site.worktree_of.is_some() { to } else { from };
            let file = crate::validate::file_in(&site.path, &file.to_string_lossy()).ok()?;
            Some(Target::Sqlite(file))
        }
        MainDb::None | MainDb::External => None,
    }
}

/// How the Overview names it: `shop on mysql`, `database/database.sqlite`.
pub fn label(site: &Site, t: &Target) -> String {
    match t {
        Target::Server { service, database } => match database {
            Some(db) => format!("{db} on {service}"),
            None => service.clone(),
        },
        Target::Sqlite(file) => {
            let base = crate::platform::canonicalize(&site.path).unwrap_or(site.path.clone());
            file.strip_prefix(&base)
                .unwrap_or(file)
                .display()
                .to_string()
        }
    }
}

/// What the wrapper lets in without a password (`servers.json`): Anvil's MySQL and
/// PostgreSQL services as `<driver> 127.0.0.1:<port>`, and the apps' SQLite files.
pub fn allowed(cfg: &Config, sites: &[Site]) -> serde_json::Value {
    let servers: Vec<String> = services::instances(cfg)
        .iter()
        .filter_map(|id| {
            let driver = match services::kind_of(cfg, id).ok()? {
                "mysql" => "server",
                "postgres" => "pgsql",
                _ => return None,
            };
            Some(format!("{driver} 127.0.0.1:{}", services::port(cfg, id)))
        })
        .collect();
    let sqlite: Vec<String> = sites
        .iter()
        .filter_map(|s| match target(cfg, s)? {
            Target::Sqlite(f) => Some(f.display().to_string()),
            Target::Server { .. } => None,
        })
        .collect();
    serde_json::json!({ "servers": servers, "sqlite": sqlite })
}

pub fn write_allowed(cfg: &Config, sites: &[Site]) -> Result<()> {
    fs::create_dir_all(root())?;
    Ok(fs::write(
        root().join("servers.json"),
        allowed(cfg, sites).to_string(),
    )?)
}

/// `http(s)://db.<tld>[:port]`, as apps' addresses: https when new apps get it.
fn base(cfg: &Config) -> String {
    let (scheme, port, standard) = if cfg.https_default == Some(true) {
        ("https", crate::public_https_port(cfg), 443)
    } else {
        ("http", crate::public_http_port(cfg), 80)
    };
    match port == standard {
        true => format!("{scheme}://{}", host(cfg)),
        false => format!("{scheme}://{}:{port}", host(cfg)),
    }
}

/// The Open link's query: `anvil` first (the wrapper drops it), then Adminer's own.
fn query(cfg: &Config, t: &Target) -> Result<String> {
    Ok(match t {
        Target::Server { service, database } => {
            let (driver, user) = match services::kind_of(cfg, service)? {
                "mysql" => ("server", "root"),
                "postgres" => ("pgsql", "postgres"),
                _ => bail!("{service} has no databases to browse"),
            };
            let port = services::port(cfg, service);
            let db = database
                .as_deref()
                .map_or(String::new(), |d| format!("&db={}", encoded(d)));
            format!("?anvil=1&{driver}=127.0.0.1:{port}&username={user}{db}")
        }
        Target::Sqlite(f) => format!(
            "?anvil=1&sqlite=&username=&db={}",
            encoded(&f.to_string_lossy())
        ),
    })
}

/// The address that opens the viewer logged in to `name`: a service (at `database`, or
/// its list of databases), or an app's database. Downloads Adminer when it isn't there.
pub fn url(cfg: &Config, name: &str, database: Option<&str>) -> Result<String> {
    if !cfg.db_viewer {
        bail!("{TURNED_OFF}");
    }
    let sites = crate::sites(cfg);
    if let Some(why) = problem(cfg, &sites) {
        bail!("{why}");
    }
    let t = if services::instances(cfg).iter().any(|id| id == name) {
        Target::Server {
            service: name.into(),
            database: database.map(String::from),
        }
    } else {
        let site = sites
            .iter()
            .find(|s| s.name == crate::site_name(name))
            .with_context(|| format!("No service or app named {name}"))?;
        target(cfg, site).with_context(|| {
            format!(
                "{} uses no database the viewer opens: one on an Anvil MySQL or PostgreSQL service, or a SQLite file in its folder",
                site.name
            )
        })?
    };
    let q = query(cfg, &t)?;
    ensure()?;
    write_allowed(cfg, &sites)?;
    Ok(format!("{}/{q}", base(cfg)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::ServiceConfig;
    use crate::tests::site;

    fn cfg() -> Config {
        let mut cfg = Config {
            db_viewer: true,
            php: Some("8.4".into()),
            ..Config::default()
        };
        for (id, port) in [("mysql", 3307), ("postgres", 5433), ("redis", 6379)] {
            cfg.services.insert(
                id.into(),
                ServiceConfig {
                    port: Some(port),
                    ..Default::default()
                },
            );
        }
        cfg
    }

    fn laravel(dir: &std::path::Path, env: &str) -> Site {
        fs::write(dir.join(".env"), env).unwrap();
        let mut s = site("shop", dir.to_str().unwrap(), Some("8.4"));
        s.kind = "laravel".into();
        s
    }

    #[test]
    fn served_only_while_on_with_php_and_an_app_called_db_keeps_its_address() {
        let mut cfg = cfg();
        let shop = site("shop", "/s/shop", Some("8.4"));
        assert_eq!(php(&cfg).as_deref(), Some("8.4"));
        assert_eq!(problem(&cfg, std::slice::from_ref(&shop)), None);
        let block = caddy_block(&cfg, "unix//h/run/php-8.4.sock");
        assert!(block.starts_with("\nhttp://db.test, https://db.test {\n\troot * \""));
        assert!(block.ends_with(
            "\tencode\n\trewrite * /index.php\n\tphp_fastcgi unix//h/run/php-8.4.sock\n}\n"
        ));

        let db = site("db", "/s/db", None);
        let why = problem(&cfg, &[shop.clone(), db]).unwrap();
        assert!(why.starts_with("db answers at db.test"), "{why}");
        let mut aliased = shop.clone();
        aliased.aliases = vec!["db.test".into()];
        assert!(
            problem(&cfg, &[aliased])
                .unwrap()
                .starts_with("shop answers")
        );

        cfg.php = None;
        assert_eq!(php(&cfg), None);
        assert!(problem(&cfg, &[]).unwrap().starts_with("Needs PHP"));
        cfg.db_viewer = false;
        cfg.php = Some("8.4".into());
        assert_eq!((php(&cfg), problem(&cfg, &[])), (None, None));
    }

    #[test]
    fn an_apps_database_on_a_service_or_in_its_folder() {
        let cfg = cfg();
        let dir = tempfile::tempdir().unwrap();
        let s = laravel(
            dir.path(),
            "DB_CONNECTION=mysql\nDB_PORT=3307\nDB_DATABASE=shop\n",
        );
        let t = target(&cfg, &s).unwrap();
        assert_eq!(
            t,
            Target::Server {
                service: "mysql".into(),
                database: Some("shop".into())
            }
        );
        assert_eq!(label(&s, &t), "shop on mysql");
        // A server Anvil doesn't run: nothing to open.
        let s = laravel(dir.path(), "DB_CONNECTION=pgsql\nDB_PORT=6000\n");
        assert_eq!(target(&cfg, &s), None);

        // SQLite: Laravel's default file, once it's there.
        let s = laravel(dir.path(), "DB_CONNECTION=sqlite\n");
        assert_eq!(target(&cfg, &s), None);
        fs::create_dir_all(dir.path().join("database")).unwrap();
        fs::write(dir.path().join("database/database.sqlite"), "").unwrap();
        let t = target(&cfg, &s).unwrap();
        let real = crate::platform::canonicalize(dir.path()).unwrap();
        assert_eq!(t, Target::Sqlite(real.join("database/database.sqlite")));
        assert_eq!(label(&s, &t), "database/database.sqlite");
        // A file outside the app's folder is never offered.
        let out = tempfile::NamedTempFile::new().unwrap();
        let env = format!(
            "DB_CONNECTION=sqlite\nDB_DATABASE={}\n",
            out.path().display()
        );
        assert_eq!(target(&cfg, &laravel(dir.path(), &env)), None);
    }

    #[test]
    fn only_anvils_own_servers_and_the_apps_files_log_in_without_a_password() {
        let cfg = cfg();
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("database")).unwrap();
        fs::write(dir.path().join("database/database.sqlite"), "").unwrap();
        let s = laravel(dir.path(), "DB_CONNECTION=sqlite\n");
        let file = crate::platform::canonicalize(dir.path())
            .unwrap()
            .join("database/database.sqlite");
        assert_eq!(
            allowed(&cfg, &[s]),
            serde_json::json!({
                "servers": ["server 127.0.0.1:3307", "pgsql 127.0.0.1:5433"],
                "sqlite": [file.display().to_string()],
            })
        );
    }

    #[test]
    fn open_links_log_in_with_adminers_own_parameters() {
        let mut cfg = cfg();
        let server = |service: &str, database: Option<&str>| Target::Server {
            service: service.into(),
            database: database.map(String::from),
        };
        assert_eq!(
            query(&cfg, &server("mysql", Some("shop"))).unwrap(),
            "?anvil=1&server=127.0.0.1:3307&username=root&db=shop"
        );
        assert_eq!(
            query(&cfg, &server("postgres", None)).unwrap(),
            "?anvil=1&pgsql=127.0.0.1:5433&username=postgres"
        );
        assert!(query(&cfg, &server("redis", None)).is_err());
        assert_eq!(
            query(&cfg, &Target::Sqlite("/a b/x.sqlite".into())).unwrap(),
            "?anvil=1&sqlite=&username=&db=%2Fa%20b%2Fx.sqlite"
        );
        // 443 whether or not this Mac forwards the ports.
        cfg.https_default = Some(true);
        cfg.https_port = 443;
        assert_eq!(base(&cfg), "https://db.test");
        cfg.db_viewer = false;
        assert_eq!(
            url(&cfg, "mysql", None).unwrap_err().to_string(),
            TURNED_OFF
        );
    }

    #[test]
    fn ensure_writes_the_wrapper_and_drops_older_versions() {
        let _env = crate::lock(&crate::ENV_LOCK);
        let home = tempfile::tempdir().unwrap();
        // SAFETY: every test that sets ANVIL_HOME holds ENV_LOCK.
        unsafe { std::env::set_var("ANVIL_HOME", home.path()) };
        // Downloaded already: no network needed.
        fs::create_dir_all(dir()).unwrap();
        fs::write(dir().join("adminer.php"), "<?php").unwrap();
        fs::create_dir_all(root().join("5.0.0")).unwrap();
        fs::write(root().join("servers.json"), "{}").unwrap();
        let ok = ensure();
        let index = fs::read_to_string(dir().join("index.php"));
        let old = root().join("5.0.0").exists();
        let kept = root().join("servers.json").exists();
        unsafe { std::env::remove_var("ANVIL_HOME") };
        ok.unwrap();
        assert_eq!(index.unwrap(), WRAPPER);
        assert!(!old && kept);
    }
}
