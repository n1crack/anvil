# Changelog

All notable changes to Anvil are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Until 1.0.0, minor versions may contain breaking changes.

## [Unreleased]

### Added

- `anvil app start|stop|restart <site> [<process>]` and `anvil service
  start|stop <id>` ask the running app to do it, so the change shows there at
  once, and print each process's state (`api: running`). Without the app open
  they say so and exit with 1.
- What agents and the terminal did through Anvil ("Claude Code restarted api ·
  14:02"): an app's newest five on its Overview, all of them in Processes ›
  Agents, newest first. A failed action says why on its line.

## [0.1.0] - 2026-10-19

The first public release of Anvil, a local development environment, for macOS on
Apple Silicon. It comes as a desktop app that carries the `anvil` CLI.

### Added

- Local domains: every application answers at `<name>.test` through a built-in
  DNS server and a one-time `/etc/resolver` entry (`anvil tld localhost` uses
  `<name>.localhost` instead, with no setup).
- `anvil setup` (one password prompt) forwards ports 80 and 443 on this Mac to
  Anvil, so addresses need no port number, trusts the HTTPS certificate and
  puts the terminal commands, `anvil` among them, on PATH. `anvil uninstall`
  lists and removes everything Anvil changed.
- HTTPS per application through Caddy and a local certificate authority:
  right-click › HTTPS in the app, `anvil https <site> [--off]` in the CLI. With
  HTTPS on, the http:// address redirects to https://. New applications are
  served over plain HTTP; Settings › "HTTPS for new applications" changes that.
- Parking a folder makes every sub-folder an application; linking serves any
  single folder; extra domains can be added to an application and show in its
  header.
- Automatic detection of Laravel, Statamic, Symfony, PHP, static, Next.js, Nuxt,
  Vite, Node.js, Rust, Go, .NET, Django, FastAPI, Flask and Docker Compose
  projects, with a manual override per app.
- Isolated PHP 8.0 to 8.5 from portable builds, a default version (`anvil use
  8.4`) and a pinned version per application (`anvil app php <site> 8.3`).
- php.ini settings for all applications (`anvil php ini <key> <value>`) or for
  one (`anvil app ini <site> <key> <value>`); an application with its own
  settings gets its own PHP-FPM pool.
- Node.js, Go, Rust, .NET and Python toolchains installed into `~/.anvil`, with
  their package and build caches kept there too. Node.js follows `.nvmrc` or
  `.node-version`, and every app can be pinned to an installed version. Python
  apps run through uv with the project's own Python and `.venv`.
- Applications with their own dev server run on a private port behind the
  proxy and also answer at `<name>.localhost`. Extra package.json dev servers
  get their own address, `<app>--<script>.test`. Next.js apps open at
  `<name>.localhost` until their `next.config` allows the domain suffix; the app
  adds `allowedDevOrigins` on request.
- A Dev tab (⌘3) runs an app's dev processes (for Laravel the queue, log tailer
  and Vite from `php artisan dev`, otherwise package.json dev servers) and
  shows their live, colored output, one at a time or all merged. The toolbar
  has Run ▾ (a package.json or composer.json script, after a confirmation),
  Configure and Start/Stop. Keys: ↑↓ select, s start, x stop, r restart, q stop
  all, c clear; ⌘-click opens a link in the output.
- Configure edits an app's processes as a table: command, "Dev start", "With
  Anvil", processes of your own, and "Reset to detected". It is stored in
  `~/.anvil/config.toml`; the project's files are never changed.
- Processes Anvil starts get color (`FORCE_COLOR`, `--ansi` for artisan) and
  the PATH your terminal has, so Homebrew tools such as pnpm are found. A
  process that needs `node_modules` installs it first, with the package manager
  the lock file names.
- Docker Compose projects: a Containers tab shows each container's state and
  live log, with Start, Stop and Restart. Stop never removes containers.
- A Processes window (the pulse icon in the toolbar) lists everything Anvil
  runs in three tabs, Apps, Services and Anvil, with port, PID, CPU, memory and
  a live, colored log.
- Services: MySQL (9.7, 8.4, 8.0), PostgreSQL (18 to 14), Redis (8.10, 7.4, 7.2,
  built from source), Mailpit, and S3 storage with the Versity S3 Gateway
  (Apache-2.0) or SILO (AGPL-3.0, the maintained MinIO fork), both checked
  against their release's SHA-256 list. Add any number of each, each with its
  own name, port, data folder, log, tags and attached applications. Services
  keep the version they were added with; removing one keeps its data unless
  asked.
- See inside a running service: databases with sizes (create and drop), Redis
  keys and memory, Mailpit messages, S3 keys and the Laravel `.env` lines with
  a link to the web console.
- For each application, where the database, cache and mail server named in its
  `.env` come from: an Anvil service, Docker, another local program or a remote
  host.
- A project's `.anvil.toml` can ask for its PHP and Node.js versions and the
  services it needs, so whoever clones it gets the same setup:
  `php = "8.4"`, `node = "22"`, `[services] mysql = "8.4"` (`""` for any
  version). A version you pick in Anvil still wins. What's missing shows as one
  line in the Overview with Install and Add; `anvil project [--apply]` lists it
  and installs or adds it. Anvil never writes the file.
- New applications from templates for every supported type, in the app with
  their setup shown live, or with `anvil new` and the same options as flags
  (`anvil new laravel shop --starter react --db pgsql --pm pnpm --php 8.4
  --no-git --https`; `anvil new --help` lists which templates each applies to).
