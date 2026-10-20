<div align="center">

<img src="assets/icon.svg" width="128" alt="Anvil">

# Anvil

**Your tools. Your rules.**

A local development environment for web projects.<br>
Free and open source. Runs on macOS (Apple Silicon); Linux and Windows are in development.

</div>

---

Anvil turns the folders on your computer into running applications at local domains like `myapp.test`. PHP, Node.js, Go, Rust, .NET, Python, MySQL, PostgreSQL, Redis, Mailpit and S3 storage (Versity S3 Gateway or SILO) run natively, isolated in `~/.anvil`, without touching your system packages. If a project brings its own Docker Compose file, Anvil shows and controls its containers too. The few system changes it makes are ones you approve, and `anvil uninstall` removes them.

It comes as a desktop app and a CLI.

## Install

### macOS (Apple Silicon)

```sh
brew install --cask n1crack/anvil/anvil   # the app, which carries the anvil CLI
brew install n1crack/anvil/anvil          # or only the CLI
```

Pick one: both put `anvil` on PATH. The app is also on the [releases page](https://github.com/n1crack/anvil/releases), and updates itself.

### Linux and Windows

In development.

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

That installs the CLI. To build the desktop app as well, see [Build from source](https://anvil.tr/docs/getting-started#build-from-source).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). To report a security issue, see [SECURITY.md](SECURITY.md).

If Anvil is useful to you, you can support it on [GitHub Sponsors](https://github.com/sponsors/n1crack).

## License

[GNU General Public License v3.0](LICENSE). Built with [Rust](https://www.rust-lang.org), [Tauri](https://tauri.app), [Caddy](https://caddyserver.com) and [static-php-cli](https://github.com/crazywhalecc/static-php-cli).
