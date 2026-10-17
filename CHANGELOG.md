# Changelog

All notable changes to Anvil are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Until 1.0.0, minor versions may contain breaking changes.

## [Unreleased]

### Fixed

- `anvil clean` exits with an error when it refuses or fails to delete an
  item, instead of reporting success.

## [0.1.0] - 2026-10-17

The first public release of Anvil, a macOS development environment.

### Added

- Local domains: every application answers at `<name>.test` through a built-in
  DNS server and a one-time `/etc/resolver` entry.
- Local HTTPS through Caddy with a local certificate authority that `anvil trust`
  adds to your login keychain. Default ports are 8080 (HTTP) and 8443 (HTTPS).
- Parking a folder makes every sub-folder an application; linking serves any
  single folder; extra domains can be added to an application.
- Automatic detection of Laravel, Statamic, Symfony, PHP, static, Next.js, Nuxt,
  Vite, Node.js, Rust, Go and .NET projects, with a manual override per app.
- Isolated PHP 8.0 to 8.5 from portable builds, a default version and a pinned
  version per application.
- php.ini settings for all applications or for one; an application with its own
  settings gets its own PHP-FPM pool.
- Node.js, Go, Rust and .NET toolchains installed into `~/.anvil`, with their
  package and build caches kept there too. Node.js follows `.nvmrc` or
  `.node-version`, and every app can be pinned to an installed version.
- Applications with their own dev server run on a private port behind the
  proxy, with a configurable start command and port, start with Anvil, and a
  "not running" page while stopped.
- Services: MySQL (9.7, 8.4, 8.0), PostgreSQL (18 to 14), Redis (8.10, 7.4, 7.2,
  built from source) and Mailpit. Add any number of each, each with its own
  name, port, data folder, log, tags and attached applications. Services keep
  the version they were added with; removing one keeps its data unless asked.
- See inside a running service: databases with sizes (create and drop), Redis
  keys and memory, Mailpit messages.
- For each application, where the database, cache and mail server named in its
  `.env` come from: an Anvil service, Docker, another local program or a remote
  host.
- New applications from templates for every supported type (`anvil new`).
- Disk usage per application split into safe, rebuildable and keep, with
  clean-up that never touches your code or data (`anvil usage`, `anvil clean`).
- Git status per application (`anvil git`), a log viewer for Laravel, Symfony,
  app output and PHP-FPM logs (`anvil logs`), and each application's
  dependencies with locked versions and an optional update check
  (`anvil packages`).
- Native macOS app with a menu bar icon: applications and services side by
  side, an application panel with Overview, Git, Logs, Packages, Resources and
  Settings tabs, a Settings window (General, PHP, Languages, Storage, Shortcuts,
  About), keyboard shortcuts and launch at login.
- `anvil --version` prints the release version.

### Security

- The web server, the DNS server and all services listen on 127.0.0.1 only;
  apps with their own server are told to bind to 127.0.0.1.
- Dotfiles such as `.env` and `.git` are never served.
- php.ini keys and values are validated before they reach the PHP-FPM
  configuration.
- Clean-up only accepts paths Anvil knows for that kind of application and
  never follows a symlinked folder out of the project; removing a language
  version or an Anvil cache only accepts what is actually installed.
- The config file is written atomically, so the app and the CLI never read a
  half-written one.
- Git runs with `core.fsmonitor` disabled, so a repository can't make Anvil
  run a program; a folder whose path would break the web server config is not
  served.

[Unreleased]: https://github.com/OWNER/anvil/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/OWNER/anvil/releases/tag/v0.1.0
