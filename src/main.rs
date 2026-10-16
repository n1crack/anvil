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

fn dir_or_cwd(p: Option<PathBuf>) -> Result<PathBuf> {
    let p = p.map_or_else(std::env::current_dir, Ok)?;
    p.canonicalize()
        .with_context(|| format!("{} not found", p.display()))
}

fn start(cfg: &Config) -> Result<()> {
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

    // On Ctrl-C / SIGTERM, take PHP-FPM and the web server down with us.
    let pids: Vec<String> = server.pids().iter().map(u32::to_string).collect();
    ctrlc::set_handler(move || {
        let _ = Command::new("kill").args(&pids).status();
        std::process::exit(0);
    })?;
    server.wait()
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
