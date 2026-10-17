# Changelog

All notable changes to Anvil are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Until 1.0.0, minor versions may contain breaking changes.

## [Unreleased]

### Added

- Addresses work without a port number: `anvil setup` (one password prompt)
  forwards ports 80 and 443 on this Mac to Anvil, trusts the HTTPS certificate
  and puts the terminal commands on PATH. `anvil uninstall` removes it.
- HTTPS per application: right-click › HTTPS in the app, `anvil https <site>
  [--off]` in the CLI. With HTTPS on, the http:// address redirects to https://.
- A Dev tab (⌘3) runs an app's dev processes (for Laravel the queue, log tailer
  and Vite from `php artisan dev`, otherwise package.json dev servers) and
  shows their live, colored output, one at a time or all merged. The toolbar
  has Run ▾ (a package.json or composer.json script, after a confirmation),
  Configure and Start/Stop. Keys: ↑↓ select, s start, x stop, r restart, q stop
  all, c clear; ⌘-click opens a link in the output.
- Configure edits an app's processes as a table: command, "Dev start", "With
  Anvil", processes of your own, and "Reset to detected". It is stored in
  `~/.anvil/config.toml`; the project's files are never changed. A project's
  own `.anvil.toml` is read when present.
- Extra package.json dev servers get their own address,
  `<script>.<app>.<tld>` (for example `docs.vuefinder.test`).
- Docker Compose projects are detected as their own kind. A Containers tab
  shows each container's state and live log, with Start, Stop and Restart.
  Stop never removes containers.
- A Processes window (the pulse icon in the toolbar) lists everything Anvil
  runs in three tabs, Apps, Services and Anvil, with port, PID, CPU, memory and
  a live, colored log.
- New applications show their setup live: the steps tick off over the colored
  output, and Open selects the new app. Options per template: Laravel's
  starter kit, database and package manager, Next.js's TypeScript and Tailwind,
  the PHP or Node.js version and Git.
- Settings lists each language (PHP, Node.js, Go, Rust, .NET, Python) on its
  own page with its versions. The Node.js page shows which npm, pnpm, yarn and
  bun apps and your terminal get, and installs a missing one into Anvil's
  folder.
- Log retention: Anvil's own logs stay under 5 MB. Settings › Storage › "Trim
  app logs weekly" (on by default) cuts an app's `storage/logs/*.log` over
  10 MB to its last 1 MB and deletes daily logs older than 14 days.
- The CLI uses color on a terminal; `NO_COLOR` turns it off.

### Changed

- The Overview is a summary: one "Dev · 3 of 4 running" (or "Containers")
  line that opens the tab, then where the app runs and its folders. The header
  shows the state; Start and Stop live in the Dev tab.
- A process's output shows only its current run: it is cleared when the
  process starts and when Anvil starts.
- Processes Anvil starts get color (`FORCE_COLOR`, `--ansi` for artisan) and
  the PATH your terminal has, so Homebrew tools such as pnpm are found.
- A dev process or script that needs `node_modules` installs it first, with
  the package manager the lock file names.
- The Logs tab follows new entries, groups repeated entries (×24), shows the
  app's own stack frames first with vendor frames folded, opens a frame in
  your editor, and shows plain-text logs as a terminal.
- The Packages tab has one tab per package manager, shows updates inline
  (11.36.1 → 12.4.0) and checks for them only when asked.
- New applications are served over plain HTTP; Settings › "HTTPS for new
  applications" changes that. Applications that existed before keep HTTPS.
- Next.js apps open at `<name>.localhost` until their `next.config` allows the
  domain suffix; the app adds `allowedDevOrigins` on request.
- Apps with their own dev server also answer at `<name>.localhost`.
- A light theme that follows macOS.

### Fixed

- The Add Alias and New Group prompts and the new-database field took no typing.
- Next.js dev servers listened on every network interface; they now listen on
  127.0.0.1 only.
- Progress counters that use backspaces showed as escape-code fragments.

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
