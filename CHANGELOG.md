# Changelog

All notable changes to Anvil. Details are in the [documentation](https://anvil.tr/docs).
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and
[Semantic Versioning](https://semver.org/spec/v2.0.0.html); until 1.0.0, minor versions may
contain breaking changes.

## [Unreleased]

### Added

- PHP extensions: Xdebug, PCOV, APCu, igbinary and msgpack, per PHP version, per app or from `.anvil.toml`.
- OPcache for PHP 8.1–8.4.
- Laravel Octane with RoadRunner, FrankenPHP or Swoole (experimental).
- Requests tab: every request an app served, with headers (`anvil app requests`).
- Browser errors: an app's JavaScript errors, failed requests and page loads, for agents (`anvil app browser`).
- Plugins, off by default:
  - Mail catcher: an inbox per app and a Mail tab.
  - Database viewer: Adminer at `db.test`, opened logged in (`anvil db open`).
  - Tunnel: share an app at a temporary public address (`anvil app share`).
- Agents (MCP): requests, mail, browser errors, worktree removal, aliases; the app defaults to the agent's folder.
- Overview: Aliases, and a Mail line for apps that send to Mailpit.
- A web root of your choice per app (`anvil app root`).
- `anvil app start|stop|restart` and `anvil service start|stop` act in the running app.
- Processes › Agents: what agents and the terminal did through Anvil.

### Changed

- Sidebar: worktrees fold under their app; a lock shows HTTP or HTTPS.
- Packages tab: key packages as badges, with Check outdated and Check security.
- New worktree: a branch that doesn't exist is created.
- Removing a worktree lists the files git doesn't keep before you confirm.
- php.ini values also apply to an app's processes and the terminal.
- Lower CPU use while windows are open.

### Fixed

- Apps inside a dot folder (`~/.sites/shop`) returned 404 for their files.
- CPU and memory were missing under locales with decimal commas.
- Dev processes left by a killed Anvil kept their ports.
- Uninstalling a Node.js, Go, Rust or Python version apps are pinned to is refused.
- .NET apps no longer offer a version to pin.
- The first MySQL on a new install failed to start.

## [0.1.0] - 2026-10-19

The first public release, for macOS: a desktop app that carries the `anvil` CLI.

### Added

- `.test` domains, ports 80/443 and HTTPS per app, set up with one password prompt.
- Park a folder or link one; extra domains per app.
- Detection of Laravel, Statamic, Symfony, PHP, static, Next.js, Nuxt, Vite, Node.js, Rust, Go, .NET, Django, FastAPI, Flask and Docker Compose.
- PHP 8.1–8.5, Node.js, Go, Rust, .NET and Python, isolated in `~/.anvil` and pinned per app.
- Services: MySQL, PostgreSQL, Redis, Mailpit and S3 storage, as many as you need.
- Dev processes with live output, and Docker Compose containers.
- Git worktrees side by side, each with its own address, processes and database.
- `.anvil.toml`: a project's PHP, Node.js and services.
- New apps from templates (`anvil new`).
- Logs, Packages, Git and disk usage per app.
- Desktop app with a menu bar icon, Processes window, updates and notifications.

### Security

- Everything listens on 127.0.0.1 only; dotfiles are never served.
- Downloads are checked against their published checksums.

[Unreleased]: https://github.com/n1crack/anvil/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/n1crack/anvil/releases/tag/v0.1.0
