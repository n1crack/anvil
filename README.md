<div align="center">

<img src="assets/icon.svg" width="128" alt="Anvil">

# Anvil

**Your tools. Your rules.**

A fast, native local development environment for macOS.<br>
Free and open source. Built for developers.

</div>

---

Anvil turns the folders on your Mac into running applications at local domains like `myapp.test`. PHP, Node.js, Go, Rust, .NET, Python, MySQL, PostgreSQL, Redis and Mailpit run natively, isolated in `~/.anvil`: no containers, no virtual machines, no changes to your system packages. The few system changes it makes are ones you approve, and `anvil uninstall` removes them.

> **Status: early development.** Expect breaking changes until 1.0.

- Local domains, per-site HTTPS, standard ports
- Laravel, Statamic, Symfony, PHP, static, Next.js, Nuxt, Vite, Node.js, Rust, Go, .NET, Django, FastAPI and Flask detected automatically
- PHP 8.0 – 8.5 and php.ini settings per application
- Services, logs, dependencies, Git status and disk clean-up per application
- A native macOS app and a CLI

## Install

Anvil runs on macOS on Apple Silicon. Build it from source with [Rust](https://rustup.rs):

```sh
git clone https://github.com/n1crack/anvil && cd anvil
cargo install --path .
```

## Quick start

```sh
anvil php install 8.4      # an isolated PHP
anvil setup                # prints the one-time commands for local domains and ports
anvil park ~/Sites         # every folder in ~/Sites becomes <folder>.test
anvil start
```

Open `http://<folder>.test`. For HTTPS, `anvil https <folder>` and `anvil trust`.

## Documentation

Guides, the CLI reference and configuration are at [anvil.tr/docs](https://anvil.tr/docs). See also the [Changelog](CHANGELOG.md) and [Code signing](SIGNING.md).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). To report a security issue, see [SECURITY.md](SECURITY.md).

If Anvil is useful to you, you can support it on [GitHub Sponsors](https://github.com/sponsors/n1crack).

## License

[GNU General Public License v3.0](LICENSE). Built with [Rust](https://www.rust-lang.org), [Tauri](https://tauri.app), [Caddy](https://caddyserver.com) and [static-php-cli](https://github.com/crazywhalecc/static-php-cli).
