# CLI reference

Every command also has built-in help: `anvil --help`, `anvil <command> --help`,
`anvil <command> <subcommand> --help`. `anvil --version` prints the version.

Sites are named after their folder (or the name you linked them with) and answer
at `<name>.test`. With the CLI, restart `anvil start` after changing sites or
versions; the macOS app applies changes right away.

## Sites

| Command | Description |
|---|---|
| `anvil park [path]` | Serve every sub-folder of a directory (default: current) |
| `anvil forget [path]` | Stop serving a parked directory |
| `anvil link [name]` | Serve the current directory as `<name>.test` |
| `anvil unlink <name>` | Remove a linked site |
| `anvil sites` | List sites with their type, PHP version and web root |
| `anvil new <kind> <name> [--in <dir>]` | Create an application from a template: `laravel`, `statamic`, `symfony`, `php`, `static`, `next`, `nuxt`, `vite`, `node`, `rust`, `go`, `dotnet`. Default location: the first parked folder |

## PHP

| Command | Description |
|---|---|
| `anvil php install <version>` | Install the latest patch release of a PHP minor version, e.g. `8.4` |
| `anvil php list` | List installed PHP versions |
| `anvil php uninstall <version>` | Remove an installed PHP version |
| `anvil php ini <key> [value]` | Set a php.ini value for every site; leave out the value to clear it |
| `anvil use <version>` | Set the default PHP version |
| `anvil use <version> --site <name>` | Set the PHP version for one site |
| `anvil ini <site> <key> [value]` | Set a php.ini value for one site; leave out the value to clear it |

## Languages and apps with their own server

| Command | Description |
|---|---|
| `anvil runtime list` | Installed and available Node.js, Go, Rust and .NET versions |
| `anvil runtime install <id> <version>` | e.g. `node 22`, `go 1.27.2`, `rust stable`, `dotnet 10.0` |
| `anvil runtime uninstall <id> <version>` | Remove an installed version, e.g. `node 22` |
| `anvil app command <site> [command]` | Set an app's start command; leave it out for the default |
| `anvil app runtime <site> [version]` | Pin the language version (e.g. `24` for Node); leave it out for automatic |
| `anvil app autostart <site> <true\|false>` | Start the app whenever Anvil starts |
| `anvil app type <site> [kind]` | Set the app type (e.g. `vite`), or leave it out to use the detected one |
| `anvil app port <site> [port]` | Pin the app's private port, or leave it out for an automatic one |

## Services

See [services.md](services.md) for what each service offers.

| Command | Description |
|---|---|
| `anvil service list` | List added services and their state |
| `anvil service add <kind> [name] [--port <port>] [--version <version>]` | Add `mysql`, `postgres`, `redis` or `mailpit`; downloads that version the first time (Redis is built from source) |
| `anvil service versions` | Versions each service offers, and which are installed |
| `anvil service databases <name>` | What a running service holds: databases and sizes, Redis keys, Mailpit messages |
| `anvil service create-db <name> <db>` | Create a database in a running MySQL or PostgreSQL service |
| `anvil service drop-db <name> <db>` | Drop a database from a running MySQL or PostgreSQL service |
| `anvil service remove <name> [--delete-data]` | Remove a service; its data is kept unless `--delete-data` |
| `anvil service tag <name> [tags…]` | Set a service's tags, e.g. `queue`; no tags clears them |
| `anvil service link <name> [apps…]` | Set the applications that use a service; none clears them |
| `anvil service uninstall <kind> <version>` | Remove a program version no service uses anymore, e.g. `redis 7.2.16` |
| `anvil service enable <name>` | Start the service with Anvil |
| `anvil service disable <name>` | Don't start the service with Anvil |
| `anvil service port <name> [port] [--web [port]]` | Set the port; leave it out for the usual one. `--web` sets Mailpit's inbox port (no value resets it) |

## Inspecting applications

| Command | Description |
|---|---|
| `anvil git [site]` | Branch, changes, last commit and remote of an app (default: current folder) |
| `anvil logs <site> [--source <file>] [-n <count>] [--level <level>]` | Recent log entries, oldest first. `--source` takes a file name like `laravel.log`, `app` or `php-fpm` (default: the newest); `-n` defaults to 20; `--level` is `error`, `warning`, `info` or `debug` |
| `anvil packages [site] [--updates]` | Dependencies and locked versions of an app, or of a folder (default: current); `--updates` asks the registries for newer ones |
| `anvil usage [site]` | Disk usage of an app, what is safe to delete, and the services it uses. Without an app: Anvil's own footprint and every app's size |
| `anvil clean [site] [paths…] [--safe]` | Delete items `anvil usage` lists for an app, or with `--safe` every safe-to-delete item; `anvil clean --safe` does it for every app. Rebuildable items wait until the app is stopped |

## Running Anvil

| Command | Description |
|---|---|
| `anvil setup` | Download the web server and print the one-time DNS command |
| `anvil start` | Run DNS, PHP-FPM, the web server, enabled services and autostart apps in the foreground (Ctrl-C to stop) |
| `anvil trust` | Trust Anvil's local HTTPS certificate authority in your login keychain |
| `anvil uninstall [--yes]` | List what Anvil changed on this Mac: `/etc/resolver` files it wrote, its certificate authority in the keychain, the login item, the app's `~/Library` folders and `~/.anvil`. With `--yes`, remove them (quit Anvil first) |
