use anvil::style::{bold, commands, cyan, dim, green, red, yellow};
use anvil::*;
use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::{Command, ExitCode};

/// `--help` colors: khaki headings, commands and flags in a soft green,
/// placeholders grey. The descriptions get an off-white in `help_text`.
const HELP_STYLES: clap::builder::Styles = {
    use clap::builder::styling::{Ansi256Color, Color, Style};
    const fn fg(c: u8) -> Style {
        Style::new().fg_color(Some(Color::Ansi256(Ansi256Color(c))))
    }
    clap::builder::Styles::styled()
        .header(fg(144).bold())
        .usage(fg(144).bold())
        .literal(fg(150))
        .placeholder(fg(245))
        .error(fg(168).bold())
        .valid(fg(150))
        .invalid(fg(180))
};

#[derive(Parser)]
#[command(version, about = "Anvil — your tools, your rules.", styles = HELP_STYLES)]
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
    /// Parallel workspaces: git worktrees of an app, each served on its own
    Worktree {
        #[command(subcommand)]
        cmd: WorktreeCmd,
    },
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
    /// Databases, cache, mail and S3 storage: mysql, postgres, redis, mailpit, versitygw, silo
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
        #[command(flatten)]
        opts: NewOpts,
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
        /// Also run each package manager's security audit (composer audit, npm/pnpm/yarn/bun audit)
        #[arg(long)]
        audit: bool,
    },
    /// Download the web server, put the terminal shims on PATH and print the
    /// one-time system setup command
    Setup,
    /// The program `php`, `composer`, `laravel`, `node`, `npm` or `npx` runs in this folder
    Which { tool: String },
    /// What an app's .anvil.toml asks for (PHP, Node.js, services) and what's missing
    /// (default: the app of the current folder); --apply installs and adds it
    Project {
        site: Option<String>,
        #[arg(long)]
        apply: bool,
        /// Accept the services' licenses without being asked
        #[arg(long)]
        accept_license: bool,
    },
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

/// The New application sheet's choices as flags. Their values and defaults come
/// from `create::options`; a flag the template doesn't offer is refused.
#[derive(clap::Args, Default)]
#[command(next_help_heading = "Template options (unset ones take the app's defaults)")]
struct NewOpts {
    /// laravel: none, react, vue, svelte or livewire
    #[arg(long, value_name = "KIT")]
    starter: Option<String>,
    /// laravel: sqlite, or a MySQL/PostgreSQL service, e.g. mysql or pgsql
    #[arg(long)]
    db: Option<String>,
    /// vite: react, vue, svelte or vanilla
    #[arg(long)]
    framework: Option<String>,
    /// laravel, statamic, next, nuxt, vite: npm, pnpm or bun
    #[arg(long, value_name = "MANAGER")]
    pm: Option<String>,
    /// laravel, statamic, symfony, php: an installed PHP version, e.g. 8.4
    #[arg(long, value_name = "VERSION")]
    php: Option<String>,
    /// next, nuxt, vite, node: an installed Node.js version, e.g. 22
    #[arg(long, value_name = "VERSION")]
    node: Option<String>,
    /// next, vite: TypeScript; --no-ts for JavaScript
    #[arg(long, overrides_with = "no_ts")]
    ts: bool,
    #[arg(long, hide = true)]
    no_ts: bool,
    /// next: Tailwind; --no-tailwind without it
    #[arg(long, overrides_with = "no_tailwind")]
    tailwind: bool,
    #[arg(long, hide = true)]
    no_tailwind: bool,
    /// Every template: a Git repository; --no-git without one
    #[arg(long, overrides_with = "no_git")]
    git: bool,
    #[arg(long, hide = true)]
    no_git: bool,
    /// Every template: HTTPS; --no-https for HTTP (default: Settings, https_default)
    #[arg(long, overrides_with = "no_https")]
    https: bool,
    #[arg(long, hide = true)]
    no_https: bool,
}

