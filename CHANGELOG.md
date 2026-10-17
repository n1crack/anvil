# Changelog

All notable changes to Anvil are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Until 1.0.0, minor versions may contain breaking changes.

## [Unreleased]

### Added

- Addresses work without a port number: a setup step (asks for your password once)
  forwards ports 80 and 443 on this Mac to Anvil. `anvil uninstall` removes it.
- HTTPS per application: a lock next to each application turns it on or off
  (`anvil https <site> [--off]` in the CLI). With HTTPS on, the http:// address
  redirects to https://.
- The CLI uses color on a terminal; `NO_COLOR` turns it off.
- An application's Overview lists each running piece on its own row, with its
  own start, stop and restart buttons, its newest log line, and a click to open
  its log. "Start all" and "Stop all" act on the whole application. A process
  that crashes stays red and shows how it ended.
- Laravel 13's `php artisan dev` processes (queue, logs, Vite) are read from the
  application itself; the server is left out, since Anvil serves through PHP-FPM.
- Extra `package.json` dev servers get their own address,
  `<script>.<app>.<tld>` (for example `docs.vuefinder.test`).
- One-off scripts appear as "Commands"; their output streams into the log.
- Docker Compose projects are detected as their own kind, with their containers
  as rows. Containers keep running when Anvil quits; Stop waits for the grace
  period set in the compose file.
- A Processes window (the button next to Settings) lists everything Anvil runs
  with PID, port, CPU, memory and a live log.

### Changed

- New applications are served over plain HTTP; Settings › "HTTPS for new
  applications" changes that. Applications that existed before keep HTTPS.
- Next.js apps open at `<name>.localhost` until their `next.config` allows the
  domain suffix: in development Next.js only becomes interactive at addresses it
  trusts. The app's Overview adds `allowedDevOrigins` to `next.config` on request.
- Apps with their own dev server also answer at `<name>.localhost`.

### Fixed

- Next.js dev servers listened on every network interface; they now listen on
  127.0.0.1 only.

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
- Downloads are checked against the checksum their source publishes before
  they are unpacked or run (Node, Go, Caddy, PostgreSQL, MySQL, Redis,
  Mailpit, Composer, rustup); PHP builds and the .NET installer publish none
  and rely on HTTPS. A failed reinstall keeps the working version.
- The config file is written atomically, so the app and the CLI never read a
  half-written one.
- Git runs with `core.fsmonitor` disabled, so a repository can't make Anvil
  run a program; a folder whose path would break the web server config is not
  served.

[Unreleased]: https://github.com/n1crack/anvil/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/n1crack/anvil/releases/tag/v0.1.0
