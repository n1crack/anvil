<div align="center">

<img src="assets/icon.svg" width="128" alt="Anvil">

# Anvil

A local development environment for PHP, Node.js, Python, Go, Rust and .NET projects.<br>
Desktop app and CLI for macOS, Windows and Linux. Free and open source, in beta.

</div>

---

Each project runs at its own address, such as `shop.test`, with its own language version and its own database.

<p align="center">
  <img src="https://anvil.tr/screenshots/overview.webp" width="820" alt="The Anvil app: the applications list and a Laravel app's Overview">
</p>

## What you get

- **An address per project.** Park a folder and every project in it gets a `.test` address. HTTPS is a switch per project.
- **Versions per project.** PHP, Node.js, Go and Python can be pinned per project. `php`, `composer` and `node` in your terminal follow the folder you're in.
- **Dev processes.** Queue workers, Vite and your other scripts run in the background, with live output and a crash alert.
- **Services.** MySQL, PostgreSQL, Redis, Mailpit and S3 storage; Docker containers too.
- **Branches side by side.** A Git worktree gets its own address, processes and database.
- **Per-project tabs.** Logs, requests, packages and caught mail.

Anvil keeps its files in `~/.anvil`. Outside it, `anvil setup` asks for your password, then sends `.test` lookups to Anvil, forwards ports 80/443 to it, trusts its local certificate and puts its commands on your shell's PATH. `anvil uninstall` lists everything first, then removes it.

## Install

**macOS** (12 or later):

```sh
brew install --cask n1crack/anvil/anvil
```

**Windows and Linux:** download the installer from the [releases page](https://github.com/n1crack/anvil/releases).

See the [installation guide](https://anvil.tr/docs/installation) for the CLI alone, Linux's beta limits and other options.

## Quick start

```sh
anvil php install 8.4   # an isolated PHP
anvil setup             # .test lookups, ports 80/443, PATH (asks for your password)
anvil park ~/Sites      # every folder in ~/Sites becomes <folder>.test
anvil start
```

Open `http://<folder>.test`. For HTTPS, run `anvil https <folder>`. The certificate exists after the first start, so run `anvil setup` once more to trust it.

## More

- [Documentation](https://anvil.tr/docs) and the [Changelog](CHANGELOG.md)
- [Contributing](CONTRIBUTING.md) and [Security](SECURITY.md)
- [GitHub Sponsors](https://github.com/sponsors/n1crack), if Anvil is useful to you

## License

[GNU General Public License v3.0](LICENSE). Built with [Rust](https://www.rust-lang.org), [Tauri](https://tauri.app), [Caddy](https://caddyserver.com) and [static-php-cli](https://github.com/crazywhalecc/static-php-cli). PHP comes from [anvil-php](https://github.com/n1crack/anvil-php).