impl NewOpts {
    /// The flags given as `create::Options` for a `kind` app whose sheet offers
    /// the option keys `offered`. `create::resolve` checks the values.
    fn chosen(&self, kind: &str, offered: &[&str]) -> Result<create::Options> {
        let toggle = |on: bool, off: bool| match (on, off) {
            (true, _) => Some("on".to_string()),
            (_, true) => Some("off".to_string()),
            _ => None,
        };
        // A service id, or Laravel's name for it.
        let db = self.db.as_deref().map(|d| match d {
            "sqlite" => d.to_string(),
            d => match d.strip_prefix("service:").unwrap_or(d) {
                "pgsql" | "postgresql" => "service:postgres".into(),
                id => format!("service:{id}"),
            },
        });
        let given = [
            ("--starter", "kit", self.starter.clone()),
            ("--db", "db", db),
            ("--framework", "framework", self.framework.clone()),
            ("--pm", "js", self.pm.clone()),
            ("--php", "php", self.php.clone()),
            ("--node", "node", self.node.clone()),
            ("--ts/--no-ts", "ts", toggle(self.ts, self.no_ts)),
            (
                "--tailwind/--no-tailwind",
                "tailwind",
                toggle(self.tailwind, self.no_tailwind),
            ),
            ("--git/--no-git", "git", toggle(self.git, self.no_git)),
            (
                "--https/--no-https",
                "https",
                toggle(self.https, self.no_https),
            ),
        ];
        let mut out = create::Options::new();
        for (flag, key, value) in given {
            let Some(value) = value else { continue };
            if !offered.contains(&key) {
                // The version flags apply, but there is nothing installed to pick.
                if key == "php" && (create::needs(kind) == Some("php") || kind == "php") {
                    bail!(
                        "No PHP version is installed; add one with `anvil php install <version>`"
                    );
                }
                if key == "node" && apps::runtime(kind) == Some("node") {
                    bail!(
                        "No Node.js version is installed; add one with `anvil runtime install node <version>`"
                    );
                }
                bail!("{flag} doesn't apply to {} apps", apps::label(kind));
            }
            out.insert(key.into(), value);
        }
        Ok(out)
    }
}

#[derive(Subcommand)]
enum WorktreeCmd {
    /// List worktrees, of one app or of every app
    List { app: Option<String> },
    /// Add a worktree of the current app (or --app) next to it
    Add {
        branch: String,
        #[arg(long)]
        app: Option<String>,
        /// Create the branch, from the app's current one
        #[arg(long)]
        new: bool,
    },
    /// Remove a worktree: its folder, and with --drop-db its database copies
    Remove {
        name: String,
        #[arg(long)]
        drop_db: bool,
        /// Remove even with uncommitted changes or unpushed commits
        #[arg(long)]
        force: bool,
    },
}

