mod dns;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};

const CADDY_VERSION: &str = "2.11.7";
const PHP_INDEX: &str = "https://dl.static-php.dev/static-php-cli/common/";

#[derive(Parser)]
#[command(version, about = "Anvil — your tools, your rules.")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Serve every sub-folder of a directory (default: current)
    Park { path: Option<PathBuf> },
    /// Stop serving a parked directory
    Forget { path: Option<PathBuf> },
    /// Serve the current directory as <name>.<tld>
    Link { name: Option<String> },
    /// Remove a linked site
    Unlink { name: String },
    /// List sites
    Sites,
    /// Manage PHP versions
    Php {
        #[command(subcommand)]
        cmd: PhpCmd,
    },
    /// Set the PHP version globally, or for one site with --site
    Use {
        version: String,
        #[arg(long)]
        site: Option<String>,
    },
    /// Download the web server and print one-time system setup commands
    Setup,
    /// Trust Anvil's local HTTPS certificate authority (asks for your password)
    Trust,
    /// Run DNS, PHP-FPM and the web server in the foreground
    Start,
}

#[derive(Subcommand)]
enum PhpCmd {
    /// Install the latest patch release of a PHP minor version, e.g. 8.4
    Install { version: String },
    /// List installed versions
    List,
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Config {
    tld: String,
    http_port: u16,
    https_port: u16,
    dns_port: u16,
    php: Option<String>,
    paths: Vec<PathBuf>,
    links: BTreeMap<String, PathBuf>,
    sites: BTreeMap<String, SiteConfig>,
}

#[derive(Serialize, Deserialize, Default)]
struct SiteConfig {
    php: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        // ponytail: macOS only lets root bind 80/443 on loopback; switch the defaults once a privileged helper exists.
        Config {
            tld: "test".into(),
            http_port: 8080,
            https_port: 8443,
            dns_port: 5354,
            php: None,
            paths: vec![],
            links: BTreeMap::new(),
            sites: BTreeMap::new(),
        }
    }
}

struct Site {
    name: String,
    root: PathBuf,
    php: Option<String>,
}

fn home() -> PathBuf {
    std::env::var_os("ANVIL_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").expect("HOME not set")).join(".anvil")
        })
}

fn load() -> Result<Config> {
    match fs::read_to_string(home().join("config.toml")) {
        Ok(s) => Ok(toml::from_str(&s).context("config.toml is invalid")?),
        Err(_) => Ok(Config::default()),
    }
}

fn save(cfg: &Config) -> Result<()> {
    fs::create_dir_all(home())?;
    fs::write(home().join("config.toml"), toml::to_string_pretty(cfg)?)?;
    Ok(())
}

fn site_name(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

fn dir_or_cwd(p: Option<PathBuf>) -> Result<PathBuf> {
    let p = p.map_or_else(std::env::current_dir, Ok)?;
    p.canonicalize()
        .with_context(|| format!("{} not found", p.display()))
}

fn sites(cfg: &Config) -> Vec<Site> {
    let mut found = BTreeMap::new();
    for dir in &cfg.paths {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if e.path().is_dir() && !name.starts_with('.') {
                found.insert(site_name(&name), e.path());
            }
        }
    }
    for (name, path) in &cfg.links {
        found.insert(name.clone(), path.clone());
    }
    found
        .into_iter()
        .map(|(name, path)| {
            let public = path.join("public");
            let root = if public.is_dir() { public } else { path };
            let php = cfg
                .sites
                .get(&name)
                .and_then(|s| s.php.clone())
                .or_else(|| cfg.php.clone());
            Site { name, root, php }
        })
        .collect()
}

fn platform() -> Result<(&'static str, &'static str)> {
    // (static-php-cli suffix, caddy suffix)
    Ok(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => ("macos-aarch64", "mac_arm64"),
        ("macos", "x86_64") => ("macos-x86_64", "mac_amd64"),
        ("linux", "aarch64") => ("linux-aarch64", "linux_arm64"),
        ("linux", "x86_64") => ("linux-x86_64", "linux_amd64"),
        (os, arch) => bail!("{os}/{arch} is not supported yet"),
    })
}

fn run(cmd: &mut Command) -> Result<()> {
    let status = cmd
        .status()
        .with_context(|| format!("failed to run {cmd:?}"))?;
    if !status.success() {
        bail!("{cmd:?} exited with {status}");
    }
    Ok(())
}

/// Download a .tar.gz and unpack it into `dir`.
fn fetch_tgz(url: &str, dir: &Path) -> Result<()> {
    fs::create_dir_all(dir)?;
    let tmp = dir.join(".download.tar.gz");
    run(Command::new("curl").args(["-fL#", "-o"]).arg(&tmp).arg(url))?;
    run(Command::new("tar").arg("-xzf").arg(&tmp).arg("-C").arg(dir))?;
    fs::remove_file(tmp)?;
    Ok(())
}

fn php_dir(v: &str) -> PathBuf {
    home().join("php").join(v)
}

