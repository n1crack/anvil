<div align="center">

<img src="assets/icon.svg" width="128" alt="Anvil">

# Anvil

**Your tools. Your rules.**

A fast, native local development environment for macOS.<br>
Free and open source. Built for developers.

</div>

---

Anvil turns the folders on your Mac into running applications at local domains like `myapp.test`, with HTTPS, isolated language runtimes, databases and per-application settings. No containers, no virtual machines and no changes to your system packages. Everything Anvil installs lives in `~/.anvil`.

> **Status: early development.** The core works on macOS today. Expect breaking changes until 1.0.

## Why Anvil

- **Native and fast.** Runtimes and services run directly on your Mac. No VM layer between your code and the CPU.
- **Isolated.** PHP, Node.js, Go, Rust, .NET and the services live in their own folders under `~/.anvil`. Nothing leaks into your shell or your system.
- **Per-application control.** Every application can have its own PHP version, php.ini settings, start command and port.
- **Everything included.** No paid tiers, no upsells. Every feature is free.
- **Open.** Apache 2.0 licensed. Read it, change it, ship it.

## Features

| | Status |
|---|---|
| Local domains (`*.test`) via a built-in DNS server | ✅ Available |
| Local HTTPS with a trusted certificate authority | ✅ Available |
| Park a folder: every sub-folder becomes an application | ✅ Available |
| Link any folder, add extra domains to an application | ✅ Available |
| Automatic detection: Laravel, Statamic, Symfony, PHP, static, Next.js, Nuxt, Vite, Node.js, Rust, Go, .NET | ✅ Available |
| PHP 8.0 – 8.5, isolated, per-application version | ✅ Available |
| php.ini settings per application (own PHP-FPM pool) and global defaults | ✅ Available |
| Node.js, Go, Rust and .NET toolchains, isolated in `~/.anvil` | ✅ Available |
| Apps with their own dev server, proxied to their `.test` domain | ✅ Available |
| Services: MySQL 8.4, PostgreSQL 18, Redis, Mailpit | ✅ Available |
| New applications from templates | ✅ Available |
| Disk usage per application, with safe clean-up | ✅ Available |
| Native macOS app with menu bar | ✅ Available |
| Standard ports 80/443 via a privileged helper, URLs without `:8443` | 🛠 Next |
| Signed and notarized releases | 📋 Planned |
| Linux | 📋 Planned |
| Windows | 📋 Planned |

## Requirements