#[derive(Subcommand)]
enum ServiceCmd {
    /// List added services and their state
    List,
    /// Add a service: `add redis`, `add redis cache --port 6380`. Downloads the
    /// program the first time (Redis is built from source).
    Add {
        /// mysql, postgres, redis, mailpit, versitygw or silo (both S3)
        kind: String,
        /// Defaults to the kind, then `<kind>-2`…
        name: Option<String>,
        #[arg(long)]
        port: Option<u16>,
        /// e.g. 8.0.46; `anvil service versions` lists them
        #[arg(long)]
        version: Option<String>,
        /// Accept the program's license without being asked
        #[arg(long)]
        accept_license: bool,
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
        /// The web inbox or console port (leave the value out for the default)
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
    services::migrate_files(cfg);
    let mut services = services::Services::default();
    for (id, e) in services.start_enabled(cfg) {
        eprintln!("  {id}: {}", style::error(e));
    }
    let mut server = Server::start(cfg)?;
    let mut apps = apps::Apps::default();
    for site in sites(cfg).iter().filter(|s| s.autostart) {
        // Its own database first, when it's a worktree (`worktrees::prepare_site`).
        let started = worktrees::prepare_site(site, &mut |line| eprintln!("  {line}"))
            .and_then(|()| apps.start(site, cfg));
        match started {
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
    for c in site_name_clashes(cfg) {
        eprintln!("  {}", yellow(c));
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
        sigterm(pids.last().unwrap().1);
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

/// Put the shims on PATH, then print one sudo command for every system step still
/// missing: resolver file, ports daemon and the certificate authority.
/// Ask on a terminal whether the service's license is accepted; elsewhere say how to.
fn ask_license(kind: &str, version: &str) -> Result<bool> {
    use std::io::{BufRead, IsTerminal, Write};
    let l = services::license(kind, version);
    if !std::io::stdin().is_terminal() {
        anyhow::bail!(
            "{} {version} comes under {}: read {} and add --accept-license",
            services::name(kind),
            l.name,
            l.url
        );
    }
    print!(
        "{} {version} comes under {}.\nRead it at {}\nAccept the license? [y/N] ",
        services::name(kind),
        l.name,
        cyan(l.url)
    );
    std::io::stdout().flush()?;
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer)?;
    Ok(matches!(answer.trim(), "y" | "Y" | "yes" | "Yes"))
}

fn print_setup(cfg: &Config) -> Result<()> {
    let status = setup_status(cfg);
    if !status.shims {
        println!(
            "Adding {} to PATH in {} so php, composer, laravel, node, npm and npx follow each site's versions.",
            home().join("shims").display(),
            shims::shell_rc_files(&user_home())
                .iter()
                .map(|f| f.display().to_string())
                .collect::<Vec<_>>()
                .join(" and ")
        );
    }
    shims::setup()?;
    if !status.shims {
        println!("{}\n", commands("Open a new terminal window to use them."));
    }
    let root = root_cert();
    let ports = (!status.ports)
        .then(|| stage_ports_daemon(cfg))
        .transpose()?;
    let cert = (root.exists() && !status.trusted).then_some(root.as_path());
    let script = setup_script(cfg, !status.resolver, ports.as_deref(), cert)?;
    if !script.is_empty() {
        println!(
            "{}\n\n  {}\n",
            commands("Run once so addresses, ports 80 and 443 and HTTPS work on this Mac:"),
            cyan(format!("sudo sh -c {}", shims::sh_quote(&script)))
        );
    }
    if !root.exists() {
        println!(
            "{}",
            commands(
                "Then `anvil start` once and `anvil setup` again to trust Anvil's HTTPS certificates."
            )
        );
    }
    Ok(())
}

/// `anvil service add`: the license asked for (or accepted with the flag), then
/// the program downloaded and the service enabled.
fn add_service(
    cfg: &mut Config,
    kind: &str,
    name: Option<&str>,
    port: Option<u16>,
    version: Option<&str>,
    accept_license: bool,
) -> Result<()> {
    let v = version.map_or_else(|| services::default_version(kind).to_string(), String::from);
    let accept = accept_license
        || services::license_accepted(cfg, kind, &v).is_some()
        || ask_license(kind, &v)?;
    edit(cfg, |c| services::require_license(c, kind, &v, accept))?;
    let id = edit(cfg, |c| services::add(c, kind, name, port, version))?;
    let v = services::version(cfg, &id);
    if !services::installed(kind, &v) {
        println!("Downloading {kind} {v}…");
    }
    if let Err(e) = services::ensure_installed(cfg, &id) {
        edit(cfg, |c| services::remove(c, &id, false))?;
        return Err(e);
    }
    edit(cfg, |c| services::set_enabled(c, &id, true))?;
    println!(
        "Added {} on port {}; it starts with {}.",
        bold(&id),
        services::port(cfg, &id),
        cyan("`anvil start`")
    );
    Ok(())
}

/// `anvil project`: what the app's `.anvil.toml` asks for and whether Anvil has it;
/// with `apply`, install or add what's missing.
fn project_cmd(
    cfg: &mut Config,
    site: Option<String>,
    apply: bool,
    accept_license: bool,
) -> Result<()> {
    let all = sites(cfg);
    let s = match &site {
        Some(n) => all.iter().find(|s| s.name == site_name(n)),
        None => shims::site_for(&all, &std::env::current_dir()?),
    }
    .with_context(|| match &site {
        Some(n) => format!("No app named {n}"),
        None => "This folder isn't an app in Anvil; name one: `anvil project <app>`".into(),
    })?
    .clone();
    let file = s.path.join(".anvil.toml");
    if let Some(e) = project::error(&s.path) {
        bail!("{e}");
    }
    let needs = project::needs(cfg, &s);
    if needs.is_empty() {
        println!(
            "{}",
            dim(format!(
                "{} asks for no PHP, Node.js or services.",
                file.display()
            ))
        );
        return Ok(());
    }
    println!("{}", dim(file.display()));
    let width = needs
        .iter()
        .map(|n| n.label.chars().count())
        .max()
        .unwrap_or(0);
    for n in &needs {
        let state = match (&n.overridden, n.met, n.kind.as_str()) {
            (Some(v), ..) => dim(format!("you chose {v} in Anvil")),
            (None, true, "php" | "node") => green("installed"),
            (None, true, _) => green("added"),
            (None, false, "php" | "node") => yellow("not installed"),
            (None, false, _) => yellow(format!("not added (adds {})", n.install)),
        };
        println!("  {:<width$}  {state}", n.label);
    }
    let missing: Vec<_> = needs.iter().filter(|n| n.missing()).collect();
    if missing.is_empty() || !apply {
        if !missing.is_empty() {
            println!(
                "{}",
                commands("Run `anvil project --apply` to install and add them.")
            );
        }
        return Ok(());
    }
    for n in missing {
        match n.kind.as_str() {
            "php" => php_install(cfg, &n.install)?,
            "node" => runtimes::install("node", &n.install)?,
            kind => add_service(cfg, kind, None, None, Some(&n.install), accept_license)?,
        }
    }
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

/// `anvil shim <tool> [args…]`, what the shims in ~/.anvil/shims run: the
/// arguments go through untouched, so it is handled before clap sees them.
fn shim(tool: &std::ffi::OsStr, args: impl Iterator<Item = std::ffi::OsString>) -> Result<()> {
    use std::os::unix::process::CommandExt;
    let tool = tool.to_string_lossy();
    let args: Vec<std::ffi::OsString> = args.collect();
    let words: Vec<String> = args.iter().map(|a| a.to_string_lossy().into()).collect();
    let cwd = std::env::current_dir()?;
    let (program, pre, env) = shims::resolve_run(&tool, &words, &cwd, &load()?, true)?;
    let e = Command::new(&program).args(pre).args(args).envs(env).exec();
    Err(e).with_context(|| format!("couldn't run {}", program.display()))
}

/// A config change through `anvil::update` (under the config lock, on top of what's
/// on disk now); `cfg` becomes the saved config.
fn edit<T>(cfg: &mut Config, f: impl FnOnce(&mut Config) -> Result<T>) -> Result<T> {
    let (saved, out) = update(f)?;
    *cfg = saved;
    Ok(out)
}

fn run() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    if args.next().is_some_and(|a| a == "shim") {
        let tool = args.next().context("usage: anvil shim <tool> [args…]")?;
        return shim(&tool, args);
    }
    let cmd = match Cli::try_parse() {
        Ok(cli) => cli.cmd,
        Err(e) if e.kind() == clap::error::ErrorKind::DisplayHelp && style::enabled() => {
            print!("{}", style::help_text(&e.render().ansi().to_string()));
            std::process::exit(0);
        }
        Err(e) => e.exit(),
    };
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
            edit(&mut cfg, |c| {
                if !c.paths.contains(&p) {
                    c.paths.push(p.clone());
                }
                Ok(())
            })?;
            println!("Parked {}", p.display());
        }
        Cmd::Forget { path } => {
            let p = dir_or_cwd(path)?;
            edit(&mut cfg, |c| {
                c.paths.retain(|x| x != &p);
                Ok(())
            })?;
        }
        Cmd::Link { name } => {
            let p = dir_or_cwd(None)?;
            let name = name.unwrap_or_else(|| p.file_name().unwrap().to_string_lossy().into());
            let name = edit(&mut cfg, |c| link(c, &name, p.clone()))?;
            println!(
                "Linked {} -> {}",
                cyan(format!("{name}.{}", cfg.tld)),
                p.display()
            );
        }
        Cmd::Unlink { name } => {
            edit(&mut cfg, |c| {
                c.links.remove(&name);
                Ok(())
            })?;
        }
        Cmd::Sites => {
            let all = sites(&cfg);
            // Columns as wide as their longest value, so a long kind ("Docker
            // Compose") or address doesn't push the rest out of line.
            let host = all
                .iter()
                .map(|s| s.host.chars().count() + 2 * s.worktree_of.is_some() as usize)
                .max()
                .unwrap_or(0);
            let kind = all
                .iter()
                .map(|s| apps::label(&s.kind).chars().count())
                .max()
                .unwrap_or(0);
            for s in &all {
                let indent = if s.worktree_of.is_some() { "  " } else { "" };
                println!(
                    "{:<host$} {} {} {}",
                    format!("{indent}{}", s.host),
                    dim(format!("{:<kind$}", apps::label(&s.kind))),
                    match &s.php {
                        Some(v) => green(format!("{v:<5}")),
                        None => dim(format!("{:<5}", "-")),
                    },
                    dim(s.root.display())
                );
            }
            for c in site_name_clashes(&cfg) {
                eprintln!("{}", yellow(c));
            }
        }
        Cmd::Worktree { cmd } => {
            let all = sites(&cfg);
            match cmd {
                WorktreeCmd::List { app } => {
                    let app = app.map(|a| site_name(&a));
                    for s in all.iter().filter(|s| {
                        s.worktree_of.is_some() && (app.is_none() || s.worktree_of == app)
                    }) {
                        println!(
                            "{} {} {}",
                            s.name,
                            s.branch.as_deref().unwrap_or("(detached)"),
                            dim(s.path.display())
                        );
                    }
                }
                WorktreeCmd::Add { branch, app, new } => {
                    let cwd = std::env::current_dir()?;
                    let found = match &app {
                        Some(a) => all.iter().find(|s| s.name == site_name(a)),
                        None => all.iter().find(|s| cwd.starts_with(&s.path)),
                    };
                    let app = found
                        .filter(|s| s.worktree_of.is_none())
                        .context("Name an app with --app, or run this inside one")?;
                    let folder = worktrees::add(&cfg, app, &branch, new)?;
                    println!("Added {}", folder.display());
                }
                WorktreeCmd::Remove {
                    name,
                    drop_db,
                    force,
                } => {
                    let s = all
                        .iter()
                        .find(|s| s.name == site_name(&name) && s.worktree_of.is_some())
                        .with_context(|| format!("No worktree named {name}"))?;
                    if apps::running_anywhere(&s.name) {
                        bail!("Stop {} in Anvil first", s.name);
                    }
                    // Saved after Ok and Err alike: a failed drop has removed the records of
                    // the databases that did go.
                    let result = edit(&mut cfg, |c| Ok(worktrees::remove(c, s, drop_db, force)))?;
                    result?;
                    println!("Removed {}", s.name);
                }
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
        } => edit(&mut cfg, |c| set_global_ini(c, &key, value.as_deref()))?,
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
            Some(name) => edit(&mut cfg, |c| set_site_php(c, &name, Some(version)))?,
            None => edit(&mut cfg, |c| set_default_php(c, &version))?,
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
                        (true, true) if services::runs_somewhere(&s.id) => green("running"),
                        (true, true) => "enabled".into(),
                        (true, false) => "installed".into(),
                    };
                    let inbox = s
                        .web_port
                        .map(|p| match s.kind {
                            "mailpit" => format!(", inbox on {p}"),
                            _ => format!(", console on {p}"),
                        })
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
                accept_license,
            } => add_service(
                &mut cfg,
                &kind,
                name.as_deref(),
                port,
                version.as_deref(),
                accept_license,
            )?,
            ServiceCmd::Remove { id, delete_data } => {
                edit(&mut cfg, |c| services::remove(c, &id, delete_data))?
            }
            ServiceCmd::Tag { id, tags } => edit(&mut cfg, |c| services::set_tags(c, &id, &tags))?,
            ServiceCmd::Link { id, apps } => edit(&mut cfg, |c| services::set_apps(c, &id, &apps))?,
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
                for (n, b) in c.buckets {
                    println!("{n:<32} {:>10} KB", b / 1024);
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
            ServiceCmd::Enable { id } => edit(&mut cfg, |c| services::set_enabled(c, &id, true))?,
            ServiceCmd::Disable { id } => edit(&mut cfg, |c| services::set_enabled(c, &id, false))?,
            ServiceCmd::Port { id, port, web } => {
                // With --web, the SMTP port only changes when it's given too.
                if let Some(web) = web {
                    edit(&mut cfg, |c| services::set_web_port(c, &id, web))?;
                }
                if web.is_none() || port.is_some() {
                    edit(&mut cfg, |c| services::set_port(c, &id, port))?;
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
            AppCmd::Command { site, command } => edit(&mut cfg, |c| {
                set_site_app(c, &site, None, Some(command), None, None)
            })?,
            AppCmd::Runtime { site, version } => {
                edit(&mut cfg, |c| set_site_runtime(c, &site, version))?
            }
            AppCmd::Autostart { site, on } => edit(&mut cfg, |c| {
                set_site_app(c, &site, None, None, None, Some(on))
            })?,
            AppCmd::Type { site, kind } => edit(&mut cfg, |c| {
                set_site_app(c, &site, Some(kind), None, None, None)
            })?,
            AppCmd::Port { site, port } => edit(&mut cfg, |c| {
                set_site_app(c, &site, None, None, Some(port), None)
            })?,
        },
        Cmd::New {
            kind,
            name,
            dir,
            opts,
        } => {
            if !create::TEMPLATES.contains(&kind.as_str()) {
                bail!(
                    "Unknown type {kind}; use one of: {}",
                    create::TEMPLATES.join(", ")
                );
            }
            let offered: Vec<_> = create::options(&kind, &cfg).iter().map(|o| o.key).collect();
            let opts = create::resolve(&kind, &cfg, &opts.chosen(&kind, &offered)?)?;
            let parent = match dir {
                Some(d) => d,
                None => cfg
                    .paths
                    .first()
                    .cloned()
                    .context("Park a folder first, or pass --in")?,
            };
            let parent = dir_or_cwd(Some(parent))?;
            println!("Creating {} ({})…", bold(&name), dim(apps::label(&kind)));
            let path = create::create(&kind, &name, &parent, &opts, create::LOG_SIZE)?;
            // Outside a parked folder the new app needs a link to get its address.
            if !cfg.paths.contains(&parent) {
                edit(&mut cfg, |c| link(c, &name, path.clone()))?;
            } else {
                cfg = load()?; // with the HTTPS creation chose
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
        Cmd::Packages {
            site,
            updates,
            audit,
        } => {
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
                if !audit {
                    continue;
                }
                match packages::audit(&dir, e.id, &cfg) {
                    Ok(list) if list.is_empty() => {
                        println!("  {}", green("No known vulnerabilities"))
                    }
                    Ok(list) => {
                        println!(
                            "  {}",
                            bold(format!("{} known vulnerabilities", list.len()))
                        );
                        for a in list {
                            let fixed =
                                a.fixed.map_or(String::new(), |f| format!(", fixed in {f}"));
                            println!(
                                "  {:<10} {} {}{fixed}: {}{}",
                                a.severity,
                                a.package,
                                a.version.as_deref().unwrap_or("-"),
                                a.title,
                                a.url.map_or(String::new(), |u| dim(format!(" {u}")))
                            );
                        }
                    }
                    Err(err) => println!("  {}", red(format!("{err:#}"))),
                }
            }
        }
        Cmd::Ini { site, key, value } => {
            edit(&mut cfg, |c| set_site_ini(c, &site, &key, value.as_deref()))?
        }
        Cmd::Https { site, off } => {
            let s = sites(&cfg)
                .into_iter()
                .find(|s| s.name == site_name(&site))
                .with_context(|| format!("No site named {site}"))?;
            edit(&mut cfg, |c| set_secure(c, &s.name, !off))?;
            let s = sites(&cfg).into_iter().find(|x| x.name == s.name).unwrap();
            println!(
                "{} {}",
                cyan(site_url(&cfg, &s)),
                commands("(restart `anvil start` to apply)")
            );
        }
        Cmd::Setup => {
            ensure_caddy()?;
            edit(&mut cfg, |_| Ok(()))?;
            print_setup(&cfg)?;
        }
        Cmd::Project {
            site,
            apply,
            accept_license,
        } => project_cmd(&mut cfg, site, apply, accept_license)?,
        Cmd::Which { tool } => {
            let (program, pre, _) =
                shims::resolve_with(&tool, &std::env::current_dir()?, &cfg, false)?;
            let mut line = vec![program.display().to_string()];
            line.extend(pre);
            println!("{}", line.join(" "));
        }
        Cmd::Tld { tld } => {
            if let Some(t) = tld {
                edit(&mut cfg, |c| set_tld(c, &t))?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn flags(args: &[&str]) -> NewOpts {
        let argv = ["anvil", "new", "laravel", "shop"].iter().chain(args);
        match Cli::try_parse_from(argv).unwrap().cmd {
            Cmd::New { opts, .. } => opts,
            _ => unreachable!(),
        }
    }

    #[test]
    fn worktree_subcommands_parse() {
        let parse = |a: &[&str]| match Cli::try_parse_from(["anvil", "worktree"].iter().chain(a))
            .unwrap()
            .cmd
        {
            Cmd::Worktree { cmd } => cmd,
            _ => unreachable!(),
        };
        assert!(matches!(parse(&["list"]), WorktreeCmd::List { app: None }));
        assert!(matches!(
            parse(&["add", "x", "--new", "--app", "shop"]),
            WorktreeCmd::Add {
                new: true,
                app: Some(_),
                ..
            }
        ));
        assert!(matches!(
            parse(&["remove", "shop--x", "--drop-db", "--force"]),
            WorktreeCmd::Remove {
                drop_db: true,
                force: true,
                ..
            }
        ));
    }

    const LARAVEL: &[&str] = &["kit", "db", "js", "php", "git", "https"];

    #[test]
    fn new_flags_map_onto_the_sheets_options() {
        let args = [
            "--starter",
            "react",
            "--db",
            "pgsql",
            "--pm",
            "pnpm",
            "--php",
            "8.4",
            "--no-git",
            "--https",
        ];
        let got = flags(&args).chosen("laravel", LARAVEL).unwrap();
        let want: create::Options = [
            ("kit", "react"),
            ("db", "service:postgres"),
            ("js", "pnpm"),
            ("php", "8.4"),
            ("git", "off"),
            ("https", "on"),
        ]
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .into();
        assert_eq!(got, want);
        // Unset flags leave the defaults to `create::resolve`; the last of a pair wins.
        assert!(flags(&[]).chosen("laravel", LARAVEL).unwrap().is_empty());
        let next = ["ts", "tailwind", "js", "node", "git", "https"];
        let got = flags(&["--db", "mysql-2"]).chosen("laravel", LARAVEL);
        assert_eq!(got.unwrap()["db"], "service:mysql-2");
        let got = flags(&["--ts", "--no-ts", "--no-tailwind"]).chosen("next", &next);
        let got = got.unwrap();
        assert_eq!(
            (got["ts"].as_str(), got["tailwind"].as_str()),
            ("off", "off")
        );
    }

    #[test]
    fn new_flags_that_dont_apply_are_refused() {
        let err = |args: &[&str], kind, offered| {
            flags(args).chosen(kind, offered).unwrap_err().to_string()
        };
        let next = &["ts", "tailwind", "js", "node", "git", "https"][..];
        assert_eq!(
            err(&["--starter", "react"], "next", next),
            "--starter doesn't apply to Next.js apps"
        );
        assert_eq!(
            err(&["--tailwind"], "laravel", LARAVEL),
            "--tailwind/--no-tailwind doesn't apply to Laravel apps"
        );
        assert!(err(&["--pm", "bun"], "static", &["git", "https"]).contains("--pm doesn't apply"));
        // Laravel takes a PHP version, there is just none installed.
        assert!(err(&["--php", "8.4"], "laravel", &["kit", "git"]).starts_with("No PHP version"));
        // Values are `create::resolve`'s to check, against the same list the app uses.
        let cfg = Config::default();
        let given = flags(&["--starter", "angular"])
            .chosen("laravel", LARAVEL)
            .unwrap();
        let e = create::resolve("laravel", &cfg, &given)
            .unwrap_err()
            .to_string();
        assert!(e.contains("angular isn't a choice for Starter kit; use one of: none, react"));
    }
}
