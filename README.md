<div align="center">

<img src="assets/icon.svg" width="128" alt="Anvil">

# Anvil

**Your tools. Your rules.**

A local development environment for PHP, Node.js, Python, Go, Rust and .NET projects.<br>
Free and open source, in beta. Runs on macOS, Windows and Linux.

</div>

---

Anvil turns the folders on your computer into running applications at local domains like `myapp.test`. It comes as a desktop app and a CLI.

<p align="center">
  <img src="https://anvil.tr/screenshots/overview.webp" width="820" alt="The Anvil app: the applications list and a Laravel app's Overview">
</p>

## What you get

- **An address per project.** Park a folder and every project in it gets a `.test` address with HTTPS from a local certificate authority (on Linux, `http://<name>.localhost:8080` for now).
- **Versions per project.** Pin PHP or Node.js per project; `php`, `composer` and `node` in your terminal follow the folder you're in.
- **Dev processes.** Queue workers, Vite and your other dev scripts run in the background, each with live output, restart and a crash alert.
- **Services.** MySQL, PostgreSQL, Redis, Mailpit and S3 storage, any number of each; Docker containers and external servers too.
- **Packages.** Key versions, updates and security advisories per app, with Install and Update.
- **Branches side by side.** A Git worktree gets its own address, processes and database while main keeps running.
- **Readable logs.** Laravel, Symfony and PHP-FPM logs as entries whose stack frames open in your editor.
- **Requests.** Every request an app served, with its headers.
- **Extras, off until you turn them on.** A Mail tab that catches each app's mail, a database viewer, a public tunnel and an MCP server for AI agents.

Everything runs natively and lives in `~/.anvil`; your system packages stay untouched. The few system changes Anvil makes are ones you approve, and `anvil uninstall` removes them.

## Install

### macOS

macOS 12 or later; 15 is recommended. MySQL needs macOS 14 and PostgreSQL 15.

With [Homebrew](https://brew.sh), add Anvil's tap once:

```sh
brew tap n1crack/anvil
```

Then install the app, which carries the `anvil` CLI:

```sh
brew install --cask anvil
```

Or only the CLI:

```sh
brew install anvil
```

Pick one: both put `anvil` on PATH. Without the tap step, use the full names: `brew install --cask n1crack/anvil/anvil` or `brew install n1crack/anvil/anvil`. The app updates itself; `brew upgrade` updates the CLI. The app is also on the [releases page](https://github.com/n1crack/anvil/releases).

### Windows

Windows 10 or 11, 64-bit. Download `Anvil_<version>_x64-setup.exe` from the [releases page](https://github.com/n1crack/anvil/releases) and run it: it installs for your account, without administrator rights, and puts `anvil` on PATH once Anvil sets up. The app updates itself.

The installer isn't code-signed yet, so SmartScreen may say "Windows protected your PC" the first time: choose More info, then Run anyway. Setting up `.test` addresses and HTTPS asks for permission once.

The CLI alone is `anvil-<version>-x86_64-pc-windows-msvc.zip`.

### Linux

x86_64 with glibc 2.35 or later: Ubuntu 22.04+, Debian 12+, Fedora, Arch. From the [releases page](https://github.com/n1crack/anvil/releases), `Anvil_<version>_amd64.AppImage` runs on any of them and updates itself; on Debian and Ubuntu, `sudo apt install ./Anvil_<version>_amd64.deb`. The app needs WebKitGTK 4.1 (the .deb brings it).

In this beta, applications answer at `http://<name>.localhost:8080`: `.test` addresses, ports 80/443 and HTTPS trust come later, and `anvil setup` says so. OPcache is built into every PHP version; other PHP extensions can't be added yet. MySQL and Redis build part of themselves the first time, which needs gcc and make.

The CLI alone is `anvil-<version>-x86_64-unknown-linux-gnu.tar.gz`.

## Quick start

```sh
anvil php install 8.4      # an isolated PHP
anvil setup                # local domains, ports 80/443, HTTPS certificate, PATH (one sudo command)
anvil park ~/Sites         # every folder in ~/Sites becomes <folder>.test
anvil start
```

Open `http://<folder>.test`. For HTTPS, `anvil https <folder>`, and `anvil setup` once more after the first start to trust the certificate.

## Documentation

Getting started, guides, the CLI reference and configuration are at [anvil.tr/docs](https://anvil.tr/docs). See also the [Changelog](CHANGELOG.md).

## Build from source

With [Rust](https://rustup.rs):

```sh
git clone https://github.com/n1crack/anvil && cd anvil
cargo install --path .
```

That installs the CLI. To build the desktop app as well, see [Build from source](https://anvil.tr/docs/installation#build-from-source).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). To report a security issue, see [SECURITY.md](SECURITY.md).

If Anvil is useful to you, you can support it on [GitHub Sponsors](https://github.com/sponsors/n1crack).

## License

[GNU General Public License v3.0](LICENSE). Built with [Rust](https://www.rust-lang.org), [Tauri](https://tauri.app), [Caddy](https://caddyserver.com) and [static-php-cli](https://github.com/crazywhalecc/static-php-cli). PHP comes from [anvil-php](https://github.com/n1crack/anvil-php), built from php.net's sources with static-php-cli.
