use anvil::*;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::Command;

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
    /// Set a php.ini value for one site, or clear it by leaving out the value
    Ini {
        site: String,
        key: String,
        value: Option<String>,
    },
    /// Databases, cache and mail: mysql, postgres, redis, mailpit
    Service {
        #[command(subcommand)]
        cmd: ServiceCmd,
    },
    /// Download the web server and print one-time system setup commands
    Setup,
    /// Trust Anvil's local HTTPS certificate authority (asks for your password)
    Trust,
    /// Run DNS, PHP-FPM and the web server in the foreground
    Start,
}

#[derive(Subcommand)]
enum ServiceCmd {
    /// List services and their state
    List,
    /// Download a service (Redis is built from source)
    Install { id: String },
    /// Remove a service's program; its data is kept
    Uninstall { id: String },
    /// Start the service with Anvil
    Enable { id: String },
    /// Don't start the service with Anvil
    Disable { id: String },
    /// Set the port, or reset it to the default by leaving it out
    Port { id: String, port: Option<u16> },
}

#[derive(Subcommand)]
enum PhpCmd {
    /// Install the latest patch release of a PHP minor version, e.g. 8.4
    Install { version: String },
    /// List installed versions
    List,
}

fn dir_or_cwd(p: Option<PathBuf>) -> Result<PathBuf> {
    let p = p.map_or_else(std::env::current_dir, Ok)?;
    p.canonicalize()
        .with_context(|| format!("{} not found", p.display()))
}

fn start(cfg: &Config) -> Result<()> {
    let mut services = services::Services::default();
    for (id, e) in services.start_enabled(cfg) {
        eprintln!("  {id}: {e}");
    }
    let mut server = Server::start(cfg)?;
    for s in sites(cfg) {
        println!(
            "  https://{}.{}:{}  (PHP {})",
            s.name,
            cfg.tld,
            cfg.https_port,
            s.php.as_deref().unwrap_or("-")
        );
    }
    println!(
        "Anvil running, logs in {}. Ctrl-C to stop.",
        home().join("log").display()
    );

    for s in services.info(cfg).iter().filter(|s| s.running) {
        println!("  {} on 127.0.0.1:{}", s.name, s.port);
    }

    // On Ctrl-C / SIGTERM, stop everything cleanly (databases get time to flush).
    let (tx, rx) = std::sync::mpsc::channel();
    ctrlc::set_handler(move || {
        let _ = tx.send(());
    })?;
    let pids = server.pids();
    std::thread::spawn(move || {
        let _ = rx.recv();
        // Stopping the web server ends `server.wait()` below.
        let _ = Command::new("kill")
            .arg(pids.last().unwrap().to_string())
            .status();
    });
    server.wait()?;
    drop(server);
    drop(services);
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
        Cmd::Use { version, site } => match site {
            Some(name) => set_site_php(&mut cfg, &name, Some(version))?,
            None => set_default_php(&mut cfg, &version)?,
        },
        Cmd::Service { cmd } => match cmd {
            ServiceCmd::List => {
                for s in services::Services::default().info(&cfg) {
                    let state = match (s.installed, s.enabled) {
                        (false, _) => "not installed",
                        (true, true) => "enabled",
                        (true, false) => "installed",
                    };
                    println!("{:<12} {:<9} port {:<6} {state}", s.id, s.version, s.port);
                }
            }
            ServiceCmd::Install { id } => {
                services::install(&id)?;
                println!("Installed {id}. Enable it with `anvil service enable {id}`.");
            }
            ServiceCmd::Uninstall { id } => services::uninstall(&id)?,
            ServiceCmd::Enable { id } => services::set_enabled(&mut cfg, &id, true)?,
            ServiceCmd::Disable { id } => services::set_enabled(&mut cfg, &id, false)?,
            ServiceCmd::Port { id, port } => services::set_port(&mut cfg, &id, port)?,
        },
        Cmd::Ini { site, key, value } => set_site_ini(&mut cfg, &site, &key, value.as_deref())?,
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
            trust()?;
            println!("Anvil's certificate authority is now trusted.");
        }
        Cmd::Start => start(&cfg)?,
    }
    Ok(())
}
