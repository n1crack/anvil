use anvil::style::{bold, commands, cyan, dim, green, red, yellow};
use anvil::*;
use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::{Command, ExitCode};

#[derive(Parser)]
#[command(version, about = "Anvil — your tools, your rules.", styles = clap::builder::Styles::styled())]
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
    /// Language toolchains for apps that run their own server: node, go, rust, dotnet, python
    Runtime {
        #[command(subcommand)]
        cmd: RuntimeCmd,
    },
    /// Apps that run their own server (Node, Rust, Go, .NET, Python)
    App {
        #[command(subcommand)]
        cmd: AppCmd,
    },
    /// Create a new application: laravel, statamic, symfony, php, static, next, nuxt, vite, node, rust, go, dotnet, django, fastapi, flask
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
    /// Delete items `anvil usage` lists for an app (by path), or with --safe every
    /// item that is safe to delete; --safe without an app cleans every app
    Clean {
        site: Option<String>,
        paths: Vec<String>,
        #[arg(long)]
        safe: bool,
    },
    /// Branch, changes, last commit and remote of an app (default: current folder)
    Git { site: Option<String> },
    /// Recent log entries of an app, oldest first
    Logs {
        site: String,
        /// A file name like laravel.log, "app" or "php-fpm" (default: the newest)
        #[arg(long)]
        source: Option<String>,
        /// How many entries
        #[arg(short, default_value_t = 20)]
        n: usize,
        /// Only error, warning, info or debug
        #[arg(long)]
        level: Option<String>,
    },
    /// Dependencies of an app (or a folder, default: current) from its manifests and lock files
    Packages {
        site: Option<String>,
        /// Also ask the registries for newer versions
        #[arg(long)]
        updates: bool,
    },
    /// Download the web server and print one-time system setup commands
    Setup,
    /// Trust Anvil's local HTTPS certificate authority (asks for your password)
    Trust,
    /// Run DNS, PHP-FPM and the web server in the foreground
    Start,
    /// Serve a site over HTTPS, its http:// address redirecting there; with --off,
    /// HTTP only. New sites get HTTP unless `https_default = true` in config.toml
    Https {
        site: String,
        #[arg(long)]
        off: bool,
    },
    /// Show the domain suffix applications answer at, or set it: `anvil tld localhost`
    /// needs no password or resolver file; `anvil tld test` needs one setup step
    Tld { tld: Option<String> },
    /// List what Anvil changed on this Mac (resolver files, keychain, login item,
    /// ~/.anvil); with --yes, remove it all
    Uninstall {
        #[arg(long)]
        yes: bool,
    },
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
        /// e.g. 8.0.46; `anvil service versions` lists them
        #[arg(long)]
        version: Option<String>,
    },
    /// Versions each service offers, and which are installed
    Versions,
    /// What a running service holds: databases, keys or messages
    Databases { id: String },
    /// Create a database in a running MySQL or PostgreSQL service
    CreateDb { id: String, name: String },
    /// Drop a database from a running MySQL or PostgreSQL service
    DropDb { id: String, name: String },
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
    /// Remove a program version no service uses anymore, e.g. `uninstall redis 7.2.16`
    Uninstall { kind: String, version: String },
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
    /// Set the app type (e.g. `vite`), or leave it out to use the detected one
    Type { site: String, kind: Option<String> },
    /// Pin the app's private port, or leave it out for an automatic one
    Port { site: String, port: Option<u16> },
}

#[derive(Subcommand)]
enum PhpCmd {
    /// Install the latest patch release of a PHP minor version, e.g. 8.4
    Install { version: String },
    /// List installed versions
    List,
    /// Remove an installed version
    Uninstall { version: String },
    /// Set a php.ini value for every site, or clear it by leaving out the value
    Ini { key: String, value: Option<String> },
}

fn dir_or_cwd(p: Option<PathBuf>) -> Result<PathBuf> {
    let p = p.map_or_else(std::env::current_dir, Ok)?;
    p.canonicalize()
        .with_context(|| format!("{} not found", p.display()))
}

