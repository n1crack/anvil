# System requirements

## Platform

| | Status |
|---|---|
| macOS on Apple silicon | Supported |
| macOS on Intel | Builds exist for every download, but untested |
| Linux | The `anvil` CLI builds and its tests pass in CI; not supported yet (no MySQL, no `anvil trust`, no app) |
| Windows | Not supported |

### macOS version

Anvil has no tested minimum macOS version yet. What the binaries
themselves ask for, read with `otool -l` on one Apple silicon Mac:

| Part | Lowest macOS it runs on |
|---|---|
| `anvil` and `Anvil.app` | 11 |
| PHP 8.4 and 8.5, Caddy 2.11.7 | 12 |
| Mailpit 1.31.6, Go 1.27 | 13 |
| Node.js 24 | 13.5 |
| MySQL 8.4.11 | 14 |
| PostgreSQL 18.6 | 15 |

Other versions, and the Intel builds, may differ. MySQL downloads are the
builds Oracle names `macos15`. The app bundle declares macOS 10.13 (the Tauri
default), which is lower than what it actually needs.

## Tools Anvil uses

Anvil runs these programs. All except the last group come with macOS.

| Tool | Used for |
|---|---|
| `curl`, `tar` | Downloading and unpacking everything Anvil installs |
| `openssl` | Checking downloads against their published or pinned checksums |
| `security` | `anvil trust` and checking whether HTTPS is trusted |
| `osascript` | The app's one-click resolver setup (password prompt) |
| `lsof`, `ps`, `du`, `kill` | Processes, ports and disk use per application |
| `make`, `cc` | Building Redis, which has no macOS downloads |
| `git` | Git status per application; without it the Git info stays empty |

`make`, `cc` and `git` need the Xcode command line tools:

```sh
xcode-select --install
```

Templates for new applications use the matching language (`npx` for Next.js,
Nuxt and Vite, `cargo`, `go`, `dotnet`, PHP with Composer). Docker is optional:
if it is installed, Anvil shows which container holds a port.

## Disk space

Everything goes into `~/.anvil`. Sizes measured on one Apple silicon Mac:

| What | Size |
|---|---|
| Caddy (web server) | 48 MB |
| One PHP version | 78–85 MB |
| Composer | 3.5 MB |
| MySQL 8.4 | 640 MB |
| PostgreSQL 18 | 43 MB |
| Redis 8 (after building) | 3.5 MB |
| Mailpit | 25 MB |
| One Node.js version | 190–200 MB |
| Go 1.27 | 270 MB |
| Rust (minimal toolchain) | 460 MB |
| .NET SDK | 650 MB |

Only what you install is downloaded. Downloads are unpacked in
`~/.anvil/build` and removed afterwards, so leave room for the archive as well.
Service data and package caches grow with use; `anvil usage` shows them.

## Network

Downloads come from these hosts, useful if a firewall or proxy needs them:

| Host | For |
|---|---|
| `github.com`, `release-assets.githubusercontent.com`, `api.github.com` | Caddy, PostgreSQL, Mailpit and their checksums |
| `dl.static-php.dev` | PHP |
| `getcomposer.org` | Composer |
| `cdn.mysql.com` | MySQL |
| `download.redis.io`, `raw.githubusercontent.com` | Redis source and checksums |
| `nodejs.org` | Node.js |
| `go.dev`, `dl.google.com` | Go |
| `static.rust-lang.org` | Rust (rustup) |
| `builds.dotnet.microsoft.com` | .NET SDK and its release list |

`anvil packages --updates` also asks `repo.packagist.org`,
`registry.npmjs.org`, `crates.io`, `proxy.golang.org` and `api.nuget.org` for
newer dependency versions. New applications from templates fetch their
packages from the language's own registry.

## Ports

Anvil's servers listen on `127.0.0.1` only (Caddy also on `::1`), and apps
with their own dev server are told to do the same. These are the defaults; the first
three are set in [configuration.md](configuration.md), service ports per
service.

| Port | What |
|---|---|
| 8443 | HTTPS for `*.test` |
| 8080 | HTTP for `*.test` |
| 5354 (UDP) | DNS for `*.test` |
| 3306 | MySQL |
| 5432 | PostgreSQL |
| 6379 | Redis |
| 1025, 8025 | Mailpit SMTP and web inbox |
| 41000–44999 | Apps with their own dev server, one port each |

PHP-FPM uses sockets in `~/.anvil/run`, not ports. When a service port is
taken, Anvil suggests the next free one. None of these ports need admin
rights.

## Passwords and permissions

Anvil runs as your user. Two one-time steps ask for your password; both are
optional.

| Step | What it does | Without it |
|---|---|---|
| Resolver file (`sudo` in the terminal, or a macOS prompt from the app) | Creates `/etc/resolver/test` so `*.test` resolves to Anvil | `*.test` addresses don't open in the browser |
| `anvil trust` | Adds the local certificate authority to your login keychain | HTTPS works, but the browser warns about the certificate |

See [Getting started](getting-started.md#resolver-setup).

## Building from source

- macOS with the Xcode command line tools
- [Rust](https://rustup.rs), stable; Anvil uses the 2024 edition, so Rust 1.85
  or newer
- For the app: `cargo install tauri-cli --version '^2' --locked`
- For contributing: Node.js, to run `node --check` on the UI scripts

See [CONTRIBUTING.md](../CONTRIBUTING.md).