- Git worktrees run side by side. Each worktree of an app appears under it in
  the sidebar and answers at `<app>--<name>.test`, with its own processes and
  its own database: an empty one, or a copy of the app's when "Copy the
  database too" is on as you create it (`anvil worktree add --copy-db`). It
  starts off unless Settings › Worktrees, a preset or the app turns it on,
  also for worktrees made outside Anvil; an empty one never gets the app's
  SQLite file, whatever the copied files list. Its address shows why it isn't
  served until its database is made: start it in Anvil. A database name that
  could also be another app's worktree's ends in a short hash. Its Redis and
  cache keys get their own prefix. Tests use the database their test config
  names (`:memory:` too) unless that is the app's; a worktree whose test
  config would reach the app's database is stopped until it names one of its
  own. A database Anvil didn't make is only used after "Use existing" (`anvil
  worktree use-db`). Removing a worktree in the app stops its processes;
  `anvil worktree remove` asks you to stop it in Anvil first. Either drops its
  database only when you ask; databases left behind by worktrees that are gone
  are listed with Delete and Keep. A worktree made outside Anvil gets offers to
  copy the app's `.env` and install its packages. Create and remove them in
  the app (right-click, or the Git tab; New Worktree opens as a sheet, like
  New application) or with `anvil worktree list|add|remove`. An alias can't
  have worktrees of its own.
- Worktree hooks: what a new worktree of an app gets is yours to set. The
  files copied from the app (`.env` for Laravel, or what `.worktreeinclude`
  lists), "Copy the database too", the env keys set per worktree (`{db}`,
  `{url}`, `{prefix}`, `{port}`, `{name}`), the keys checked against the
  app's, and commands offered once it is made (`composer install`, `npm ci`,
  `php artisan migrate` for Laravel), run only when you say Run. Anvil ships a
  preset per framework, editable in Settings › Worktrees with Reset to
  default; an app changes any of it in its Settings tab, and a project can
  share its own in `.anvil.toml`'s `[worktree]`. The keys go into everything
  Anvil starts and into the env files it reads (`.env`, `.env.local`,
  `.env.development`, `.env.development.local`, and Symfony's
  `.env.<APP_ENV>` and `.env.<APP_ENV>.local`; never `.env.example`,
  `.env.testing` or `.env.test`) when Anvil copies them; a worktree whose env
  file still has the app's value for a checked key says so on its Overview,
  with "Rewrite with the hooks", which changes only those lines. A database
  named in a URL's query (`?database=`, `?dbname=`, `?db=`), percent-encoded
  or not, becomes the worktree's too. Symfony's `.env.dist` is read when there
  is no `.env`, never written. A Symfony app with composer.json's
  `extra.runtime.dotenv_overload` on or `extra.runtime.dotenv_path` set runs
  nothing in a worktree, since Anvil's keys couldn't win, and its
  `bin/phpunit` counts as a test run. A Laravel test run reads `.env.testing`
  too, and a DB_URL only `.env.testing` names points at the worktree's
  database. Taking the database keys out of an app's own settings shares the
  app's database; a `.anvil.toml` or a preset that leaves them out still gets
  them. A database key left empty, or one whose placeholder has no value,
  stops the worktree instead, and tests always get the worktree's own. So does
  a key named like a database that the hooks don't set (`POSTGRES_DB`,
  `DB_NAME`, a second connection's `DB_LEGACY_DATABASE`) while it still has
  the app's value: give it one in the hooks' keys. `anvil worktree hooks`
  prints an app's hooks, where each comes from, and any mistake in them.
- Disk usage per application split into safe, rebuildable and keep, with
  clean-up that never touches your code or data (`anvil usage`, `anvil clean`).
- Log retention: Anvil's own logs stay under 5 MB. Settings › Storage › "Trim
  app logs weekly" (on by default) cuts an app's `storage/logs/*.log` over
  10 MB to its last 1 MB and deletes daily logs older than 14 days.
- Git status per application (`anvil git`). A Logs tab for Laravel, Symfony,
  app output and PHP-FPM logs (`anvil logs`) that follows new entries, groups
  repeated ones, shows the app's own stack frames first and opens a frame in
  your editor.
- Each application's dependencies with locked versions, one tab per package
  manager, with updates checked only when asked (`anvil packages`). "Check
  security" runs the package manager's own audit (`composer audit`, or `npm`,
  `pnpm`, `yarn` or `bun audit`) and lists each advisory with its fix
  (`anvil packages --audit`).
- A desktop app with a menu bar icon: applications and services side by
  side, an application panel with Overview, Git, Dev or Containers, Logs,
  Packages, Resources and Settings tabs, a Settings window (General, a page per
  language, Storage, Shortcuts, About), keyboard shortcuts, launch at login and
  a light and dark theme that follows macOS.
- Updates: Anvil checks for a new release daily and shows "Update available" in
  the titlebar (Settings › About › "Check for updates" asks now). "Restart to
  update" installs it and brings back the dev processes that were running.
- macOS notifications while Anvil isn't in front: a dev process, Caddy or
  PHP-FPM stopping unexpectedly, and a new application's setup ending.
  Settings › General › Notifications turns them off.
- The CLI uses color on a terminal; `NO_COLOR` turns it off. `anvil --version`
  prints the release version.

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