fn start(cfg: &Config) -> Result<()> {
    // On Ctrl-C / SIGTERM, stop everything cleanly (databases get time to flush).
    // Set first: a signal during startup waits here instead of killing Anvil and
    // leaving its children behind.
    let (tx, rx) = std::sync::mpsc::channel();
    ctrlc::set_handler(move || {
        let _ = tx.send(());
    })?;
    let mut services = services::Services::default();
    for (id, e) in services.start_enabled(cfg) {
        eprintln!("  {id}: {}", style::error(e));
    }
    let mut server = Server::start(cfg)?;
    let mut apps = apps::Apps::default();
    for site in sites(cfg).iter().filter(|s| s.autostart) {
        match apps.start(site, &cfg.tld) {
            Ok(()) => println!(
                "  {} ({}) on {}",
                green(&site.name),
                dim(apps::label(&site.kind)),
                cyan(format!("127.0.0.1:{}", site.port.unwrap_or(0)))
            ),
            Err(e) => eprintln!("  {}: {}", site.name, style::error(format!("{e:#}"))),
        }
    }
    for s in sites(cfg) {
        println!(
            "  {}  {}",
            cyan(site_url(cfg, &s)),
            dim(format!("(PHP {})", s.php.as_deref().unwrap_or("-")))
        );
    }
    println!(
        "{}, logs in {}. Ctrl-C to stop.",
        green("Anvil running"),
        home().join("log").display()
    );

    for s in services.info(cfg).iter().filter(|s| s.running) {
        println!(
            "  {} ({}) on {}",
            green(&s.id),
            dim(s.name),
            cyan(format!("127.0.0.1:{}", s.port))
        );
    }

    let pids = server.pids();
    std::thread::spawn(move || {
        let _ = rx.recv();
        // Stopping the web server ends `server.wait()` below.
        let _ = Command::new("kill")
            .arg(pids.last().unwrap().1.to_string())
            .status();
    });
    server.wait()?;
    drop(apps);
    drop(server);
    drop(services);
    Ok(())
}

/// The one-time root step for a suffix other than .localhost.
fn print_resolver_step(cfg: &Config) {
    if !needs_resolver(cfg) {
        println!("*.localhost already resolves to this Mac; no resolver file needed.");
        return;
    }
    println!(
        "{}\n\n  {}\n",
        commands(&format!(
            "Run once so *.{} resolves to Anvil (or use `anvil tld localhost`, which needs no password):",
            cfg.tld
        )),
        cyan(format!(
            "sudo mkdir -p /etc/resolver && printf 'nameserver 127.0.0.1\\nport {port}\\n' | sudo tee /etc/resolver/{tld}",
            tld = cfg.tld,
            port = cfg.dns_port
        ))
    );
}

/// The one-time root step that lets addresses drop `:8443`.
fn print_ports_step(cfg: &Config) -> Result<()> {
    if ports_forwarded(cfg) {
        return Ok(());
    }
    let tmp = stage_ports_daemon(cfg)?;
    let (file, _) = ports_daemon(cfg);
    println!(
        "Run once so addresses work without a port number (ports 80 and 443 are forwarded to Anvil):\n\n  {}\n",
        cyan(format!(
            "sudo launchctl bootout system/{PORTS_LABEL} 2>/dev/null; sudo cp {} {} && sudo launchctl bootstrap system {}",
            tmp.display(),
            file.display(),
            file.display()
        ))
    );
    Ok(())
}

