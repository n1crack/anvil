<div align="center">

<img src="assets/icon.svg" width="128" alt="Anvil">

# Anvil

**Your tools. Your rules.**

A fast, native local development environment for macOS.<br>
Free and open source. Built for developers.

</div>

---

Anvil turns the folders on your Mac into running sites at local domains like `myapp.test`, with HTTPS, isolated runtimes and per-site settings. No containers, no virtual machines and no changes to your system packages. Everything Anvil installs lives in `~/.anvil`.

> **Status: early development.** The core works on macOS today. Expect breaking changes until 1.0.

## Why Anvil

- **Native and fast.** Runtimes run directly on your Mac. No VM layer between your code and the CPU.
- **Isolated.** Each runtime version lives in its own folder under `~/.anvil`. Nothing leaks into your shell or your system.
- **Per-site control.** Every site can have its own runtime version and settings.
- **Everything included.** No paid tiers, no upsells. Every feature is free.
- **Open.** GPLv3 licensed. Read it, change it, ship it.

## Features

| | Status |
|---|---|
| Local domains (`*.test`) via a built-in DNS server | ✅ Available |
| Local HTTPS with a trusted certificate authority | ✅ Available |
| Park a folder: every sub-folder becomes a site | ✅ Available |
| Link any folder as a site | ✅ Available |
| PHP 8.0 – 8.5, isolated, per-site version | ✅ Available |
| Laravel and any PHP app with a `public/` folder | ✅ Available |
| Static sites | ✅ Available |
| Native macOS app with menu bar | 🛠 Next |
| Standard ports 80/443, URLs without `:8443` | 🛠 Next |
| Per-site PHP settings (memory, upload size, extensions) | 🛠 Next |
| Node.js versions, Next.js and other JavaScript apps | 📋 Planned |
| Services: MySQL, PostgreSQL, Redis, mail testing and more | 📋 Planned |
| Rust and Go apps | 📋 Planned |
| Linux and Windows | 📋 Planned |

## Requirements

- macOS on Apple Silicon (Intel builds are untested)
- [Rust](https://rustup.rs) to build from source

## Getting started

```sh
git clone <repository-url> anvil && cd anvil
cargo install --path .

anvil php install 8.4      # installs an isolated PHP, becomes the default
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

## Usage

| Command | Description |
|---|---|
| `anvil park [dir]` | Serve every sub-folder of a directory (default: current) |
| `anvil forget [dir]` | Stop serving a parked directory |
| `anvil link [name]` | Serve the current directory as `<name>.test` |
| `anvil unlink <name>` | Remove a linked site |
| `anvil sites` | List sites with their PHP version and web root |
| `anvil php install <version>` | Install the latest release of a PHP version, e.g. `8.4` |
| `anvil php list` | List installed PHP versions |
| `anvil use <version>` | Set the default PHP version |
| `anvil use <version> --site <name>` | Set the PHP version for one site |
| `anvil start` | Run Anvil in the foreground (Ctrl-C to stop) |
| `anvil trust` | Trust Anvil's certificate authority in your keychain |

A site's web root is its `public/` folder when one exists, otherwise the folder itself. Restart `anvil start` after changing sites or versions.

## Configuration

Settings live in `~/.anvil/config.toml`:

```toml
tld = "test"        # domain suffix: <site>.test
http_port = 8080
https_port = 8443
dns_port = 5354
php = "8.4"         # default PHP version

[sites."myapp"]
php = "8.3"         # per-site override
```

If you change `tld` or `dns_port`, update `/etc/resolver/<tld>` to match.

## How it works

```
browser ──▶ Anvil DNS (*.test → 127.0.0.1)
        ──▶ Caddy (HTTP/HTTPS, local certificates, 127.0.0.1 only)
        ──▶ PHP-FPM, one process per PHP version in use
```

Anvil generates the web server and PHP-FPM configuration from your sites and supervises the processes. Sites are only reachable from your own machine, and dotfiles such as `.env` and `.git` are never served.

| Path | Contents |
|---|---|
| `~/.anvil/config.toml` | Your settings |
| `~/.anvil/php/<version>/` | PHP binaries and `php.ini` |
| `~/.anvil/bin/` | Web server |
| `~/.anvil/data/` | Certificate authority and site certificates |
| `~/.anvil/run/`, `~/.anvil/log/` | Generated configuration, sockets and logs |

To remove Anvil completely: `rm -rf ~/.anvil`, `sudo rm /etc/resolver/test` and delete "Caddy Local Authority" from Keychain Access.

## Development

```sh
cargo test
cargo clippy -- -D warnings
cargo fmt
```

## Built with

- [Rust](https://www.rust-lang.org)
- [Caddy](https://caddyserver.com) for serving and local HTTPS
- [static-php-cli](https://github.com/crazywhalecc/static-php-cli) for portable PHP builds

## License

Licensed under the [GNU General Public License v3.0](LICENSE).
