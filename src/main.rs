use anvil::*;
use anyhow::{Context, Result, bail};
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
    /// Language toolchains for apps that run their own server: node, go, rust, dotnet
    Runtime {
        #[command(subcommand)]
        cmd: RuntimeCmd,
    },
    /// Apps that run their own server (Node, Rust, Go, .NET)
    App {
        #[command(subcommand)]
        cmd: AppCmd,
    },
    /// Create a new application: laravel, statamic, symfony, php, static, next, nuxt, vite, node, rust, go, dotnet
    New {
        kind: String,
        name: String,
        /// Parent folder (default: the first parked folder)
        #[arg(long = "in")]
        dir: Option<PathBuf>,
    },
    /// Disk usage of an app, what is safe to delete, and the services it uses.
    /// Without an app: Anvil's own footprint and every app's size.
    Usage { site: Option<String> },
    /// Download the web server and print one-time system setup commands
    Setup,
    /// Trust Anvil's local HTTPS certificate authority (asks for your password)
    Trust,
    /// Run DNS, PHP-FPM and the web server in the foreground
    Start,
}

#[derive(Subcommand)]
enum ServiceCmd {
    /// List added services and their state
    List,
    /// Add a service: `add redis`, `add redis cache --port 6380`. Downloads the
    /// program the first time (Redis is built from source).
    Add {
        /// mysql, postgres, redis or mailpit
        kind: String,
        /// Defaults to the kind, then `<kind>-2`…
        name: Option<String>,
        #[arg(long)]
        port: Option<u16>,
    },
    /// Remove a service; its data is kept unless --delete-data
    Remove {
        id: String,
        #[arg(long)]
        delete_data: bool,
    },
    /// Set a service's tags (none clears them)
    Tag { id: String, tags: Vec<String> },
    /// Set the applications that use a service (none clears them)
    Link { id: String, apps: Vec<String> },
    /// Remove a program no service uses anymore, e.g. `uninstall redis`
    Uninstall { kind: String },
    /// Start the service with Anvil
    Enable { id: String },
    /// Don't start the service with Anvil
    Disable { id: String },
    /// Set the port, or reset it to the default by leaving it out
    Port {
        id: String,
        port: Option<u16>,
        /// Mailpit's web inbox port (leave the value out for the default)
        #[arg(long, num_args = 0..=1)]
        web: Option<Option<u16>>,
    },
}

#[derive(Subcommand)]
enum RuntimeCmd {
    /// Installed and available versions
    List,
    /// e.g. `node 22`, `go 1.27.2`, `rust stable`, `dotnet 10.0`
    Install { id: String, version: String },
    /// Remove an installed version, e.g. `node 22`
    Uninstall { id: String, version: String },
}