fn main() -> ExitCode {
    // Same output as returning the Result, with `Error:` in red on a terminal.
    if let Err(e) = run() {
        eprintln!("{} {e:?}", style::error("Error:"));
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn run() -> Result<()> {
    let cmd = Cli::parse().cmd;
    // A broken config must not stop an uninstall; it then keeps ~/.anvil.
    let loaded = load();
    let config_ok = loaded.is_ok();
    let mut cfg = match (&cmd, loaded) {
        (Cmd::Uninstall { .. }, Err(_)) => Config::default(),
        (_, cfg) => cfg?,
    };
    match cmd {
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
            println!(
                "Linked {} -> {}",
                cyan(format!("{name}.{}", cfg.tld)),
                p.display()
            );
        }
        Cmd::Unlink { name } => {
            cfg.links.remove(&name);
            save(&cfg)?;
        }
        Cmd::Sites => {
            for s in sites(&cfg) {
                println!(
                    "{} {} {:<5} {}",
                    cyan(format!("{:<40}", s.host)),
                    dim(format!("{:<9}", apps::label(&s.kind))),
                    s.php.as_deref().unwrap_or("-"),
                    dim(s.root.display())
                );
            }
        }
        Cmd::Php {
            cmd: PhpCmd::Install { version },
        } => php_install(&mut cfg, &version)?,
        Cmd::Php {
            cmd: PhpCmd::Uninstall { version },
        } => php_uninstall(&cfg, &version)?,
        Cmd::Php {
            cmd: PhpCmd::Ini { key, value },
        } => set_global_ini(&mut cfg, &key, value.as_deref())?,
        Cmd::Clean {
            site: None,
            safe: true,
            ..
        } => {
            let (freed, failed) = resources::clean_all_safe(&sites(&cfg));
            println!("Freed {:.1} MB", freed as f64 / 1_048_576.0);
            for f in failed {
                println!("  {}", red(format!("couldn't clean {f}")));
            }
        }
        Cmd::Clean { site: None, .. } => bail!("Name an app, or use --safe to clean every app"),
        Cmd::Clean {
            site: Some(site),
            paths,
            safe,
        } => {
            let s = sites(&cfg)
                .into_iter()
                .find(|s| s.name == site_name(&site))
                .with_context(|| format!("No app named {site}"))?;
            let running = apps::running_anywhere(&s.name);
            let mut targets = paths;
            if safe {
                let (_, items) = resources::disk(&s);
                targets.extend(
                    items
                        .into_iter()
                        .filter(|i| i.bytes > 0 && i.safety == resources::Safety::Safe)
                        .map(|i| i.path),
                );
            }
            if targets.is_empty() {
                bail!("Nothing to clean: pass paths from `anvil usage {site}`, or --safe");
            }
            let (mut freed, mut failed) = (0, 0);
            for t in targets {
                // Accept full paths inside the project as well as the relative ones `usage` prints.
                let t = PathBuf::from(&t)
                    .strip_prefix(&s.path)
                    .map_or(t.clone(), |r| r.display().to_string());
                match resources::clean(&s, &t, running) {
                    Ok(b) => freed += b,
                    Err(e) => {
                        println!("  {t}: {}", red(format!("{e:#}")));
                        failed += 1;
                    }
                }
            }
            println!("Freed {:.1} MB", freed as f64 / 1_048_576.0);
            if failed > 0 {
                bail!("{failed} item(s) not cleaned");
            }
        }
        Cmd::Php { cmd: PhpCmd::List } => {
            for v in installed_php() {
                let mark = if cfg.php.as_deref() == Some(&v) {
                    dim(" (default)")
                } else {
                    String::new()
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
                    println!(
                        "{}",
                        commands("No services yet. Add one with `anvil service add mysql`.")
                    );
                }
                for s in list {
                    let state = match (s.installed, s.enabled) {
                        (false, _) => dim("not installed"),
                        (true, true) => "enabled".into(),
                        (true, false) => "installed".into(),
                    };
                    let inbox = s
                        .web_port
                        .map(|p| format!(", inbox on {p}"))
                        .unwrap_or_default();
                    println!(
                        "{} {} {:<9} port {:<6} {state}{inbox}",
                        bold(format!("{:<12}", s.id)),
                        dim(format!("{:<10}", s.kind)),
                        s.version,
                        s.port
                    );
                }
            }
            ServiceCmd::Add {
                kind,
                name,
                port,
                version,
            } => {
                let id = services::add(&mut cfg, &kind, name.as_deref(), port, version.as_deref())?;
                let v = services::version(&cfg, &id);
                if !services::installed(&kind, &v) {
                    println!("Downloading {kind} {v}…");
                }
                if let Err(e) = services::ensure_installed(&cfg, &id) {
                    services::remove(&mut cfg, &id, false)?;
                    return Err(e);
                }
                services::set_enabled(&mut cfg, &id, true)?;
                println!(
                    "Added {} on port {}; it starts with {}.",
                    bold(&id),
                    services::port(&cfg, &id),
                    cyan("`anvil start`")
                );
            }
            ServiceCmd::Remove { id, delete_data } => services::remove(&mut cfg, &id, delete_data)?,
            ServiceCmd::Tag { id, tags } => services::set_tags(&mut cfg, &id, &tags)?,
            ServiceCmd::Link { id, apps } => services::set_apps(&mut cfg, &id, &apps)?,
            ServiceCmd::Databases { id } => {
                let c = anvil::databases::contents(&cfg, &id)?;
                for (n, b) in c.databases {
                    println!("{n:<32} {:>10} KB", b / 1024);
                }
                for (db, k) in c.keys {
                    println!("{db:<8} {k} keys");
                }
                if let Some(m) = c.memory {
                    println!("memory   {m}");
                }
                if let Some(m) = c.messages {
                    println!("{m} messages");
                }
            }
            ServiceCmd::CreateDb { id, name } => anvil::databases::create(&cfg, &id, &name)?,
            ServiceCmd::DropDb { id, name } => anvil::databases::drop_database(&cfg, &id, &name)?,
            ServiceCmd::Versions => {
                for kind in services::SERVICES {
                    let installed = services::installed_versions(kind);
                    let list: Vec<String> = services::versions(kind)
                        .iter()
                        .map(|v| {
                            let mut t = v.to_string();
                            if *v == services::default_version(kind) {
                                t += " (default)";
                            }
                            if installed.iter().any(|i| i == v) {
                                t += " (installed)";
                            }
                            t
                        })
                        .collect();
                    println!("{} {}", bold(format!("{kind:<9}")), list.join(", "));
                }
            }
            ServiceCmd::Uninstall { kind, version } => {
                if let Some(id) = services::instances(&cfg).into_iter().find(|i| {
                    services::kind_of(&cfg, i).ok() == Some(kind.as_str())
                        && services::version(&cfg, i) == version
                }) {
                    bail!("{id} still uses {kind} {version}; remove it first");
                }
                services::uninstall(&kind, &version)?
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
                        "{} installed: {:<20} {}",
                        bold(format!("{id:<8}")),
                        info.installed.join(", "),
                        dim(format!("available: {}", avail.join(", ")))
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
            AppCmd::Type { site, kind } => {
                set_site_app(&mut cfg, &site, Some(kind), None, None, None)?
            }
            AppCmd::Port { site, port } => {
                set_site_app(&mut cfg, &site, None, None, Some(port), None)?
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
            println!("Creating {} ({})…", bold(&name), dim(apps::label(&kind)));
            let path = create::create(&kind, &name, &parent, cfg.php.as_deref())?;
            // Outside a parked folder the new app needs a link to get its address.
            if !cfg.paths.contains(&parent) {
                link(&mut cfg, &name, path.clone())?;
            }
            println!("Created {}", path.display());
            if let Some(s) = sites(&cfg).into_iter().find(|s| s.path == path) {
                println!("Serving at {}", cyan(site_url(&cfg, &s)));
            }
        }
        Cmd::Usage { site: None } => {
            let mb = |b: u64| format!("{:.1} MB", b as f64 / 1_048_576.0);
            let (total, items) = resources::anvil_disk();
            println!("{} {}", bold("Anvil:"), mb(total));
            for i in &items {
                println!("  {} {:>10}  {}", safety(i.safety, 6), mb(i.bytes), i.label);
            }
            let all = resources::all_apps(&sites(&cfg), |_| {});
            resources::save_app_sizes(&all);
            let sum = |f: fn(&resources::AppSize) -> u64| mb(all.iter().map(f).sum());
            println!(
                "{} {} ({} safe to delete, {} rebuildable)",
                bold("Applications:"),
                sum(|a| a.total),
                sum(|a| a.safe),
                sum(|a| a.rebuild)
            );
            for a in all.iter().take(10) {
                println!(
                    "  {:>10}  {} {}",
                    mb(a.total),
                    bold(format!("{:<30}", a.name)),
                    dim(apps::label(&a.kind))
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
                bold(&s.name),
                dim(apps::label(&s.kind)),
                mb(total)
            );
            for i in items.iter().filter(|i| i.bytes > 0) {
                println!(
                    "  {} {:>10}  {:<28} {}",
                    safety(i.safety, 10),
                    mb(i.bytes),
                    i.label,
                    dim(&i.path)
                );
            }
            let mut svc = services::Services::default();
            for u in resources::services_used(&cfg, &s, &svc.pids()) {
                println!(
                    "  uses {:<12} port {:<6} {:?}{}",
                    u.label,
                    u.port.map_or("-".into(), |p| p.to_string()),
                    u.source,
                    u.detail.map_or(String::new(), |d| format!(" ({d})"))
                );
            }
        }
        Cmd::Git { site } => {
            let dir = match site {
                Some(site) => {
                    sites(&cfg)
                        .into_iter()
                        .find(|s| s.name == site_name(&site))
                        .with_context(|| format!("No app named {site}"))?
                        .path
                }
                None => dir_or_cwd(None)?,
            };
            let g = git::info(&dir).context("Not a git repository")?;
            let mut branch = g.branch;
            if let Some(u) = &g.upstream {
                branch += &format!(" (tracks {u})");
            }
            if g.ahead > 0 {
                branch += &format!(" ↑{}", g.ahead);
            }
            if g.behind > 0 {
                branch += &format!(" ↓{}", g.behind);
            }
            println!("{}   {}", dim("Branch"), bold(branch));
            match g.changed {
                0 => println!("{}  clean", dim("Changes")),
                1 => println!("{}  {}", dim("Changes"), yellow("1 changed file")),
                n => println!(
                    "{}  {}",
                    dim("Changes"),
                    yellow(format!("{n} changed files"))
                ),
            }
            if let Some(c) = g.commit {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs());
                let ago = match now.saturating_sub(c.time) {
                    s if s < 3600 => format!("{} minutes ago", s / 60),
                    s if s < 86400 => format!("{} hours ago", s / 3600),
                    s if s < 60 * 86400 => format!("{} days ago", s / 86400),
                    s if s < 730 * 86400 => format!("{} months ago", s / (30 * 86400)),
                    s => format!("{} years ago", s / (365 * 86400)),
                };
                println!(
                    "{}   {} {} {}",
                    dim("Commit"),
                    dim(c.hash),
                    c.subject,
                    dim(format!("({}, {ago})", c.author))
                );
            }
            if let Some(r) = g.remote {
                println!("{}   {}", dim("Remote"), cyan(r));
            }
        }
        Cmd::Logs {
            site,
            source,
            n,
            level,
        } => {
            let s = sites(&cfg)
                .into_iter()
                .find(|s| s.name == site_name(&site))
                .with_context(|| format!("No app named {site}"))?;
            let src = match source {
                Some(id) => logs::find(&s, &id)?,
                None => logs::sources(&s)
                    .into_iter()
                    .next()
                    .with_context(|| format!("{} has no logs yet", s.name))?,
            };
            let path = PathBuf::from(&src.path);
            let mut entries = Vec::new();
            let mut before = None;
            while entries.len() < n {
                let page = match &level {
                    Some(l) => logs::search(&path, "", Some(l), before, n - entries.len())?,
                    None => logs::read(&path, before, logs::PAGE)?,
                };
                entries.extend(page.entries);
                if !page.truncated || level.is_some() {
                    break;
                }
                before = Some(page.start_offset);
            }
            entries.truncate(n);
            for e in entries.iter().rev() {
                let mut lines = e.message.lines();
                let level = format!("{:<9}", e.level.to_uppercase());
                let level = match logs::group(&e.level) {
                    "error" => red(level),
                    "warning" => yellow(level),
                    _ => dim(level),
                };
                println!(
                    "{} {level} {}",
                    dim(e.time.as_deref().unwrap_or("-")),
                    lines.next().unwrap_or("")
                );
                for l in lines {
                    println!("    {l}");
                }
                if let Some(t) = e.trace.first() {
                    println!("    {t} ({} frames)", e.trace.len());
                }
            }
        }
        Cmd::Packages { site, updates } => {
            let named = site
                .as_deref()
                .and_then(|n| sites(&cfg).into_iter().find(|s| s.name == site_name(n)));
            let dir = match named {
                Some(s) => s.path,
                None => dir_or_cwd(site.map(PathBuf::from))?,
            };
            let p = packages::read(&dir);
            for (name, version) in &p.highlights {
                println!("{name} {version}");
            }
            let behind = if updates {
                packages::updates(&p)
            } else {
                Vec::new()
            };
            for e in &p.ecosystems {
                let dev = e.packages.iter().filter(|p| p.dev).count();
                println!(
                    "\n{} ({}): {} direct, {} dev, {} in lock",
                    bold(e.label),
                    e.manager
                        .as_deref()
                        .or(e.lock.as_deref())
                        .unwrap_or("no lock file"),
                    e.packages.len() - dev,
                    dev,
                    e.locked.len()
                );
                for (k, v) in &e.facts {
                    println!("  {k} {v}");
                }
                for p in &e.packages {
                    let latest = behind
                        .iter()
                        .find(|u| u.ecosystem == e.id && u.name == p.name)
                        .map_or(String::new(), |u| {
                            yellow(format!("  {} available ({})", u.latest, u.level))
                        });
                    println!(
                        "  {:<44} {:<16} {:<14}{}{latest}",
                        p.name,
                        p.constraint,
                        p.version.as_deref().unwrap_or("-"),
                        if p.dev { dim(" dev") } else { String::new() }
                    );
                }
            }
        }
        Cmd::Ini { site, key, value } => set_site_ini(&mut cfg, &site, &key, value.as_deref())?,
        Cmd::Https { site, off } => {
            let s = sites(&cfg)
                .into_iter()
                .find(|s| s.name == site_name(&site))
                .with_context(|| format!("No site named {site}"))?;
            set_secure(&mut cfg, &s.name, !off)?;
            let s = sites(&cfg).into_iter().find(|x| x.name == s.name).unwrap();
            println!(
                "{} {}",
                cyan(site_url(&cfg, &s)),
                commands("(restart `anvil start` to apply)")
            );
        }
        Cmd::Setup => {
            ensure_caddy()?;
            save(&cfg)?;
            print_resolver_step(&cfg);
            print_ports_step(&cfg)?;
            println!("{}", commands("Then `anvil trust` for HTTPS."));
        }
        Cmd::Tld { tld } => {
            if let Some(t) = tld {
                set_tld(&mut cfg, &t)?;
                println!(
                    "Applications now answer at {}",
                    cyan(format!("https://<name>.{}", cfg.tld))
                );
                if !setup_status(&cfg).resolver {
                    print_resolver_step(&cfg);
                }
                println!("{}", commands("Restart `anvil start` if it is running."));
            } else {
                println!("{}", cfg.tld);
            }
        }
        Cmd::Trust => {
            trust()?;
            println!("Anvil's certificate authority is now trusted.");
        }
        Cmd::Start => start(&cfg)?,
        Cmd::Uninstall { yes } => anvil::uninstall::uninstall(&cfg, config_ok, yes)?,
    }
    Ok(())
}

/// A disk item's safety, padded to `width`; what you should keep is yellow.
fn safety(s: resources::Safety, width: usize) -> String {
    let text = format!("{:<width$}", format!("{s:?}"));
    match s {
        resources::Safety::Keep => yellow(text),
        _ => text,
    }
}
