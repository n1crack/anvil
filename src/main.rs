use anvil::style::{bold, commands, cyan, dim, green, red, yellow};
use anvil::*;
use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;

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
#[command(
    version,
    about = "Anvil — your tools, your rules.",
    styles = HELP_STYLES,
    after_help = "Examples:
  anvil php install 8.4     Install PHP 8.4
  anvil park ~/Sites        Serve every folder in ~/Sites as <folder>.test
  anvil apps                List the applications and their addresses
  anvil service add mysql   Add a MySQL service
  anvil logs blog           The latest log entries of the app blog

Run `anvil <command> --help` for a command's arguments."
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Serve every sub-folder of a directory as an application
    Park {
        /// The folder (default: the current one)
        path: Option<PathBuf>,
    },
    /// Stop serving a parked directory (its folders are left alone)
    Forget {
        /// The parked folder (default: the current one)
        path: Option<PathBuf>,
    },
    /// Serve the current directory as <name>.<tld>
    Link {
        /// The application's name (default: the folder's name)
        name: Option<String>,
    },
    /// Stop serving a linked application (its folder is left alone)
    Unlink {
        /// The name `anvil link` gave it
        name: String,
    },
    /// List the applications: address, type, PHP version and web root
    #[command(alias = "sites")]
    Apps,
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
    /// Set the default PHP version
    Use {
        /// An installed version, e.g. 8.4
        version: String,
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
    /// Settings and processes of one app
    App {
        #[command(subcommand)]
        cmd: AppCmd,
    },
    /// Create a new application: laravel, statamic, symfony, wordpress, php, static, next, nuxt, vite, node, rust, go, dotnet, django, fastapi, flask
    New {
        /// The template, e.g. laravel
        kind: String,
        /// The application's name, also its folder's
        name: String,
        /// Parent folder (default: the first parked folder)
        #[arg(long = "in")]
        dir: Option<PathBuf>,
        #[command(flatten)]
        opts: NewOpts,
    },
    /// Disk usage of an app, what is safe to delete, and the services it uses.
    /// Without an app: Anvil's own footprint and every app's size.
    Usage {
        /// The application, by its name in `anvil apps` (default: the current folder's)
        #[arg(value_name = "APP")]
        site: Option<String>,
    },
    /// Delete items `anvil usage` lists for an app (by path), or with --safe every
    /// item that is safe to delete; --safe without an app cleans every app
    Clean {
        /// The application, by its name in `anvil apps` (default: the current folder's)
        #[arg(value_name = "APP")]
        site: Option<String>,
        /// Paths `anvil usage <APP>` lists, e.g. node_modules
        paths: Vec<String>,
        /// Every item that is safe to delete
        #[arg(long)]
        safe: bool,
    },
    /// Branch, changes, last commit and remote of an app (default: current folder)
    Git {
        /// The application, by its name in `anvil apps` (default: the current folder's)
        #[arg(value_name = "APP")]
        site: Option<String>,
    },
    /// Recent log entries of an app, oldest first
    Logs {
        /// The application, by its name in `anvil apps` (e.g. blog)
        #[arg(value_name = "APP")]
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
        /// The application, by its name in `anvil apps` (default: the current folder's)
        #[arg(value_name = "APP")]
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
    /// The program `php`, `composer`, `laravel`, `forge`, `node`, `npm`, `npx` or `pnpm` runs in this folder
    Which {
        /// php, composer, laravel, forge, node, npm, npx or pnpm
        tool: String,
    },
    /// What an app's .anvil.toml asks for (PHP, Node.js, services) and what's missing
    /// (default: the app of the current folder); --apply installs and adds it
    Project {
        /// The application, by its name in `anvil apps` (default: the current folder's)
        #[arg(value_name = "APP")]
        site: Option<String>,
        /// Install and add what's missing
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
    /// Serve Anvil to AI agents (MCP, stdio)
    Mcp,
    /// The Database viewer (Adminer at db.<tld>, Settings › Features)
    Db {
        #[command(subcommand)]
        cmd: DbCmd,
    },
    /// Serve an application over HTTPS, its http:// address redirecting there; with --off,
    /// HTTP only. New applications get HTTP unless `https_default = true` in config.toml
    Https {
        /// The application, by its name in `anvil apps` (e.g. blog)
        #[arg(value_name = "APP")]
        site: String,
        /// Serve it over HTTP only
        #[arg(long)]
        off: bool,
    },
    /// Show the domain suffix applications answer at, or set it: `anvil tld localhost`
    /// needs no password or resolver file; `anvil tld test` needs one setup step
    Tld {
        /// The new suffix, e.g. test or localhost
        tld: Option<String>,
    },
    /// List what Anvil changed on this Mac (resolver files, keychain, login item,
    /// ~/.anvil); with --yes, remove it all
    Uninstall {
        /// Remove it all, without asking
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
    /// laravel: sqlite, or a MySQL/PostgreSQL service, e.g. mysql or pgsql; wordpress: a MySQL service
    #[arg(long)]
    db: Option<String>,
    /// vite: react, vue, svelte or vanilla
    #[arg(long)]
    framework: Option<String>,
    /// laravel, statamic, next, nuxt, vite: pnpm (the default), npm or bun
    #[arg(long, value_name = "MANAGER")]
    pm: Option<String>,
    /// laravel, statamic, symfony, wordpress, php: an installed PHP version, e.g. 8.4
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
    List {
        /// Only this application's worktrees
        app: Option<String>,
    },
    /// Add a worktree of the current app (or --app) next to it
    Add {
        /// The branch, e.g. feature/login
        branch: String,
        /// The application (default: the current folder's)
        #[arg(long)]
        app: Option<String>,
        /// Create the branch, from the app's current one
        #[arg(long)]
        new: bool,
        /// Its own database a copy of main's (--copy-db=false: empty); unset follows the hooks
        #[arg(
            long,
            value_name = "BOOL",
            num_args = 0..=1,
            require_equals = true,
            default_missing_value = "true"
        )]
        copy_db: Option<bool>,
    },
    /// Print an app's worktree hooks and where each field comes from
    Hooks {
        /// The application (default: the current folder's)
        app: Option<String>,
    },
    /// Use the database a worktree would get that is there already and Anvil didn't make
    UseDb {
        /// The worktree, by its name (shop--feat) or its branch (feat)
        name: String,
        /// Don't ask first
        #[arg(long)]
        yes: bool,
    },
    /// Remove a worktree: its folder, and with --drop-db its database copies
    Remove {
        /// The worktree, by its name (shop--feat) or its branch (feat)
        name: String,
        /// Also drop its database copies
        #[arg(long)]
        drop_db: bool,
        /// Remove even with uncommitted changes or unpushed commits
        #[arg(long)]
        force: bool,
        /// Don't ask before deleting what git doesn't keep (its SQLite files, an edited .env)
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum DbCmd {
    /// Print the address that opens a MySQL or PostgreSQL service (at one of its
    /// databases), or an app's database, logged in
    Open {
        /// A service or an app
        name: String,
        /// A database to open at (default: an app's own; a service opens at its list)
        database: Option<String>,
    },
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)] // parsed once from the command line
enum ServiceCmd {
    /// List added services and their state
    List,
    /// Add a service: `add redis`, `add redis cache --port 6380`. Downloads the
    /// program the first time (Redis is built from source). Also a Docker image
    /// (`add --docker mongo:7 --port 27017:27017`), a container that is there
    /// (`add --container my-db`) or a server elsewhere (`add --external db.lan:3306 --kind mysql`).
    Add {
        /// mysql, postgres, redis, mailpit, versitygw or silo (both S3); with --docker,
        /// --container or --external: the name
        kind: Option<String>,
        /// Defaults to a readable one no service has, like `redis-swift-ember`
        name: Option<String>,
        /// A free port (default: the service's usual one, or the next free); with
        /// --docker, PORT:CONTAINER_PORT
        #[arg(long)]
        port: Option<String>,
        /// e.g. 8.0.46; `anvil service versions` lists them
        #[arg(long)]
        version: Option<String>,
        /// Accept the program's license without being asked
        #[arg(long)]
        accept_license: bool,
        /// Run this Docker image, e.g. mongo:7
        #[arg(long, value_name = "IMAGE", conflicts_with_all = ["container", "external", "version"])]
        docker: Option<String>,
        /// KEY=value for the --docker container; repeat for more
        #[arg(long, requires = "docker")]
        env: Vec<String>,
        /// Add a Docker container that is there already; Anvil never deletes it
        #[arg(long, conflicts_with_all = ["external", "version"])]
        container: Option<String>,
        /// A server Anvil doesn't run, as HOST:PORT
        #[arg(long, value_name = "HOST:PORT", conflicts_with = "version")]
        external: Option<String>,
        /// What the --external server is: mysql, postgres, redis or other
        #[arg(long = "kind", requires = "external")]
        external_kind: Option<String>,
        /// The --external server's user
        #[arg(long, requires = "external")]
        user: Option<String>,
        /// The --external server's password
        #[arg(long, requires = "external")]
        password: Option<String>,
        /// The --external server's database
        #[arg(long, requires = "external")]
        database: Option<String>,
    },
    /// Versions each service offers, and which are installed
    Versions,
    /// What a running service holds: databases, keys or messages
    Databases {
        /// The service, by its name in `anvil service list`
        id: String,
    },
    /// Create a database in a running MySQL or PostgreSQL service
    CreateDb {
        /// The service, by its name in `anvil service list`
        id: String,
        /// The new database's name
        name: String,
    },
    /// Drop a database from a running MySQL or PostgreSQL service
    DropDb {
        /// The service, by its name in `anvil service list`
        id: String,
        /// The database to drop
        name: String,
    },
    /// Remove a service; its data is kept unless --delete-data
    Remove {
        /// The service, by its name in `anvil service list`
        id: String,
        /// Also delete its data
        #[arg(long)]
        delete_data: bool,
    },
    /// Set a service's tags (none clears them)
    Tag {
        /// The service, by its name in `anvil service list`
        id: String,
        /// The tags, e.g. cache queue
        tags: Vec<String>,
    },
    /// Set the applications that use a service (none clears them)
    Link {
        /// The service, by its name in `anvil service list`
        id: String,
        /// The applications, by their names in `anvil apps`
        apps: Vec<String>,
    },
    /// Remove a program version no service uses anymore, e.g. `uninstall redis 7.2.16`
    Uninstall {
        /// mysql, postgres, redis, mailpit, versitygw or silo
        kind: String,
        /// The version, as `anvil service versions` lists it
        version: String,
    },
    /// Start the service with Anvil
    Enable {
        /// The service, by its name in `anvil service list`
        id: String,
    },
    /// Don't start the service with Anvil
    Disable {
        /// The service, by its name in `anvil service list`
        id: String,
    },
    /// Start the service in the running Anvil (and with Anvil from now on)
    Start {
        /// The service, by its name in `anvil service list`
        id: String,
    },
    /// Stop the service in the running Anvil (and don't start it with Anvil)
    Stop {
        /// The service, by its name in `anvil service list`
        id: String,
    },
    /// Set the port, or reset it to the default by leaving it out
    Port {
        /// The service, by its name in `anvil service list`
        id: String,
        /// A free port (leave it out for the default)
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
    /// Install a version, e.g. `node 22`, `go 1.27.2`, `rust stable`, `dotnet 10.0`
    Install {
        /// node, go, rust, dotnet or python
        id: String,
        /// A version `anvil runtime list` offers
        version: String,
    },
    /// Remove an installed version, e.g. `node 22`
    Uninstall {
        /// node, go, rust, dotnet or python
        id: String,
        /// An installed version
        version: String,
    },
}

#[derive(Subcommand)]
enum AppCmd {
    /// Pin the app's PHP version (e.g. `8.3`), or leave it out for the default
    Php {
        /// The application, by its name in `anvil apps` (e.g. blog)
        #[arg(value_name = "APP")]
        site: String,
        /// e.g. 8.3 (leave it out for the default)
        version: Option<String>,
    },
    /// Show the app's web root, set it to a folder inside the project (`web`, `dist`,
    /// `.`), or go back to the detected one (`public/` when present) with --reset
    Root {
        /// The application, by its name in `anvil apps` (e.g. blog)
        #[arg(value_name = "APP")]
        site: String,
        /// A folder inside the project, e.g. web, dist or .
        dir: Option<String>,
        /// Go back to the detected web root
        #[arg(long, conflicts_with = "dir")]
        reset: bool,
    },
    /// Turn an extension on or off for this app only, which then gets a PHP-FPM of its own
    /// (xdebug, pcov, apcu, igbinary, msgpack, mongodb, memcached, imagick)
    Ext {
        /// The application, by its name in `anvil apps` (e.g. blog)
        #[arg(value_name = "APP")]
        site: String,
        #[arg(value_enum)]
        action: ExtAction,
        /// xdebug, pcov, apcu, igbinary, msgpack, mongodb, memcached or imagick
        name: String,
    },
    /// Set a php.ini value for the app, or clear it by leaving out the value
    Ini {
        /// The application, by its name in `anvil apps` (e.g. blog)
        #[arg(value_name = "APP")]
        site: String,
        /// A php.ini setting, e.g. memory_limit
        key: String,
        /// Its value, e.g. 512M (leave it out to clear the setting)
        value: Option<String>,
    },
    /// Set the start command (leave it out to go back to the default)
    Command {
        /// The application, by its name in `anvil apps` (e.g. blog)
        #[arg(value_name = "APP")]
        site: String,
        /// e.g. "pnpm dev" (leave it out for the default)
        command: Option<String>,
    },
    /// Pin the language version (e.g. `24` for Node), or leave it out for automatic
    Runtime {
        /// The application, by its name in `anvil apps` (e.g. blog)
        #[arg(value_name = "APP")]
        site: String,
        /// e.g. 24 (leave it out for automatic)
        version: Option<String>,
    },
    /// Start the app whenever Anvil starts
    Autostart {
        /// The application, by its name in `anvil apps` (e.g. blog)
        #[arg(value_name = "APP")]
        site: String,
        /// true or false
        #[arg(action = clap::ArgAction::Set)]
        on: bool,
    },
    /// Set the app type (e.g. `vite`), or leave it out to use the detected one
    Type {
        /// The application, by its name in `anvil apps` (e.g. blog)
        #[arg(value_name = "APP")]
        site: String,
        /// e.g. vite, next or laravel
        kind: Option<String>,
    },
    /// Pin the app's private port, or leave it out for an automatic one
    Port {
        /// The application, by its name in `anvil apps` (e.g. blog)
        #[arg(value_name = "APP")]
        site: String,
        /// A free port, e.g. 3001
        port: Option<u16>,
    },
    /// Start the app's processes, or one of them, in the running Anvil
    Start {
        /// The application, by its name in `anvil apps` (e.g. blog)
        #[arg(value_name = "APP")]
        site: String,
        /// One process, by its name in the app's Processes (default: all)
        process: Option<String>,
    },
    /// Stop the app's processes, or one of them
    Stop {
        /// The application, by its name in `anvil apps` (e.g. blog)
        #[arg(value_name = "APP")]
        site: String,
        /// One process, by its name in the app's Processes (default: all)
        process: Option<String>,
    },
    /// Restart the app's processes, or one of them
    Restart {
        /// The application, by its name in `anvil apps` (e.g. blog)
        #[arg(value_name = "APP")]
        site: String,
        /// One process, by its name in the app's Processes (default: all)
        process: Option<String>,
    },
    /// Share the app at a temporary public address (a Cloudflare quick tunnel) until Ctrl-C
    Share {
        /// The application, by its name in `anvil apps` (e.g. blog)
        #[arg(value_name = "APP")]
        site: String,
    },
    /// Switch an npm or yarn app to pnpm: pnpm-lock.yaml from its lock file, node_modules
    /// installed again; the old lock file goes only after that works. Test the app after
    Pnpm {
        /// The application, by its name in `anvil apps` (e.g. blog)
        #[arg(value_name = "APP")]
        site: String,
    },
    /// The app's latest mails, oldest first, while the mail catcher (Settings › Features)
    /// catches them; else the inbox address of the Anvil Mailpit its .env sends to
    Mail {
        /// The application, by its name in `anvil apps` (e.g. blog)
        #[arg(value_name = "APP")]
        site: String,
        /// Print one message: its headers, attachments, links and text
        #[arg(long, value_name = "ID")]
        open: Option<String>,
        /// How many mails
        #[arg(short, default_value_t = 20)]
        n: usize,
    },
    /// The latest requests Caddy served for the app, oldest first: time, method, status,
    /// duration, size and path. Static assets (Vite's, Next's, maps) only with --assets
    Requests {
        /// The application, by its name in `anvil apps` (e.g. blog)
        #[arg(value_name = "APP")]
        site: String,
        /// Only one status class
        #[arg(long, value_parser = ["1xx", "2xx", "3xx", "4xx", "5xx"])]
        status: Option<String>,
        /// Only one method, e.g. POST
        #[arg(long)]
        method: Option<String>,
        /// How many requests
        #[arg(short, default_value_t = 20)]
        n: usize,
        /// Include static assets
        #[arg(long)]
        assets: bool,
        /// Keep printing new requests as they come
        #[arg(long, short)]
        follow: bool,
    },
    /// Errors on the app's pages from your browser (JS errors, rejections, console errors,
    /// failed requests, CSP), grouped, oldest first, while its Browser errors switch is on;
    /// --on and --off turn it on (PHP apps and dev servers) and off
    Browser {
        /// The application, by its name in `anvil apps` (e.g. blog)
        #[arg(value_name = "APP")]
        site: String,
        /// How many groups
        #[arg(short, default_value_t = 20)]
        n: usize,
        /// Keep printing new events as they come
        #[arg(long, short)]
        follow: bool,
        /// Turn its Browser errors switch on
        #[arg(long, conflicts_with = "off")]
        on: bool,
        /// Turn its Browser errors switch off
        #[arg(long)]
        off: bool,
    },
}

#[derive(Subcommand)]
enum PhpCmd {
    /// Install the latest patch release of a PHP minor version, e.g. 8.4
    Install {
        /// The minor version, e.g. 8.4
        version: String,
    },
    /// Install the newest patch release of installed versions
    Update {
        /// An installed version, e.g. 8.4 (leave it out for all)
        version: Option<String>,
    },
    /// List installed versions
    List,
    /// Remove an installed version
    Uninstall {
        /// An installed version, e.g. 8.4
        version: String,
    },
    /// Set a php.ini value for every application, or clear it by leaving out the value
    Ini {
        /// A php.ini setting, e.g. memory_limit
        key: String,
        /// Its value, e.g. 512M (leave it out to clear the setting)
        value: Option<String>,
    },
    /// Build OPcache for an installed PHP 8.1–8.4
    Opcache {
        /// An installed version, e.g. 8.3
        version: String,
    },
    /// Build mysqli for an installed PHP: WordPress needs it, and Anvil's macOS PHP lacks it
    Mysqli {
        /// An installed version, e.g. 8.4
        version: String,
    },
    /// Extensions for every app on a PHP version, built on this Mac (xdebug, pcov, apcu, igbinary, msgpack, mongodb, memcached, imagick)
    Ext {
        #[command(subcommand)]
        cmd: ExtCmd,
    },
}

#[derive(clap::ValueEnum, Clone)]
enum ExtAction {
    Add,
    Remove,
}

#[derive(Subcommand)]
enum ExtCmd {
    /// Turn an extension on, building it first if needed
    Add {
        /// xdebug, pcov, apcu, igbinary, msgpack, mongodb, memcached or imagick
        name: String,
        /// The PHP version, e.g. 8.4; the default version if left out
        #[arg(long, value_name = "VERSION")]
        php: Option<String>,
    },
    /// Turn an extension off (its build is kept)
    Remove {
        /// xdebug, pcov, apcu, igbinary, msgpack, mongodb, memcached or imagick
        name: String,
        /// The PHP version, e.g. 8.4; the default version if left out
        #[arg(long, value_name = "VERSION")]
        php: Option<String>,
    },
    /// The extensions Anvil can add, and which are on
    List {
        /// The PHP version, e.g. 8.4; the default version if left out
        #[arg(long, value_name = "VERSION")]
        php: Option<String>,
    },
}

/// `anvil forget`: a folder that isn't parked is an error that lists the parked ones.
fn forget(cfg: &mut Config, p: &std::path::Path) -> Result<()> {
    if !cfg.paths.iter().any(|x| x == p) {
        let parked: Vec<_> = cfg.paths.iter().map(|x| x.display().to_string()).collect();
        match parked.is_empty() {
            true => bail!("{} isn't parked; no folder is", p.display()),
            false => bail!(
                "{} isn't parked; these are: {}",
                p.display(),
                parked.join(", ")
            ),
        }
    }
    cfg.paths.retain(|x| x != p);
    Ok(())
}

/// `anvil unlink`: the name as `anvil link` saved it (`blog.test` works too).
fn unlink(cfg: &mut Config, name: &str) -> Result<String> {
    let name = site_name(name.trim().trim_end_matches(&format!(".{}", cfg.tld)));
    if cfg.links.remove(&name).is_none() {
        bail!("No linked app named {name}; `anvil apps` lists the applications");
    }
    Ok(name)
}

fn dir_or_cwd(p: Option<PathBuf>) -> Result<PathBuf> {
    let p = p.map_or_else(std::env::current_dir, Ok)?;
    anvil::platform::canonicalize(&p).with_context(|| format!("{} not found", p.display()))
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
    // A worktree's web requests wait for its own database (`worktrees::web_env`): served
    // now that it's made.
    let now = load()?;
    if needs_restart(&now) {
        drop(server);
        server = Server::start(&now)?;
    }
    for s in sites(cfg) {
        println!(
            "  {}  {}",
            cyan(site_url(cfg, &s)),
            dim(format!("(PHP {})", s.php.as_deref().unwrap_or("-")))
        );
    }
    for c in site_name_clashes(cfg) {
        eprintln!("  {}", style::warn(c));
    }
    match &server.mail_error {
        Some(e) => eprintln!("  {}", style::error(e)),
        None if now.mail_catcher => println!(
            "  {} on {}",
            green("Mail catcher"),
            cyan(format!("127.0.0.1:{}", anvil::mail::port(&now)))
        ),
        None => {}
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
    if !cfg!(target_os = "macos") {
        println!("{}", commands(&format!("{}.", anvil::LINUX_NOT_YET)));
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
    let l = services::license(kind, version);
    let question = format!(
        "{} {version} comes under {}.\nRead it at {}\nAccept the license?",
        services::name(kind),
        l.name,
        cyan(l.url)
    );
    let elsewhere = format!("read {} and add --accept-license", l.url);
    confirm(&question, &elsewhere)
}

/// Ask `question` [y/N] on a terminal; elsewhere refuse with "<question>: <elsewhere>".
fn confirm(question: &str, elsewhere: &str) -> Result<bool> {
    use std::io::{BufRead, IsTerminal, Write};
    if !std::io::stdin().is_terminal() {
        let first = question.lines().next().unwrap_or(question);
        anyhow::bail!("{}: {elsewhere}", first.trim_end_matches(['?', '.']));
    }
    print!("{question} [y/N] ");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer)?;
    Ok(matches!(answer.trim(), "y" | "Y" | "yes" | "Yes"))
}

/// The new worktree's `after_create` commands, run when the user says yes on a terminal
/// (its database prepared first); elsewhere only listed.
fn after_create(cfg: &Config, folder: &std::path::Path) -> Result<()> {
    use std::io::IsTerminal;
    let Some(site) = worktrees::site_at(cfg, folder) else {
        return Ok(());
    };
    let commands = worktrees::after_create(cfg, &site);
    if commands.is_empty() {
        return Ok(());
    }
    let list = commands.join("; ");
    if !std::io::stdin().is_terminal() {
        println!("Not run (no terminal to ask): {list}");
        return Ok(());
    }
    if !confirm(&format!("Run {list} in {}?", site.name), "")? {
        return Ok(());
    }
    worktrees::prepare_site(&site, &mut |line| eprintln!("  {line}"))?;
    for c in &commands {
        println!("{}", bold(c));
        worktrees::run_after(cfg, &site, c, false)?;
    }
    Ok(())
}

/// `anvil worktree hooks`: each field, and the layer it comes from.
fn print_hooks(h: &worktrees::Resolved) {
    let from = |f: &str| dim(format!("({})", h.from[f]));
    let list = |v: &[String]| {
        if v.is_empty() {
            dim("none")
        } else {
            v.join(", ")
        }
    };
    let on = if h.copy_database { "on" } else { "off" };
    println!("{}", bold(format!("{} preset", h.preset)));
    for p in &h.problems {
        println!("{}", yellow(p));
    }
    println!("copy           {}  {}", list(&h.copy), from("copy"));
    println!("copy_database  {on}  {}", from("copy_database"));
    let keys: Vec<String> = h.keys.iter().map(|(k, v)| format!("{k}={v}")).collect();
    println!("keys           {}  {}", list(&keys), from("keys"));
    println!("check          {}  {}", list(&h.check), from("check"));
    println!(
        "after_create   {}  {}",
        list(&h.after_create),
        from("after_create")
    );
}

fn print_setup(cfg: &Config) -> Result<()> {
    let status = setup_status(cfg);
    if cfg!(windows) {
        return print_setup_windows(cfg, &status);
    }
    if !status.shims {
        println!(
            "Adding {} to PATH in {} so php, composer, laravel, node, npm, npx and pnpm follow each site's versions.",
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
    if !cfg!(target_os = "macos") {
        let missing = !status.resolver || !status.ports || (root.exists() && !status.trusted);
        if missing {
            println!("{}", commands(&format!("{}.", anvil::LINUX_NOT_YET)));
        }
        return Ok(());
    }
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

/// `anvil setup` on Windows: the shims on the user's PATH, then the system steps (the
/// NRPT rule, the authority trusted) at once, behind Windows' own permission prompt.
fn print_setup_windows(cfg: &Config, status: &anvil::Setup) -> Result<()> {
    if !status.shims {
        println!(
            "Adding {} to your PATH so php, composer, laravel, node, npm, npx and pnpm follow each site's versions.",
            home().join("shims").display()
        );
    }
    shims::setup()?;
    if !status.shims {
        println!("{}\n", commands("Open a new terminal window to use them."));
    }
    let root = root_cert();
    let cert = (root.exists() && !status.trusted).then_some(root.as_path());
    let script = setup_script(cfg, !status.resolver, None, cert)?;
    if !script.is_empty() {
        println!(
            "Windows asks for permission to set up .{} addresses and HTTPS…",
            cfg.tld
        );
        anvil::run_as_admin(&script, "")?;
        println!("Done.");
    }
    if !root.exists() {
        println!(
            "{}",
            commands(
                "Then `anvil start` once and `anvil setup` again to trust Anvil's HTTPS certificates."
            )
        );
    }
    if !status.git {
        println!(
            "{}",
            commands("Git isn't installed: `winget install Git.Git`, or Install Git in the app.")
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
    // Said before the license is asked for and anything downloaded.
    if let Some(why) = platform::needs_newer_macos(services::name(kind), kind, &v) {
        bail!(why);
    }
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

/// `anvil service add --docker <image>` (pulled now) or `--container <name>`.
fn add_docker(
    cfg: &mut Config,
    name: Option<&str>,
    port: Option<&str>,
    image: Option<String>,
    container: Option<String>,
    env: Vec<String>,
) -> Result<()> {
    let ports = port
        .map(|p| -> Result<(u16, u16)> {
            let (outer, inner) = p.split_once(':').unwrap_or((p, p));
            Ok((outer.parse()?, inner.parse()?))
        })
        .transpose()
        .context("Give --port as PORT or PORT:CONTAINER_PORT, e.g. 27017:27017")?;
    // A container that is there: its published port unless one is given.
    let ports = match (&container, ports) {
        (Some(c), None) => {
            let found = services::containers()?.into_iter().find(|x| &x.name == c);
            let found = found.with_context(|| format!("Docker has no container named {c}"))?;
            found.port.zip(found.container_port)
        }
        (_, p) => p,
    };
    let (outer, inner) = (ports.map(|p| p.0), ports.map(|p| p.1));
    let id = edit(cfg, |c| {
        services::add_docker(
            c,
            name,
            image.as_deref(),
            container.as_deref(),
            outer,
            inner,
            &env,
        )
    })?;
    if let Some(image) = &image {
        println!("Pulling {image}…");
        if let Err(e) = services::ensure_installed(cfg, &id) {
            edit(cfg, |c| services::remove(c, &id, false))?;
            return Err(e);
        }
        edit(cfg, |c| services::set_enabled(c, &id, true))?;
        println!(
            "Added {}; it starts with {}.",
            bold(&id),
            cyan("`anvil start`")
        );
    } else {
        println!(
            "Added {}; Anvil starts and stops it but never deletes it.",
            bold(&id)
        );
    }
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
                "{} asks for no PHP, Node.js, services or extensions.",
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
    // Turned on for the app or its version: unmet only for want of a build.
    let on = |e: &str| {
        s.ext.iter().any(|x| x == e) || s.php.as_deref().is_some_and(|v| phpext::on(cfg, v, e))
    };
    for n in &needs {
        let state = match (&n.overridden, n.met, n.kind.as_str()) {
            (Some(v), ..) => dim(format!("you chose {v} in Anvil")),
            (None, true, "php" | "node") => green("installed"),
            (None, true, "ext") => green("on"),
            (None, false, "ext") if on(&n.install) => yellow("on, not built"),
            (None, false, "ext") => yellow("not on"),
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
            "ext" => {
                *cfg = phpext::add_site(&s.name, &n.install)?;
                control::notify_changed();
            }
            kind => add_service(cfg, kind, None, None, Some(&n.install), accept_license)?,
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    // Same output as returning the Result, with `Error:` in red on a terminal.
    if let Err(e) = run() {
        eprintln!("{} {e:?}", style::error("Error:"));
        // Every lookup by name fails with one of these messages; the list is a command away.
        let msg = e.to_string();
        if msg.starts_with("No app named ") {
            eprintln!("{}", style::note("`anvil apps` lists the applications."));
        } else if msg.starts_with("No service named ") {
            eprintln!(
                "{}",
                style::note("`anvil service list` lists the services.")
            );
        }
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// `anvil shim <tool> [args…]`, what the shims in ~/.anvil/shims run: the
/// arguments go through untouched, so it is handled before clap sees them.
fn shim(tool: &std::ffi::OsStr, args: impl Iterator<Item = std::ffi::OsString>) -> Result<()> {
    let tool = tool.to_string_lossy();
    let args: Vec<std::ffi::OsString> = args.collect();
    let words: Vec<String> = args.iter().map(|a| a.to_string_lossy().into()).collect();
    let cwd = std::env::current_dir()?;
    let (program, pre, env) = shims::resolve_run(&tool, &words, &cwd, &load()?, true)?;
    let e = anvil::platform::exec(
        anvil::platform::command(&program)
            .args(pre)
            .args(args)
            .envs(env),
    );
    Err(e).with_context(|| format!("couldn't run {}", program.display()))
}

/// A config change through `anvil::update` (under the config lock, on top of what's
/// on disk now); `cfg` becomes the saved config. A running app is told, so it shows
/// the change at once.
fn edit<T>(cfg: &mut Config, f: impl FnOnce(&mut Config) -> Result<T>) -> Result<T> {
    let (saved, out) = update(f)?;
    *cfg = saved;
    control::notify_changed();
    Ok(out)
}

/// Asks the running app, which owns every process; an error when none runs.
fn ask_app(method: &str, params: serde_json::Value) -> Result<serde_json::Value> {
    control::request(method, params, "Terminal")?
        .context("Anvil isn't running. Open the Anvil app, then try again.")
}

/// `name: running` for each row `keep` picks from the app's reply.
fn print_states(rows: &serde_json::Value, label: &str, keep: impl Fn(&serde_json::Value) -> bool) {
    for r in rows.as_array().into_iter().flatten().filter(|r| keep(r)) {
        let state = if r["running"] == true {
            "running"
        } else {
            "stopped"
        };
        println!("{}: {state}", r[label].as_str().unwrap_or_default());
    }
}

/// `anvil app start|stop|restart <site> [<process>]`, done by the app.
/// `14:02:33 POST   419    12 ms   1.2 KB /login`; the host only when it isn't the app's.
fn human_size(b: u64) -> String {
    match b {
        b if b < 1024 => format!("{b} B"),
        b if b < 1_048_576 => format!("{:.1} KB", b as f64 / 1024.0),
        b => format!("{:.1} MB", b as f64 / 1_048_576.0),
    }
}

/// One caught mail: who, what, its files and links, then its text (the HTML when it has none).
fn print_mail(m: &anvil::mail::Message) {
    let s = &m.summary;
    let line = |k: &str, v: &str| {
        if !v.is_empty() {
            println!("{} {v}", dim(format!("{k:<8}")));
        }
    };
    line("From", &s.from);
    line("To", &s.to.join(", "));
    line("Cc", &m.cc.join(", "));
    line("Subject", &s.subject);
    line("Date", m.date.as_deref().unwrap_or_default());
    for a in m.attachments.iter().filter(|a| !a.inline) {
        line(
            "File",
            &format!("{} ({}, {})", a.name, a.mime, human_size(a.size as u64)),
        );
    }
    for l in &m.links {
        line("Link", l);
    }
    println!();
    match (&m.text, &m.html) {
        (Some(t), _) => println!("{}", t.trim_end()),
        (None, Some(h)) => println!("{}", h.trim_end()),
        _ => println!("{}", dim("(no text)")),
    }
}

/// One group of `anvil app browser`: when last, how many, kind, message, where, frames.
fn print_group(g: &browser::Group) {
    let at = g.last as u64;
    let kind = match g.level.as_str() {
        "error" => red(format!("{:<10}", g.kind)),
        _ => yellow(format!("{:<10}", g.kind)),
    };
    let times = if g.count > 1 {
        dim(format!(" ×{}", g.count))
    } else {
        String::new()
    };
    println!(
        "{} {kind} {}{times}  {}",
        dim(format!("{}:{:02}", platform::local_clock(at), at % 60)),
        g.message.lines().next().unwrap_or_default(),
        dim(&g.page)
    );
    if let Some(s) = &g.source {
        println!("    {}", dim(s));
    }
    for f in &g.frames {
        println!("    {}", dim(f));
    }
}

fn print_request(r: &requests::Request, host: &str) {
    let at = r.ts as u64;
    let status = format!("{:<3}", r.status);
    let status = match r.status / 100 {
        5 => red(status),
        4 => yellow(status),
        _ => status,
    };
    let ms = r.duration_ms;
    let took = if ms < 1.0 {
        format!("{ms:.1} ms")
    } else if ms < 1000.0 {
        format!("{ms:.0} ms")
    } else {
        format!("{:.2} s", ms / 1000.0)
    };
    let size = human_size(r.size);
    let own = r.host.split(':').next() == Some(host);
    println!(
        "{} {:<6} {status} {took:>8} {size:>8} {}{}",
        dim(format!("{}:{:02}", platform::local_clock(at), at % 60)),
        r.method,
        if own { String::new() } else { dim(&r.host) },
        r.uri
    );
}

/// `anvil app share`: a quick tunnel to the app until Ctrl-C or cloudflared ends.
fn share(cfg: &Config, site: &str) -> Result<()> {
    use tunnel::State;
    if !cfg.tunnel {
        bail!("{}", tunnel::TURNED_OFF);
    }
    let s = find_site(cfg, site)?;
    let (tx, rx) = std::sync::mpsc::channel();
    let stop = tx.clone();
    ctrlc::set_handler(move || {
        let _ = stop.send(());
    })?;
    eprintln!("{}", style::note("Opening a tunnel…"));
    let mut shares = tunnel::Shares::default();
    shares.start(cfg, &s, move || {
        let _ = tx.send(());
    })?;
    let _ = rx.recv();
    match shares.states().remove(&s.name) {
        Some(State::Ready { url }) => {
            println!("{}", cyan(&url));
            if let Some(note) = tunnel::note(&s) {
                eprintln!("{}", style::warn(note));
            }
            eprintln!("{}", style::note(tunnel::LIMITS));
            eprintln!("{}", style::note("Ctrl-C stops it."));
        }
        Some(State::Failed { error }) => bail!(error),
        _ => return Ok(()), // Ctrl-C while it opened; dropping `shares` stops it
    }
    loop {
        if rx.recv_timeout(std::time::Duration::from_secs(1)).is_ok() {
            return Ok(());
        }
        if let Some(State::Failed { error }) = shares.states().remove(&s.name) {
            bail!(error);
        }
    }
}

fn run_in_app(site: String, process: Option<String>, action: &str) -> Result<()> {
    let mut params = serde_json::json!({"site": site, "action": action});
    if let Some(p) = &process {
        params["process"] = p.as_str().into();
    }
    let overview = ask_app("run", params)?;
    print_states(&overview["rows"], "name", |r| {
        process.as_ref().is_none_or(|p| r["id"] == p.as_str())
    });
    Ok(())
}

/// `anvil service start|stop <id>`, done by the app.
fn service_in_app(id: String, action: &str) -> Result<()> {
    let rows = ask_app("service", serde_json::json!({"id": id, "action": action}))?;
    print_states(&rows, "id", |r| r["id"] == id.as_str());
    Ok(())
}

fn run() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    // A Windows shim is this program named `php.exe`, `npm.exe`…
    if cfg!(windows)
        && let Some(tool) = std::env::current_exe()
            .ok()
            .as_deref()
            .and_then(shims::shim_name)
    {
        return shim(std::ffi::OsStr::new(tool), std::env::args_os().skip(1));
    }
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
            // A parked folder that was deleted can still be forgotten.
            let p = match dir_or_cwd(path.clone()) {
                Ok(p) => p,
                Err(_) => std::path::absolute(path.unwrap_or_default())?,
            };
            edit(&mut cfg, |c| forget(c, &p))?;
            println!("Forgot {}", p.display());
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
            let name = edit(&mut cfg, |c| unlink(c, &name))?;
            println!("Unlinked {name}");
        }
        Cmd::Apps => {
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
            // On stderr, so a script reading the list gets nothing for none.
            if all.is_empty() {
                eprintln!(
                    "{}",
                    style::note(
                        "No applications yet: `anvil park` a folder of projects, or `anvil link` one."
                    )
                );
            }
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
                eprintln!("{}", style::warn(c));
            }
        }
        Cmd::Worktree { cmd } => {
            let all = sites(&cfg);
            match cmd {
                WorktreeCmd::List { app } => {
                    let app = app.map(|a| site_name(&a));
                    let list: Vec<_> = all
                        .iter()
                        .filter(|s| {
                            s.worktree_of.is_some() && (app.is_none() || s.worktree_of == app)
                        })
                        .collect();
                    if list.is_empty() {
                        eprintln!(
                            "{}",
                            style::note(
                                "No worktrees yet: `anvil worktree add <branch>` in an app."
                            )
                        );
                    }
                    for s in list {
                        println!(
                            "{} {} {}",
                            s.name,
                            s.branch.as_deref().unwrap_or("(detached)"),
                            dim(s.path.display())
                        );
                    }
                }
                WorktreeCmd::Add {
                    branch,
                    app,
                    new,
                    copy_db,
                } => {
                    let cwd = std::env::current_dir()?;
                    let found = match &app {
                        Some(a) => all.iter().find(|s| s.name == site_name(a)),
                        None => all.iter().find(|s| cwd.starts_with(&s.path)),
                    };
                    let app = found
                        .filter(|s| s.worktree_of.is_none())
                        .context("Name an app with --app, or run this inside one")?;
                    let (folder, choice) = worktrees::add(&cfg, app, &branch, new, copy_db)?;
                    edit(&mut cfg, |c| worktrees::set_db_at(c, &folder, choice))?;
                    println!("Added {}", folder.display());
                    after_create(&cfg, &folder)?;
                }
                WorktreeCmd::Hooks { app } => {
                    let cwd = std::env::current_dir()?;
                    let found = match &app {
                        Some(a) => all.iter().find(|s| s.name == site_name(a)),
                        None => all
                            .iter()
                            .filter(|s| cwd.starts_with(&s.path))
                            .max_by_key(|s| s.path.as_os_str().len()),
                    };
                    let s = found.context("Name an app, or run this inside one")?;
                    print_hooks(&worktrees::hooks(&cfg, s));
                }
                WorktreeCmd::UseDb { name, yes } => {
                    let s = find_worktree(&all, &name)?;
                    let db = worktrees::db_name(s);
                    let question = format!(
                        "Use {db} for {}? Anvil didn't make it; its migrations and tests then change it.",
                        s.name
                    );
                    if !yes && !confirm(&question, "add --yes to use it")? {
                        bail!("Nothing changed");
                    }
                    edit(&mut cfg, |c| worktrees::adopt(c, s))?;
                    println!("{} uses {db}", s.name);
                }
                WorktreeCmd::Remove {
                    name,
                    drop_db,
                    force,
                    yes,
                } => {
                    let s = find_worktree(&all, &name)?;
                    if apps::running_anywhere(&s.name) {
                        bail!("Stop {} in Anvil first", s.name);
                    }
                    // Uncommitted work refuses first: no question for a remove that won't go.
                    worktrees::stop_if_clean(s, force, || ())?;
                    let losses = worktrees::removal_losses(&cfg, s);
                    if !yes && !losses.is_empty() {
                        let question = format!(
                            "These will be deleted with the folder: {}.\nRemove {}?",
                            losses.join(", "),
                            s.name
                        );
                        if !confirm(&question, "add --yes to remove them too")? {
                            bail!("Nothing changed");
                        }
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
            cmd: PhpCmd::Update { version },
        } => {
            if let Some(v) = &version
                && !installed_php().contains(v)
            {
                bail!("PHP {v} is not installed — run `anvil php install {v}`");
            }
            let found: Vec<_> = php_updates()?
                .into_iter()
                .filter(|(minor, ..)| version.as_ref().is_none_or(|v| v == minor))
                .collect();
            if found.is_empty() {
                println!(
                    "PHP {}is up to date",
                    version.map(|v| v + " ").unwrap_or_default()
                );
            }
            for (minor, ..) in &found {
                php_install(&mut cfg, minor)?;
            }
            if !found.is_empty() {
                println!(
                    "{}",
                    commands(
                        "Restart Anvil to use it: `anvil start`, or the app reloads on its next refresh."
                    )
                );
            }
        }
        Cmd::Php {
            cmd: PhpCmd::Uninstall { version },
        } => php_uninstall(&cfg, &version)?,
        Cmd::Php {
            cmd: PhpCmd::Ini { key, value },
        } => {
            edit(&mut cfg, |c| set_global_ini(c, &key, value.as_deref()))?;
            changed("Every app", &key, value.as_deref(), "cleared");
        }
        Cmd::Php {
            cmd: PhpCmd::Opcache { version },
        } => {
            if !installed_php().contains(&version) {
                bail!("PHP {version} is not installed — run `anvil php install {version}`");
            }
            if opcache::state(&version) == opcache::State::BuiltIn {
                bail!("PHP {version} has OPcache built in");
            }
            opcache::build(&version)?;
            println!(
                "{}",
                commands(
                    "Restart Anvil to use it: `anvil start`, or the app reloads on its next refresh."
                )
            );
        }
        Cmd::Php {
            cmd: PhpCmd::Mysqli { version },
        } => {
            if !installed_php().contains(&version) {
                bail!("PHP {version} is not installed — run `anvil php install {version}`");
            }
            if opcache::has_mysqli(&version) {
                bail!("PHP {version} has mysqli already");
            }
            opcache::build_mysqli(&version)?;
            println!(
                "{}",
                commands(
                    "Restart Anvil to use it: `anvil start`, or the app reloads on its next refresh."
                )
            );
        }
        Cmd::Php {
            cmd: PhpCmd::Ext { cmd },
        } => {
            let (ExtCmd::Add { php, .. } | ExtCmd::Remove { php, .. } | ExtCmd::List { php }) =
                &cmd;
            let v = php
                .clone()
                .or_else(|| cfg.php.clone())
                .context("No PHP version yet — run `anvil php install 8.4`")?;
            match &cmd {
                ExtCmd::Add { name, .. } => drop(phpext::add(&v, name)?),
                ExtCmd::Remove { name, .. } => drop(phpext::remove(&v, name)?),
                ExtCmd::List { .. } => {}
            }
            if let ExtCmd::List { .. } = cmd {
                for e in phpext::offered() {
                    let state = match (phpext::on(&cfg, &v, e.name), phpext::built(&v, e.name)) {
                        (true, true) => green("on"),
                        (true, false) => {
                            yellow(format!("on, not built (anvil php ext add {})", e.name))
                        }
                        (false, _) => dim(phpext::needs(e.name).unwrap_or("off")),
                    };
                    println!("{:<9} {:<10} {state}", e.name, e.version);
                }
                // Apps on this version with extensions of their own.
                let built = phpext::built_names(&v);
                for s in sites(&cfg)
                    .into_iter()
                    .filter(|s| s.php.as_deref() == Some(v.as_str()) && !s.ext.is_empty())
                {
                    let list: Vec<String> = s
                        .ext
                        .iter()
                        .map(|e| {
                            if built.contains(&e.as_str()) {
                                e.clone()
                            } else {
                                format!("{e} (not built: anvil app ext {} add {e})", s.name)
                            }
                        })
                        .collect();
                    println!("{} {}", dim(format!("{}:", s.name)), list.join(", "));
                }
            } else {
                control::notify_changed();
                println!(
                    "{}",
                    commands(
                        "Restart Anvil to use it: `anvil start`, or Stop and Start in the app."
                    )
                );
            }
        }
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
            let s = find_site(&cfg, &site)?;
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
            let all = installed_php();
            if all.is_empty() {
                eprintln!(
                    "{}",
                    style::note("No PHP yet: `anvil php install 8.4` installs one.")
                );
            }
            // Offline, the list goes without it.
            let updates = php_updates().unwrap_or_default();
            for v in all {
                let mark = if cfg.php.as_deref() == Some(&v) {
                    dim(" (default)")
                } else {
                    String::new()
                };
                let opcache = match opcache::state(&v) {
                    opcache::State::Ready => "opcache".into(),
                    opcache::State::BuiltIn => "opcache (built in)".into(),
                    opcache::State::Missing => dim(format!("no opcache (anvil php opcache {v})")),
                };
                let patch = match updates.iter().find(|(m, ..)| *m == v) {
                    Some((_, _, new)) => format!("  {new} available (anvil php update {v})"),
                    None => String::new(),
                };
                let full = opcache::installed_full(&v).unwrap_or_else(|_| v.clone());
                println!("{full}  {opcache}{mark}{patch}");
            }
        }
        Cmd::Use { version } => {
            edit(&mut cfg, |c| set_default_php(c, &version))?;
            phpext::build_missing(&cfg, &version);
            control::notify_changed();
            println!("Default PHP: {version}");
        }
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
                    let state = match (s.kind, s.installed, s.enabled) {
                        (services::DOCKER, ..) => match (&s.problem, s.running) {
                            (Some(why), _) => dim(why),
                            (None, true) => green("running"),
                            (None, false) => "stopped".into(),
                        },
                        (services::EXTERNAL, ..) if s.running => green("answers"),
                        (services::EXTERNAL, ..) => dim("not answering"),
                        (_, false, _) => dim("not installed"),
                        (_, true, true) if services::runs_somewhere(&s.id) => green("running"),
                        (_, true, true) => "enabled".into(),
                        (_, true, false) => "installed".into(),
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
                        // An external one's host; a Docker one's image.
                        s.host.as_deref().unwrap_or(&s.version),
                        // A container that publishes none.
                        match s.port {
                            0 => "-".to_string(),
                            p => p.to_string(),
                        }
                    );
                }
            }
            ServiceCmd::Add {
                kind,
                name,
                port,
                version,
                accept_license,
                docker,
                env,
                container,
                external,
                external_kind,
                user,
                password,
                database,
            } => {
                if docker.is_none() && container.is_none() && external.is_none() {
                    let kind = kind.context(
                        "Name the kind: mysql, postgres, redis, mailpit, versitygw or silo",
                    )?;
                    let port = port
                        .map(|p| p.parse::<u16>())
                        .transpose()
                        .context("--port takes a port number")?;
                    add_service(
                        &mut cfg,
                        &kind,
                        name.as_deref(),
                        port,
                        version.as_deref(),
                        accept_license,
                    )?
                } else if name.is_some() {
                    bail!("Give the name once, e.g. `anvil service add --docker mongo:7 mongo`");
                } else if let Some(at) = external {
                    let (host, p) = at
                        .rsplit_once(':')
                        .and_then(|(h, p)| Some((h, p.parse::<u16>().ok()?)))
                        .context("Give --external as HOST:PORT, e.g. db.lan:3306")?;
                    let ext = services::External {
                        kind: external_kind.unwrap_or_else(|| "other".into()),
                        host: host.into(),
                        user,
                        password,
                        database,
                    };
                    let id = edit(&mut cfg, |c| {
                        services::add_external(c, kind.as_deref(), ext, p)
                    })?;
                    println!("Added {} at {at}", bold(&id));
                } else {
                    add_docker(
                        &mut cfg,
                        kind.as_deref(),
                        port.as_deref(),
                        docker,
                        container,
                        env,
                    )?
                }
            }
            ServiceCmd::Remove { id, delete_data } => {
                // An adopted container or a server elsewhere: only forgotten.
                let forgotten = cfg.services.get(&id).is_some_and(|s| {
                    s.external.is_some() || s.docker.as_ref().is_some_and(|d| d.image.is_none())
                });
                services::drop_container(&cfg, &id, delete_data)?;
                edit(&mut cfg, |c| services::remove(c, &id, delete_data))?;
                match (forgotten, delete_data) {
                    (true, _) => println!("Removed {id} from Anvil; it is left as it is"),
                    (false, true) => println!("Removed {id} and its data"),
                    (false, false) => println!("Removed {id}; its data is kept"),
                }
            }
            ServiceCmd::Tag { id, tags } => {
                edit(&mut cfg, |c| services::set_tags(c, &id, &tags))?;
                let list = tags.join(", ");
                changed(&id, "tags", (!list.is_empty()).then_some(&list), "none");
            }
            ServiceCmd::Link { id, apps } => {
                edit(&mut cfg, |c| services::set_apps(c, &id, &apps))?;
                let list = apps.join(", ");
                changed(
                    &id,
                    "applications",
                    (!list.is_empty()).then_some(&list),
                    "none",
                );
            }
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
            ServiceCmd::Enable { id } => {
                edit(&mut cfg, |c| services::set_enabled(c, &id, true))?;
                println!("{id} starts with Anvil");
            }
            ServiceCmd::Disable { id } => {
                edit(&mut cfg, |c| services::set_enabled(c, &id, false))?;
                println!("{id} doesn't start with Anvil");
            }
            ServiceCmd::Start { id } => service_in_app(id, "start")?,
            ServiceCmd::Stop { id } => service_in_app(id, "stop")?,
            ServiceCmd::Port { id, port, web } => {
                // With --web, the SMTP port only changes when it's given too.
                if let Some(web) = web {
                    edit(&mut cfg, |c| services::set_web_port(c, &id, web))?;
                    changed(
                        &id,
                        "web port",
                        web.map(|p| p.to_string()).as_deref(),
                        "default",
                    );
                }
                if web.is_none() || port.is_some() {
                    edit(&mut cfg, |c| services::set_port(c, &id, port))?;
                    changed(
                        &id,
                        "port",
                        port.map(|p| p.to_string()).as_deref(),
                        "default",
                    );
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
            RuntimeCmd::Uninstall { id, version } => runtime_uninstall(&cfg, &id, &version)?,
        },
        Cmd::App { cmd } => match cmd {
            AppCmd::Php { site, version } => {
                edit(&mut cfg, |c| set_site_php(c, &site, version.clone()))?;
                // Its own extensions, built for the version it's on now; the app then
                // restarts PHP-FPM with them (`needs_restart`).
                phpext::build_missing_for(&cfg, &site);
                control::notify_changed();
                changed(&site, "PHP", version.as_deref(), "default");
            }
            AppCmd::Root {
                site,
                dir: None,
                reset: false,
            } => {
                let s = find_site(&cfg, &site)?;
                has_web_root(&s)?;
                match &s.root_custom {
                    Some(dir) => println!("{dir} {}", dim(format!("({})", s.root.display()))),
                    None => println!(
                        "{} {}",
                        s.root_detected,
                        dim(format!("(detected, {})", s.root.display()))
                    ),
                }
            }
            AppCmd::Root { site, dir, .. } => {
                edit(&mut cfg, |c| set_site_root(c, &site, dir.as_deref()))?;
                changed(&site, "web root", dir.as_deref(), "detected");
            }
            AppCmd::Ext { site, action, name } => {
                match action {
                    ExtAction::Add => drop(phpext::add_site(&site, &name)?),
                    ExtAction::Remove => drop(phpext::remove_site(&site, &name)?),
                }
                control::notify_changed();
                println!(
                    "{}",
                    commands(
                        "Restart Anvil to use it: `anvil start`, or Stop and Start in the app."
                    )
                );
            }
            AppCmd::Ini { site, key, value } => {
                edit(&mut cfg, |c| set_site_ini(c, &site, &key, value.as_deref()))?;
                changed(&site, &key, value.as_deref(), "cleared");
            }
            AppCmd::Command { site, command } => {
                edit(&mut cfg, |c| {
                    set_site_app(c, &site, None, Some(command.clone()), None, None)
                })?;
                changed(&site, "start command", command.as_deref(), "default");
            }
            AppCmd::Runtime { site, version } => {
                edit(&mut cfg, |c| set_site_runtime(c, &site, version.clone()))?;
                changed(&site, "runtime", version.as_deref(), "automatic");
            }
            AppCmd::Pnpm { site } => {
                let s = find_site(&cfg, &site)?;
                let lock = anvil::packages::switch_to_pnpm(&cfg, &s)?;
                println!("{} uses pnpm now; {lock} is gone. Test the app.", s.name);
            }
            AppCmd::Autostart { site, on } => {
                edit(&mut cfg, |c| {
                    set_site_app(c, &site, None, None, None, Some(on))
                })?;
                match on {
                    true => println!("{site} starts with Anvil"),
                    false => println!("{site} doesn't start with Anvil"),
                }
            }
            AppCmd::Type { site, kind } => {
                edit(&mut cfg, |c| {
                    set_site_app(c, &site, Some(kind.clone()), None, None, None)
                })?;
                changed(&site, "type", kind.as_deref(), "detected");
            }
            AppCmd::Port { site, port } => {
                edit(&mut cfg, |c| {
                    set_site_app(c, &site, None, None, Some(port), None)
                })?;
                changed(
                    &site,
                    "port",
                    port.map(|p| p.to_string()).as_deref(),
                    "automatic",
                );
            }
            AppCmd::Start { site, process } => run_in_app(site, process, "start")?,
            AppCmd::Stop { site, process } => run_in_app(site, process, "stop")?,
            AppCmd::Restart { site, process } => run_in_app(site, process, "restart")?,
            AppCmd::Share { site } => share(&cfg, &site)?,
            AppCmd::Mail { site, open, n } => {
                let s = find_site(&cfg, &site)?;
                if anvil::mail::caught(&cfg, &s.name) {
                    let dir = anvil::mail::root().join(&s.name);
                    match open {
                        Some(id) => print_mail(&anvil::mail::read(&dir, &id)?),
                        None => {
                            let all = anvil::mail::list(&dir);
                            if all.is_empty() {
                                println!("{}", dim(format!("No mail from {} yet", s.name)));
                            }
                            for m in all.iter().take(n).rev() {
                                println!(
                                    "{} {}  {}  {}  {}",
                                    dim(format!(
                                        "{}:{:02}",
                                        platform::local_clock(m.ts / 1000),
                                        m.ts / 1000 % 60
                                    )),
                                    cyan(&m.id),
                                    m.to.join(", "),
                                    bold(if m.subject.is_empty() {
                                        "(no subject)"
                                    } else {
                                        &m.subject
                                    }),
                                    dim(human_size(m.size)),
                                );
                            }
                        }
                    }
                    return Ok(());
                }
                if open.is_some() {
                    bail!(
                        "Anvil doesn't catch {}'s mail: turn on Settings › Features › Mail catcher",
                        s.name
                    );
                }
                let id = resources::mailpit_for(&cfg, &s.path).with_context(|| {
                    format!(
                        "{}'s .env doesn't send mail to an Anvil Mailpit (MAIL_MAILER=smtp, MAIL_HOST=127.0.0.1, MAIL_PORT=its SMTP port)",
                        s.name
                    )
                })?;
                if !services::runs_somewhere(&id) {
                    bail!("{id} isn't running: anvil service start {id}");
                }
                let port = services::web_port(&cfg, &id).unwrap_or(8025);
                println!("http://127.0.0.1:{port}");
            }
            AppCmd::Requests {
                site,
                status,
                method,
                n,
                assets,
                follow,
            } => {
                if !cfg.request_log {
                    bail!("{}", requests::TURNED_OFF);
                }
                let s = find_site(&cfg, &site)?;
                let filter = requests::Filter {
                    status,
                    method,
                    assets,
                    ..Default::default()
                };
                let path = requests::file(&s.name);
                let page = requests::read(&path, &filter, None, n)?;
                for r in page.requests.iter().rev() {
                    print_request(r, &s.host);
                }
                let mut from = page.end_offset;
                if follow {
                    loop {
                        std::thread::sleep(std::time::Duration::from_secs(1));
                        let t = requests::tail(&path, from, &filter)?;
                        for r in t.requests.iter().rev() {
                            print_request(r, &s.host);
                        }
                        from = t.end_offset;
                    }
                }
            }
            AppCmd::Browser {
                site,
                n,
                follow,
                on,
                off,
            } => {
                if on || off {
                    edit(&mut cfg, |c| browser::set(c, &site, on))?;
                }
                let s = find_site(&cfg, &site)?;
                if on || off {
                    println!(
                        "Browser errors {} for {}{}",
                        if on { "on" } else { "off" },
                        s.name,
                        if on {
                            " (the running Anvil restarts its web server)"
                        } else {
                            ""
                        }
                    );
                    return Ok(());
                }
                if !browser::on(&s) {
                    bail!(
                        "{}'s Browser errors switch is off: anvil app browser {} --on",
                        s.name,
                        s.name
                    );
                }
                if browser::blocked(&s.name) {
                    println!(
                        "{}",
                        yellow(format!(
                            "{}'s Content-Security-Policy keeps Anvil's script out of its pages",
                            s.name
                        ))
                    );
                }
                let path = browser::file(&s.name);
                let r = browser::read(&path, 0)?;
                if r.groups.is_empty() {
                    println!("{}", dim(format!("No errors from {}'s pages yet", s.name)));
                }
                for g in r.groups.iter().rev().take(n).rev() {
                    print_group(g);
                }
                let mut from = r.end;
                if follow {
                    loop {
                        std::thread::sleep(std::time::Duration::from_secs(1));
                        let t = browser::read(&path, from)?;
                        for g in &t.groups {
                            print_group(g);
                        }
                        from = t.end;
                    }
                }
            }
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
            // A running app serves it now, not once its window next looks.
            control::notify_changed();
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
            let s = find_site(&cfg, &site)?;
            let (total, items) = resources::disk(&s);
            let mb = |b: u64| format!("{:.1} MB", b as f64 / 1_048_576.0);
            println!(
                "{} ({}): {} on disk",
                bold(&s.name),
                dim(apps::label(&s.kind)),
                mb(total)
            );
            for i in items.iter().filter(|i| i.bytes > 0) {
                let (safe, size) = (safety(i.safety, 10), mb(i.bytes));
                match i.path.is_empty() {
                    true => println!("  {safe} {size:>10}  {}", i.label),
                    false => println!("  {safe} {size:>10}  {:<28} {}", i.label, dim(&i.path)),
                }
            }
            let mut svc = services::Services::default();
            for u in resources::services_used(&cfg, &s, &svc.pids()) {
                println!(
                    "  uses {:<12} port {:<6} {}{}",
                    u.label,
                    u.port.map_or("-".into(), |p| p.to_string()),
                    match u.source {
                        resources::Source::Anvil => "Anvil",
                        resources::Source::Docker => "Docker",
                        resources::Source::Local => "local",
                        resources::Source::Remote => "remote",
                        resources::Source::None if u.port.is_some() => "not running",
                        resources::Source::None => "",
                    },
                    u.detail.map_or(String::new(), |d| format!(" ({d})"))
                );
            }
        }
        Cmd::Git { site } => {
            let dir = match site {
                Some(site) => find_site(&cfg, &site)?.path,
                None => dir_or_cwd(None)?,
            };
            let g = git::info(&dir)
                .with_context(|| format!("{} isn't a Git repository", dir.display()))?;
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
                let ago = ago(now.saturating_sub(c.time));
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
            let s = find_site(&cfg, &site)?;
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
            if p.ecosystems.is_empty() {
                eprintln!(
                    "{}",
                    style::note(format!(
                        "No dependencies in {}: no composer.json, package.json, Cargo.toml, go.mod or .csproj",
                        dir.display()
                    ))
                );
            }
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
        Cmd::Https { site, off } => {
            let s = find_site(&cfg, &site)?;
            if !apps::serves(&s.kind) {
                bail!(
                    "{} is a {} app, which Anvil doesn't serve, so it has no HTTPS",
                    s.name,
                    apps::label(&s.kind)
                );
            }
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
                // A new site's scheme, with the port when it isn't the standard one.
                let (scheme, port, standard) = if cfg.https_default == Some(true) {
                    ("https", anvil::public_https_port(&cfg), 443)
                } else {
                    ("http", anvil::public_http_port(&cfg), 80)
                };
                let port = if port == standard {
                    String::new()
                } else {
                    format!(":{port}")
                };
                println!(
                    "Applications now answer at {}",
                    cyan(format!("{scheme}://<name>.{}{port}", cfg.tld))
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
        Cmd::Start => {
            // One owner: the app already runs Caddy, PHP-FPM and the services.
            if matches!(
                control::request("ping", serde_json::json!({}), "Terminal"),
                Ok(Some(_))
            ) {
                bail!("Anvil is running and already serves your apps");
            }
            start(&cfg)?
        }
        Cmd::Mcp => {
            if !cfg.mcp {
                eprintln!("{}", mcp::TURNED_OFF);
                std::process::exit(1);
            }
            let mut client = "Agent".to_string();
            mcp::serve(
                std::io::stdin().lock(),
                std::io::stdout().lock(),
                &mut client,
            )?
        }
        Cmd::Db {
            cmd: DbCmd::Open { name, database },
        } => println!(
            "{}",
            anvil::dbviewer::url(&cfg, &name, database.as_deref())?
        ),
        Cmd::Uninstall { yes } => anvil::uninstall::uninstall(&cfg, config_ok, yes)?,
    }
    Ok(())
}

/// What a setting command did: `blog: port 3901`, or `blog: port automatic` when unset.
fn changed(subject: &str, what: &str, value: Option<&str>, unset: &str) {
    println!("{subject}: {what} {}", value.unwrap_or(unset));
}

/// `secs` ago in words: "just now", "1 hour ago", "3 days ago".
fn ago(secs: u64) -> String {
    let (n, unit) = match secs {
        s if s < 60 => return "just now".into(),
        s if s < 3600 => (s / 60, "minute"),
        s if s < 86400 => (s / 3600, "hour"),
        s if s < 60 * 86400 => (s / 86400, "day"),
        s if s < 730 * 86400 => (s / (30 * 86400), "month"),
        s => (s / (365 * 86400), "year"),
    };
    format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" })
}

/// A disk item's safety, padded to `width`; what you should keep is yellow.
fn safety(s: resources::Safety, width: usize) -> String {
    let text = format!("{:<width$}", format!("{s:?}"));
    match s {
        resources::Safety::Keep => yellow(text),
        _ => text,
    }
}

/// A worktree by its name (`shop--feat`) or by its branch (`feat`, what `worktree add`
/// took): of the app of the current folder first, else the only one with that branch.
fn find_worktree<'a>(all: &'a [Site], name: &str) -> Result<&'a Site> {
    let trees = || all.iter().filter(|s| s.worktree_of.is_some());
    if let Some(s) = trees().find(|s| s.name == site_name(name)) {
        return Ok(s);
    }
    let by_branch: Vec<&Site> = trees()
        .filter(|s| s.branch.as_deref() == Some(name))
        .collect();
    let cwd = std::env::current_dir().unwrap_or_default();
    let here = all
        .iter()
        .filter(|s| cwd.starts_with(&s.path))
        .max_by_key(|s| s.path.as_os_str().len())
        .map(|s| s.worktree_of.clone().unwrap_or_else(|| s.name.clone()));
    if let Some(s) = by_branch
        .iter()
        .find(|s| here.as_deref() == s.worktree_of.as_deref())
    {
        return Ok(s);
    }
    match by_branch.as_slice() {
        [s] => Ok(s),
        [] => bail!("No worktree named {name}"),
        many => bail!(
            "Several worktrees are on {name}: {}",
            many.iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ago_reads_as_words() {
        assert_eq!(ago(5), "just now");
        assert_eq!(ago(60), "1 minute ago");
        assert_eq!(ago(7200), "2 hours ago");
        assert_eq!(ago(86400), "1 day ago");
        assert_eq!(ago(400 * 86400), "13 months ago");
        assert_eq!(ago(800 * 86400), "2 years ago");
    }

    #[test]
    fn forget_and_unlink_refuse_what_isnt_there() {
        let mut cfg = Config {
            paths: vec![PathBuf::from("/sites")],
            ..Config::default()
        };
        cfg.links.insert("blog".into(), PathBuf::from("/blog"));
        let e = forget(&mut cfg, std::path::Path::new("/nope")).unwrap_err();
        assert_eq!(e.to_string(), "/nope isn't parked; these are: /sites");
        forget(&mut cfg, std::path::Path::new("/sites")).unwrap();
        assert!(cfg.paths.is_empty());
        let e = forget(&mut cfg, std::path::Path::new("/sites")).unwrap_err();
        assert_eq!(e.to_string(), "/sites isn't parked; no folder is");

        let tld = cfg.tld.clone();
        assert!(unlink(&mut cfg, "ghost").is_err());
        assert_eq!(unlink(&mut cfg, &format!("blog.{tld}")).unwrap(), "blog");
        assert!(cfg.links.is_empty());
    }

    fn flags(args: &[&str]) -> NewOpts {
        let argv = ["anvil", "new", "laravel", "shop"].iter().chain(args);
        match Cli::try_parse_from(argv).unwrap().cmd {
            Cmd::New { opts, .. } => opts,
            _ => unreachable!(),
        }
    }

    #[test]
    fn apps_keeps_sites_as_an_alias() {
        for name in ["apps", "sites"] {
            let cmd = Cli::try_parse_from(["anvil", name]).unwrap().cmd;
            assert!(matches!(cmd, Cmd::Apps));
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
            parse(&["add", "x", "--copy-db"]),
            WorktreeCmd::Add {
                copy_db: Some(true),
                ..
            }
        ));
        assert!(matches!(
            parse(&["add", "x", "--copy-db=false"]),
            WorktreeCmd::Add {
                copy_db: Some(false),
                ..
            }
        ));
        assert!(matches!(
            parse(&["add", "x"]),
            WorktreeCmd::Add { copy_db: None, .. }
        ));
        // `--db` (copy|empty) is gone: the checkbox's flag replaces it.
        assert!(Cli::try_parse_from(["anvil", "worktree", "add", "x", "--db", "copy"]).is_err());
        assert!(matches!(
            parse(&["hooks", "shop"]),
            WorktreeCmd::Hooks { app: Some(_) }
        ));
        assert!(matches!(
            parse(&["use-db", "shop--x", "--yes"]),
            WorktreeCmd::UseDb { yes: true, .. }
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