#[derive(Subcommand)]
enum AppCmd {
    /// Set the start command (leave it out to go back to the default)
    Command {
        site: String,
        command: Option<String>,
    },
    /// Pin the language version (e.g. `24` for Node), or leave it out for automatic
    Runtime {
        site: String,
        version: Option<String>,
    },
    /// Start the app whenever Anvil starts
    Autostart {
        site: String,
        #[arg(action = clap::ArgAction::Set)]
        on: bool,
    },
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
    let mut apps = apps::Apps::default();
    for site in sites(cfg).iter().filter(|s| s.autostart) {
        match apps.start(site, &cfg.tld) {
            Ok(()) => println!(
                "  {} ({}) on 127.0.0.1:{}",
                site.name,
                apps::label(&site.kind),
                site.port.unwrap_or(0)
            ),
            Err(e) => eprintln!("  {}: {e:#}", site.name),
        }
    }
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
        println!("  {} ({}) on 127.0.0.1:{}", s.id, s.name, s.port);
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
    drop(apps);
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
            let name = name.unwrap_or_else(|| p.file_name().unwrap().to_string_lossy().into());
            let name = link(&mut cfg, &name, p.clone())?;
            println!("Linked {name}.{} -> {}", cfg.tld, p.display());
        }
        Cmd::Unlink { name } => {
            cfg.links.remove(&name);
            save(&cfg)?;
        }
        Cmd::Sites => {
            for s in sites(&cfg) {
                println!(
                    "{:<40} {:<9} {:<5} {}",
                    format!("{}.{}", s.name, cfg.tld),
                    apps::label(&s.kind),
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
                let list = services::Services::default().info(&cfg);
                if list.is_empty() {
                    println!("No services yet. Add one with `anvil service add mysql`.");
                }
                for s in list {
                    let state = match (s.installed, s.enabled) {
                        (false, _) => "not installed",
                        (true, true) => "enabled",
                        (true, false) => "installed",
                    };
                    let inbox = s
                        .web_port
                        .map(|p| format!(", inbox on {p}"))
                        .unwrap_or_default();
                    println!(
                        "{:<12} {:<10} {:<9} port {:<6} {state}{inbox}",
                        s.id, s.kind, s.version, s.port
                    );
                }
            }
            ServiceCmd::Add { kind, name, port } => {
                let id = services::add(&mut cfg, &kind, name.as_deref(), port)?;
                if !services::installed(&kind) {
                    println!("Downloading {kind}…");
                    if let Err(e) = services::install(&kind) {
                        services::remove(&mut cfg, &id, false)?;
                        return Err(e);
                    }
                }
                services::set_enabled(&mut cfg, &id, true)?;
                println!(
                    "Added {id} on port {}; it starts with `anvil start`.",
                    services::port(&cfg, &id)
                );
            }
            ServiceCmd::Remove { id, delete_data } => services::remove(&mut cfg, &id, delete_data)?,
            ServiceCmd::Tag { id, tags } => services::set_tags(&mut cfg, &id, &tags)?,
            ServiceCmd::Link { id, apps } => services::set_apps(&mut cfg, &id, &apps)?,
            ServiceCmd::Uninstall { kind } => {
                if let Some(id) = services::instances(&cfg)
                    .into_iter()
                    .find(|i| services::kind_of(&cfg, i).ok() == Some(kind.as_str()))
                {
                    bail!("{id} still uses {kind}; remove it first");
                }
                services::uninstall(&kind)?
            }
            ServiceCmd::Enable { id } => services::set_enabled(&mut cfg, &id, true)?,
            ServiceCmd::Disable { id } => services::set_enabled(&mut cfg, &id, false)?,
            ServiceCmd::Port { id, port, web } => {
                // With --web, the SMTP port only changes when it's given too.
                if let Some(web) = web {
                    services::set_web_port(&mut cfg, &id, web)?;
                }
                if web.is_none() || port.is_some() {
                    services::set_port(&mut cfg, &id, port)?;
                }
            }
        },
        Cmd::Runtime { cmd } => match cmd {
            RuntimeCmd::List => {
                for id in runtimes::RUNTIMES {
                    let info = runtimes::info(id, true);
                    let avail: Vec<_> = info.available.iter().take(6).cloned().collect();
                    println!(
                        "{:<8} installed: {:<20} available: {}",
                        id,
                        info.installed.join(", "),
                        avail.join(", ")
                    );
                }
            }
            RuntimeCmd::Install { id, version } => runtimes::install(&id, &version)?,
            RuntimeCmd::Uninstall { id, version } => runtimes::uninstall(&id, &version)?,
        },
        Cmd::App { cmd } => match cmd {
            AppCmd::Command { site, command } => {
                set_site_app(&mut cfg, &site, None, Some(command), None, None)?
            }
            AppCmd::Runtime { site, version } => set_site_runtime(&mut cfg, &site, version)?,
            AppCmd::Autostart { site, on } => {
                set_site_app(&mut cfg, &site, None, None, None, Some(on))?
            }
        },
        Cmd::New { kind, name, dir } => {
            let parent = match dir {
                Some(d) => d,
                None => cfg
                    .paths
                    .first()
                    .cloned()
                    .context("Park a folder first, or pass --in")?,
            };
            if !create::TEMPLATES.contains(&kind.as_str()) {
                bail!(
                    "Unknown type {kind}; use one of: {}",
                    create::TEMPLATES.join(", ")
                );
            }
            let parent = dir_or_cwd(Some(parent))?;
            println!("Creating {name} ({})…", apps::label(&kind));
            let path = create::create(&kind, &name, &parent, cfg.php.as_deref())?;
            // Outside a parked folder the new app needs a link to get its address.
            if !cfg.paths.contains(&parent) {
                link(&mut cfg, &name, path.clone())?;
            }
            println!("Created {}", path.display());
            println!("Serving at https://{name}.{}:{}", cfg.tld, cfg.https_port);
        }
        Cmd::Usage { site: None } => {
            let mb = |b: u64| format!("{:.1} MB", b as f64 / 1_048_576.0);
            let (total, items) = resources::anvil_disk();
            println!("Anvil: {}", mb(total));
            for i in &items {
                println!(
                    "  {:<6} {:>10}  {}",
                    format!("{:?}", i.safety),
                    mb(i.bytes),
                    i.label
                );
            }
            let all = resources::all_apps(&sites(&cfg));
            let sum = |f: fn(&resources::AppSize) -> u64| mb(all.iter().map(f).sum());
            println!(
                "Applications: {} ({} safe to delete, {} rebuildable)",
                sum(|a| a.total),
                sum(|a| a.safe),
                sum(|a| a.rebuild)
            );
            for a in all.iter().take(10) {
                println!(
                    "  {:>10}  {:<30} {}",
                    mb(a.total),
                    a.name,
                    apps::label(&a.kind)
                );
            }
        }
        Cmd::Usage { site: Some(site) } => {
            let s = sites(&cfg)
                .into_iter()
                .find(|s| s.name == site_name(&site))
                .with_context(|| format!("No app named {site}"))?;
            let (total, items) = resources::disk(&s);
            let mb = |b: u64| format!("{:.1} MB", b as f64 / 1_048_576.0);
            println!(
                "{} ({}): {} on disk",
                s.name,
                apps::label(&s.kind),
                mb(total)
            );
            for i in items.iter().filter(|i| i.bytes > 0) {
                println!(
                    "  {:<10} {:>10}  {:<28} {}",
                    format!("{:?}", i.safety),
                    mb(i.bytes),
                    i.label,
                    i.path
                );
            }
            let mut svc = services::Services::default();
            for u in resources::services_used(&cfg, &s, &mut svc) {
                println!(
                    "  uses {:<12} port {:<6} {:?}{}",
                    u.label,
                    u.port.map_or("-".into(), |p| p.to_string()),
                    u.source,
                    u.detail.map_or(String::new(), |d| format!(" ({d})"))
                );
            }
        }
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