fn installed_php() -> Vec<String> {
    let mut v: Vec<_> = fs::read_dir(home().join("php"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().join("php-fpm").exists())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

fn php_install(cfg: &mut Config, minor: &str) -> Result<()> {
    let (plat, _) = platform()?;
    let out = Command::new("curl")
        .args(["-fsSL", &format!("{PHP_INDEX}?format=json")])
        .output()?;
    let list: Vec<serde_json::Value> =
        serde_json::from_slice(&out.stdout).context("bad PHP index")?;
    let prefix = format!("php-{minor}.");
    let suffix = format!("-fpm-{plat}.tar.gz");
    let patch = list
        .iter()
        .filter_map(|f| f["name"].as_str())
        .filter_map(|n| {
            n.strip_prefix(&prefix)?
                .strip_suffix(&suffix)?
                .parse::<u32>()
                .ok()
        })
        .max()
        .with_context(|| format!("no PHP {minor} build for {plat}"))?;
    let full = format!("{minor}.{patch}");
    let dir = php_dir(minor);
    println!("Installing PHP {full}…");
    for kind in ["fpm", "cli"] {
        fetch_tgz(&format!("{PHP_INDEX}php-{full}-{kind}-{plat}.tar.gz"), &dir)?;
    }
    let ini = dir.join("php.ini");
    if !ini.exists() {
        fs::write(
            &ini,
            "memory_limit = 512M\nupload_max_filesize = 100M\npost_max_size = 100M\nmax_execution_time = 120\n",
        )?;
    }
    if cfg.php.is_none() {
        cfg.php = Some(minor.into());
        save(cfg)?;
    }
    println!("PHP {full} installed in {}", dir.display());
    Ok(())
}

fn caddy() -> PathBuf {
    home().join("bin").join("caddy")
}

fn ensure_caddy() -> Result<()> {
    if caddy().exists() {
        return Ok(());
    }
    let (_, plat) = platform()?;
    println!("Downloading Caddy {CADDY_VERSION}…");
    fetch_tgz(
        &format!(
            "https://github.com/caddyserver/caddy/releases/download/v{CADDY_VERSION}/caddy_{CADDY_VERSION}_{plat}.tar.gz"
        ),
        &home().join("bin"),
    )
}

/// Caddy keeps its CA and certs inside ANVIL_HOME, not in the user's global dirs.
fn caddy_cmd() -> Command {
    let mut c = Command::new(caddy());
    c.env("XDG_DATA_HOME", home().join("data"))
        .env("XDG_CONFIG_HOME", home().join("data"));
    c
}

fn fpm_socket(v: &str) -> PathBuf {
    home().join("run").join(format!("php-{v}.sock"))
}

fn caddyfile(cfg: &Config, sites: &[Site]) -> String {
    let mut s = format!(
        "{{\n\tdefault_bind 127.0.0.1 [::1]\n\thttp_port {}\n\thttps_port {}\n\tadmin off\n\tlocal_certs\n\tskip_install_trust\n\tauto_https disable_redirects\n}}\n",
        cfg.http_port, cfg.https_port
    );
    for site in sites {
        s += &format!(
            "\n{0}.{1}, http://{0}.{1} {{\n\troot * \"{2}\"\n\tencode\n",
            site.name,
            cfg.tld,
            site.root.display()
        );
        if let Some(v) = &site.php {
            s += &format!("\tphp_fastcgi unix/{}\n", fpm_socket(v).display());
        }
        s += "\tfile_server {\n\t\thide .*\n\t}\n}\n";
    }
    s
}

fn start(cfg: &Config) -> Result<()> {
    ensure_caddy()?;
    let sites = sites(cfg);
    let versions: BTreeSet<_> = sites.iter().filter_map(|s| s.php.clone()).collect();
    for v in &versions {
        if !php_dir(v).join("php-fpm").exists() {
            bail!("PHP {v} is used but not installed — run `anvil php install {v}`");
        }
    }
    for d in ["run", "log"] {
        fs::create_dir_all(home().join(d))?;
    }

    let mut children: Vec<Child> = Vec::new();
    for v in &versions {
        let sock = fpm_socket(v);
        let _ = fs::remove_file(&sock);
        let conf = home().join("run").join(format!("php-{v}-fpm.conf"));
        fs::write(
            &conf,
            format!(
                "[global]\nerror_log = {log}\ndaemonize = no\n\n[www]\nlisten = {sock}\npm = ondemand\npm.max_children = 16\ncatch_workers_output = yes\nclear_env = no\n",
                log = home()
                    .join("log")
                    .join(format!("php-{v}-fpm.log"))
                    .display(),
                sock = sock.display(),
            ),
        )?;
        children.push(
            Command::new(php_dir(v).join("php-fpm"))
                .arg("-y")
                .arg(&conf)
                .arg("-c")
                .arg(php_dir(v).join("php.ini"))
                .spawn()
                .with_context(|| format!("failed to start PHP {v}"))?,
        );
    }

    let file = home().join("run").join("Caddyfile");
    fs::write(&file, caddyfile(cfg, &sites))?;

    let (port, tld) = (cfg.dns_port, cfg.tld.clone());
    std::thread::spawn(move || {
        if let Err(e) = dns::serve(port, tld) {
            eprintln!("DNS server stopped: {e}");
        }
    });

    for s in &sites {
        println!(
            "  https://{}.{}:{}  (PHP {})",
            s.name,
            cfg.tld,
            cfg.https_port,
            s.php.as_deref().unwrap_or("-")
        );
    }
    println!("Anvil running. Ctrl-C to stop.");
    children.push(
        caddy_cmd()
            .args(["run", "--adapter", "caddyfile", "--config"])
            .arg(&file)
            .spawn()?,
    );

    // On Ctrl-C / SIGTERM, take PHP-FPM and Caddy down with us.
    let pids: Vec<String> = children.iter().map(|c| c.id().to_string()).collect();
    ctrlc::set_handler(move || {
        let _ = Command::new("kill").args(&pids).status();
        std::process::exit(0);
    })?;

    let status = children.last_mut().unwrap().wait();
    for mut c in children {
        let _ = c.kill();
    }
    status?;
    Ok(())
}

fn main() -> Result<()> {
    let mut cfg = load()?;
    match Cli::parse().cmd {
        Cmd::Park { path } => {
            let p = dir_or_cwd(path)?;
            if !cfg.paths.contains(&p) {
                cfg.paths.push(p.clone());
            }
            save(&cfg)?;
            println!("Parked {}", p.display());
        }
        Cmd::Forget { path } => {
            let p = dir_or_cwd(path)?;
            cfg.paths.retain(|x| x != &p);
            save(&cfg)?;
        }
        Cmd::Link { name } => {
            let p = dir_or_cwd(None)?;
            let name =
                site_name(&name.unwrap_or_else(|| p.file_name().unwrap().to_string_lossy().into()));
            println!("Linked {name}.{} -> {}", cfg.tld, p.display());
            cfg.links.insert(name, p);
            save(&cfg)?;
        }
        Cmd::Unlink { name } => {
            cfg.links.remove(&name);
            save(&cfg)?;
        }
        Cmd::Sites => {
            for s in sites(&cfg) {
                println!(
                    "{:<40} {:<5} {}",
                    format!("{}.{}", s.name, cfg.tld),
                    s.php.as_deref().unwrap_or("-"),
                    s.root.display()
                );
            }
        }
        Cmd::Php {
            cmd: PhpCmd::Install { version },
        } => php_install(&mut cfg, &version)?,
        Cmd::Php { cmd: PhpCmd::List } => {
            for v in installed_php() {
                let mark = if cfg.php.as_deref() == Some(&v) {
                    " (default)"
                } else {
                    ""
                };
                println!("{v}{mark}");
            }
        }
        Cmd::Use { version, site } => {
            if !installed_php().contains(&version) {
                bail!("PHP {version} is not installed — run `anvil php install {version}`");
            }
            match site {
                Some(name) => cfg.sites.entry(site_name(&name)).or_default().php = Some(version),
                None => cfg.php = Some(version),
            }
            save(&cfg)?;
        }
        Cmd::Setup => {
            ensure_caddy()?;
            save(&cfg)?;
            println!(
                "Run once so *.{tld} resolves to Anvil:\n\n  sudo mkdir -p /etc/resolver && printf 'nameserver 127.0.0.1\\nport {port}\\n' | sudo tee /etc/resolver/{tld}\n\nThen `anvil trust` for HTTPS.",
                tld = cfg.tld,
                port = cfg.dns_port
            );
        }
        Cmd::Trust => {
            // Caddy creates this CA on its first start.
            let root = home().join("data/caddy/pki/authorities/local/root.crt");
            if !root.exists() {
                bail!("No certificate authority yet — run `anvil start` once, then `anvil trust`");
            }
            // ponytail: macOS login keychain only; Linux (update-ca-certificates / NSS) comes with the Linux port.
            let keychain = PathBuf::from(std::env::var_os("HOME").unwrap())
                .join("Library/Keychains/login.keychain-db");
            run(Command::new("security")
                .args(["add-trusted-cert", "-r", "trustRoot", "-k"])
                .arg(keychain)
                .arg(root))?;
            println!("Anvil's certificate authority is now trusted.");
        }
        Cmd::Start => start(&cfg)?,
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caddyfile_routes_php_sites_to_their_socket() {
        let cfg = Config::default();
        let sites = [
            Site {
                name: "shop".into(),
                root: "/s/shop/public".into(),
                php: Some("8.3".into()),
            },
            Site {
                name: "static".into(),
                root: "/s/static".into(),
                php: None,
            },
        ];
        let out = caddyfile(&cfg, &sites);
        assert!(out.contains("https_port 8443"));
        assert!(out.contains("default_bind 127.0.0.1 [::1]"));
        assert_eq!(out.matches("hide .*").count(), 2);
        assert!(out.contains("shop.test, http://shop.test {\n\troot * \"/s/shop/public\""));
        assert!(out.contains("php-8.3.sock"));
        assert_eq!(out.matches("php_fastcgi").count(), 1);
        assert_eq!(site_name("My Shop.be"), "my-shop.be");
    }
}