- macOS on Apple Silicon (Intel builds are untested)
- [Rust](https://rustup.rs) to build Anvil from source
- Xcode command line tools (`xcode-select --install`) if you want Redis, which Anvil builds from source

## Getting started

```sh
git clone <repository-url> anvil && cd anvil
cargo install --path .

anvil php install 8.4      # installs an isolated PHP; the first one becomes the default
anvil setup                # downloads the web server, prints the one-time DNS command
```

Run the command `anvil setup` prints. It needs your password once:

```sh
sudo mkdir -p /etc/resolver && printf 'nameserver 127.0.0.1\nport 5354\n' | sudo tee /etc/resolver/test
```

Then add your projects and start Anvil:

```sh
anvil park ~/Sites         # every folder in ~/Sites becomes <folder>.test
anvil start                # the first start creates the local certificate authority
anvil trust                # in another terminal: trust it for HTTPS
```

Open `https://<folder>.test:8443` in your browser.

## Applications

Anvil looks at the files in each folder to decide what kind of application it is. You can override the type in the app.

| Type | Detected by | Served by |
|---|---|---|
| Laravel | `artisan` | PHP-FPM |
| Statamic | `artisan` and `statamic/cms` in `composer.json` | PHP-FPM |
| Symfony | `bin/console` and `symfony/framework-bundle` in `composer.json` | PHP-FPM |
| PHP | `composer.json`, `index.php` or `public/index.php` | PHP-FPM |
| Rust | `Cargo.toml` | `cargo run` |
| Go | `go.mod` | `go run .` |
| .NET | a `.csproj`, `.fsproj` or `.sln` file | `dotnet run` |
| Next.js | `next` in `package.json` | `npm run dev` |
| Nuxt | `nuxt` in `package.json` | `npm run dev` |
| Vite | `vite` in `package.json` | `npm run dev` |
| Node.js | `package.json` | `npm run dev`, or `npm run start` without a `dev` script |
| Static | anything else | the web server |

- **PHP applications** are served through PHP-FPM. The web root is `public/` when it exists, otherwise the folder itself.
- **Other applications** run their own dev server on a private port. Anvil passes the port in `$PORT` (and `ASPNETCORE_URLS` for .NET) and proxies `<name>.test` to it. Node.js projects install their packages first if `node_modules` is missing. You can change the start command and the port per application.
- **A stopped application** shows a "not running" page at its domain instead of an error.
- Node.js picks the version from `.nvmrc` or `.node-version` when it is installed; otherwise the newest installed version is used.

### PHP

PHP 8.0 to 8.5 are portable builds installed into `~/.anvil/php/<version>`. Set a default version for everything and pin single applications to another one. Memory limit, upload limit, execution time and any other php.ini value can be set for all applications or for one; an application with its own settings gets its own PHP-FPM pool.

### Languages

Node.js, Go, Rust (through rustup) and .NET are installed into `~/.anvil/runtimes` and only put on `PATH` for the applications Anvil runs. Their package and build caches go to `~/.anvil/cache`, so `~/.npm`, `~/go`, `~/.cargo` and `~/.nuget` stay untouched.

### Services

Nothing runs until you add it. Add a service by picking its kind, a name and a port, and add as many of each as you need: two MySQL servers for two projects, a separate Redis for queues, a second Mailpit inbox.

| Service | Usual port |
|---|---|
| MySQL 8.4 | 3306 |
| PostgreSQL 18 | 5432 |
| Redis (built from source) | 6379 |
| Mailpit | 1025 (SMTP), web inbox on 8025 (change it with `anvil service port <name> --web <port>`) |

When the usual port is taken, by another service or another program, the next free one is suggested. Each kind's program is downloaded the first time you add one and shared after that; every service keeps its own data in `~/.anvil/data/<name>` and its own log. Services listen on 127.0.0.1 only, and turned-on services start with Anvil. MySQL starts with a `root` user without a password; PostgreSQL with a `postgres` user and trust authentication. Removing a service keeps its data unless you ask to delete it, so adding the same name again finds your databases.

### New applications

Create a project from a template with `anvil new` or the New Application sheet in the app: Laravel, Statamic, Symfony, PHP, static, Next.js, Nuxt, Vite, Node.js, Rust, Go and .NET. Anvil uses its own PHP, Composer and language toolchains, so the matching PHP version or language must be installed first. Output goes to `~/.anvil/log/create-<name>.log`, and a failed create leaves nothing behind.

### Storage

Anvil measures each application's disk use and sorts it into three groups:

- **Safe:** logs and caches. Delete any time.
- **Rebuildable:** dependencies and builds such as `node_modules`, `vendor`, `target` or `.next`. A command brings them back.
- **Keep:** your code and data, such as `storage/app` or a SQLite database. Anvil never deletes these.

In the app you can clean an application item by item, or clean the safe items of every application at once. Rebuildable items can only be deleted while the application is stopped. `anvil usage` shows the same numbers in the terminal, along with the services an application's `.env` points at.

## The app

Anvil also comes as a native macOS app with a menu bar icon. It starts Anvil, enabled services and applications set to start with Anvil when it launches.

```sh
cargo install tauri-cli --version '^2' --locked
cd app && cargo tauri build
open ../target/release/bundle/macos/Anvil.app
```

- **Applications list** with search, sorting by name or date modified, your own groups (Favorites first), a section per parked folder, linked folders, and extra linked domains under their application. Add applications with the + button: New Application, Add Folder of Applications (park) or Add Application Folder (link).
- **Application panel** with Start, Stop and Restart for apps with their own server, quick buttons for browser, Finder, Terminal, editor and log, and three tabs:
  - **Overview:** type, runtime, folder, web root, disk and services in use.
  - **Settings:** type, PHP version and php.ini settings, or start command, port and "Start with Anvil".
  - **Resources:** disk breakdown with clean-up, processes and services.
- **Settings window:**
  - **General:** site folders, launch at login, Dock icon, theme, editor and terminal, domain and ports.
  - **PHP:** install and remove versions, set the default, php.ini defaults for all applications.
  - **Languages:** install and remove Node.js, Go, Rust and .NET.
  - **Services:** add services like containers (kind, name, port), turn them on and off, change ports, open a Mailpit inbox, remove them with or without their data.
  - **Storage:** Anvil's own footprint and every application's size.
  - **Shortcuts** and **About**.
- **Menu bar menu:** status, Start/Stop, Restart, switch the default PHP version, Logs, Settings and Quit.

Closing the window keeps Anvil running in the menu bar; quit from the menu bar icon. The app applies changes right away; with the CLI, restart `anvil start` after changing sites or versions.

### Keyboard shortcuts

| Applications window | |
|---|---|
| Search applications | ⌘F |
| Move through the list | ↑ ↓ |
| Show or hide linked domains | ← → |
| Overview, Settings, Resources | ⌘1 ⌘2 ⌘3 |
| Open in browser | ⌘O |
| Open in editor | ⌘E |
| Open Terminal in the project | ⌘T |
| Show the app's log | ⌘L |
| Reload applications | ⌘R |
| Settings | ⌘, |

| Menu bar | |
|---|---|
| Use a PHP version (in the menu) | ⌘1 … ⌘9 |
| Quit Anvil | ⌘Q |

## Usage

| Command | Description |
|---|---|
| `anvil park [dir]` | Serve every sub-folder of a directory (default: current) |
| `anvil forget [dir]` | Stop serving a parked directory |
| `anvil link [name]` | Serve the current directory as `<name>.test` |
| `anvil unlink <name>` | Remove a linked site |
| `anvil sites` | List sites with their type, PHP version and web root |
| `anvil new <kind> <name> [--in <dir>]` | Create an application from a template (default location: the first parked folder) |
| `anvil php install <version>` | Install the latest release of a PHP version, e.g. `8.4` |
| `anvil php list` | List installed PHP versions |
| `anvil use <version>` | Set the default PHP version |
| `anvil use <version> --site <name>` | Set the PHP version for one site |
| `anvil ini <site> <key> [value]` | Set a php.ini value for one site; leave out the value to clear it |
| `anvil runtime list` | Installed and available Node.js, Go, Rust and .NET versions |
| `anvil runtime install <id> <version>` | e.g. `node 22`, `go 1.27.2`, `rust stable`, `dotnet 10.0` |
| `anvil runtime uninstall <id> <version>` | Remove a language version |
| `anvil app command <site> [command]` | Set an app's start command; leave it out for the default |
| `anvil app autostart <site> <true\|false>` | Start the app whenever Anvil starts |
| `anvil app runtime <site> [version]` | Pin the language version (e.g. `24` for Node); leave it out for automatic |
| `anvil service list` | List added services and their state |
| `anvil service add <kind> [name] [--port N]` | Add `mysql`, `postgres`, `redis` or `mailpit`; downloads the program the first time |
| `anvil service remove <name> [--delete-data]` | Remove a service; its data is kept unless asked |
| `anvil service uninstall <kind>` | Remove a program no service uses anymore |
| `anvil service enable <name>` / `disable <name>` | Start the service with Anvil, or not |
| `anvil service port <name> [port] [--web [port]]` | Set the port; leave it out for the usual one. `--web` sets Mailpit's inbox port (no value resets it) |
| `anvil usage [site]` | Disk usage of one application, or of Anvil and all applications |
| `anvil setup` | Download the web server and print the one-time DNS command |
| `anvil start` | Run Anvil in the foreground (Ctrl-C to stop) |
| `anvil trust` | Trust Anvil's certificate authority in your keychain |

`anvil start` starts the enabled services and the applications set to autostart.

## Configuration

Settings live in `~/.anvil/config.toml`. The app writes the same file.

```toml
tld = "test"        # domain suffix: <site>.test
http_port = 8080
https_port = 8443
dns_port = 5354
php = "8.4"         # default PHP version
editor = "PhpStorm" # app used by "Open in editor"
terminal = "iTerm"  # app used by "Open Terminal"

[ini]               # php.ini defaults for every site
memory_limit = "512M"

[sites."myapp"]
php = "8.3"         # per-site PHP version

[sites."myapp".ini]
memory_limit = "1G" # per-site php.ini, gets its own PHP-FPM pool

[sites."dashboard"]
command = "npm run dev"
port = 41500
autostart = true

[services.mysql]
kind = "mysql"
enabled = true
port = 3306

[services.queue]
kind = "redis"
enabled = true
port = 6380
```

If you change `tld` or `dns_port`, update `/etc/resolver/<tld>` to match.

## How it works

```
browser ──▶ Anvil DNS (*.test → 127.0.0.1)
        ──▶ Caddy (HTTP/HTTPS, local certificates, 127.0.0.1 only)
            ├─▶ PHP-FPM: a shared pool per PHP version, own pools for customised sites
            ├─▶ static files
            └─▶ app dev servers on private ports (Node.js, Rust, Go, .NET)
```

Anvil generates the web server and PHP-FPM configuration from your sites and supervises the processes, the app dev servers and the services. Everything is only reachable from your own machine, and dotfiles such as `.env` and `.git` are never served. If Anvil is killed, it cleans up its leftover processes on the next start.

## Data locations

| Path | Contents |
|---|---|
| `~/.anvil/config.toml` | Your settings |
| `~/.anvil/php/<version>/` | PHP binaries and `php.ini` |
| `~/.anvil/runtimes/<id>/` | Node.js, Go, Rust and .NET |
| `~/.anvil/services/<kind>/` | MySQL, PostgreSQL, Redis and Mailpit programs, shared by every service of that kind |
| `~/.anvil/data/<name>/` | Each service's data |
| `~/.anvil/data/caddy/` | Certificate authority and site certificates |
| `~/.anvil/bin/` | Web server and Composer |
| `~/.anvil/cache/` | Package and build caches for the languages and Composer |
| `~/.anvil/run/`, `~/.anvil/log/` | Generated configuration, sockets and logs |

Set `ANVIL_HOME` to use another folder instead of `~/.anvil`.

## Uninstall

1. Quit the app or stop `anvil start`.
2. `rm -rf ~/.anvil` (this also deletes your service databases).
3. `sudo rm /etc/resolver/test`
4. Delete "Caddy Local Authority" from Keychain Access.
5. Delete `Anvil.app` and run `cargo uninstall anvil`.

## Development

```sh
cargo test
cargo clippy -- -D warnings
cargo fmt
```

The core library and CLI live in `src/`, the Tauri app in `app/`.

## Built with

- [Rust](https://www.rust-lang.org)
- [Tauri](https://tauri.app) for the macOS app
- [Caddy](https://caddyserver.com) for serving and local HTTPS
- [static-php-cli](https://github.com/crazywhalecc/static-php-cli) for portable PHP builds

## License

Licensed under the [Apache License 2.0](LICENSE).
