# Changelog

All notable changes to Anvil are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Until 1.0.0, minor versions may contain breaking changes.

## [Unreleased]

### Added

- Database viewer (Settings › Plugins, off by default): Adminer 6.1.2 at
  `http(s)://db.<tld>`, served by the default PHP. Turning it on downloads the
  single-file build (418 KB) into `~/.anvil/adminer/6.1.2/`, checked against its
  pinned SHA-256 (older versions are removed; cleanable in Disk). Browse on each
  MySQL/PostgreSQL service, on each of its databases, and on an app's Overview
  Database line opens it logged in, with no password, to Anvil's own services
  on 127.0.0.1 and to SQLite files inside the apps' folders only; nothing else
  logs in without a password, and an Open link from another site's page does not
  log in. No version check or other request to adminer.org. An app called `db`
  keeps `db.<tld>`, and Settings says the viewer isn't served then.
  `anvil db open <service|app> [database]` prints the address.
- Browser errors, a switch per app in its Settings tab (off by default;
  `anvil app browser <app> --on`): the app's pages, in any browser you open them
  in, send their JavaScript errors with stack frames, unhandled rejections,
  console.error and warn (with where they were called), failed fetch/XHR and
  responses of 400 or more, resources that didn't load, CSP violations,
  deprecations and each page load's timings to Anvil, for agents. While any
  app has it on, PHP-FPM's pools (php-cgi's `-d` on Windows) prepend a small
  file from `~/.anvil/inject` that, only for that app (`ANVIL_INJECT`, set by
  its site block) and only for a page load (Sec-Fetch-Dest, else Accept), puts
  one `<script src="/__anvil/a.js?v=…">` before `</head>` of an HTML page;
  JSON, fragments, redirects, downloads and SSE are left alone, and a page
  whose Content-Security-Policy wouldn't let it run gets none (the Settings
  tab and the Overview say so). An `auto_prepend_file` of your own is still
  run. The script batches events (at most 200 a page, repeats counted, no
  cookies, storage, form values or bodies, console text cut at 1.5 KB) and
  sends them with `sendBeacon` to `/__anvil/e` on the page's own origin; Caddy
  answers 204 and writes them without headers or addresses to
  `~/.anvil/log/browser/<app>.log` (1 MiB, one older copy, 24 h; "Browser
  errors" in Disk). The Overview has a Browser line ("3 errors since 17:40").
  `anvil app browser <app>` lists them grouped, `--follow` as they come; the
  MCP `browser` tool returns them grouped by kind, message and place (count,
  first and last seen, page, top frames) with the latest page loads and a
  cursor, marked as untrusted, and `check` adds the new browser errors since
  its cursor. Apps with a server of their own (Vite, Next.js, Nuxt and other
  dev servers, an app's other dev servers at `<app>--<row>`, Octane) get the
  same script from a small proxy inside Anvil (a socket in `~/.anvil/run`; a
  loopback port on Windows), which runs with the web server while one of them
  has the switch on: Caddy sends it only their page loads (GET or POST asking
  for HTML, not websockets or `fetch`), the app is asked uncompressed and its
  page streams on with the script put in, status, headers and cookies as they
  were; assets, HMR and API calls go straight to the app as before. A stopped
  app still shows "not running", and a stopped Octane still falls back to
  PHP-FPM. Static sites don't have the switch.
- Tunnel (Settings › Plugins, off by default): an app's Overview gets a Public
  address line. Share, after a confirm, opens a Cloudflare quick tunnel (no
  account) to the app through Caddy with the app's own host, HTTPS apps over
  https; the `https://…trycloudflare.com` address shows once it answers, with
  Copy and Stop, and why it failed when it didn't. One tunnel per app; it ends
  with the app's server, the web server, the switch or Anvil. The address is
  temporary and changes every time. cloudflared 2026.10.2 is downloaded on the
  first Share (macOS arm64/x86_64, Windows x86_64), checked against its pinned
  SHA-256, into `~/.anvil/tunnel/<version>` (older versions removed; cleanable
  in Disk). Caddy now trusts X-Forwarded-* from this computer, so the app sees
  the public host; a Laravel app that doesn't trust proxies is told its links
  still point to `<app>.test`. `anvil app share <app>` does the same in a
  terminal until Ctrl-C.
- Mail catcher (`mail_catcher = true`): an SMTP listener on 127.0.0.1
  (`mail_port`, 2525 by default) inside the app and `anvil start` that only
  accepts and stores, never relays. Each app's processes, terminal PHP and web
  requests get `MAIL_MAILER=smtp`, `MAIL_HOST`, `MAIL_PORT`, `MAIL_USERNAME=<app>`
  (and `MAIL_URL`, `MAILER_DSN`), which win over `.env` in Laravel and Symfony,
  so each app's mail lands in an inbox of its own under `~/.anvil/mail/<app>`;
  mail from no known app goes to `_other`. Test runs keep their own mailer
  (phpunit.xml's `MAIL_MAILER=array`). An inbox keeps its newest 500 messages
  or 50 MB; Disk lists them as cleanable. `anvil app mail <app>` lists the
  latest, `--open <id>` prints one with its files and links. A taken port is
  reported and the web server runs on without it.
- Settings › Plugins › Mail catcher (off by default) turns it on, says where
  it listens or why it can't, and lists mail from no app. With it on, each app
  has a Mail tab: its mails newest first (time, from, to, subject, files,
  size), live while shown; one opens to its HTML in a sandbox (no scripts,
  nothing loaded from the network until Load remote images), text, headers
  or source, with Save (to Downloads) for its files, Delete and Clear. A
  switch in the tab leaves an app to its own mail setup. The Overview's Mail
  line says Anvil catches the app's mail instead of pointing at Mailpit.
  The app's CSP allows remote images so that button can work; the mail's own
  frame blocks them until then.
- Agents (MCP): a read-only `mail` tool lists an app's caught mail, newest
  first, or reads one message's text, HTML (each cut at 20,000 characters)
  and links, so an agent can follow a sign-up mail's link.
- Mail: an app whose `.env` sends mail to an Anvil Mailpit (`MAIL_MAILER=smtp`
  to this computer on that Mailpit's SMTP port) gets a Mail line on its
  Overview: Open inbox while Mailpit runs, Start while it doesn't. Other apps
  show nothing. `anvil app mail <app>` prints the inbox address. The inbox
  opens unfiltered: apps share Laravel's default sender, so a sender filter
  could hide mail.
- Requests: Caddy logs every request to an app (its redirects, extra server
  rows and worktrees each to their app's list) as JSON in
  `~/.anvil/log/requests/<app>.log`, rolled at 2 MiB with one older copy kept
  a day, so an app holds about 2 MB. Cookie, Set-Cookie, Authorization and
  Proxy-Authorization are written as REDACTED. `anvil app requests <app>`
  prints the latest (`--status 4xx`, `--method POST`, `-n 50`, `--follow`;
  Vite's, Next's and other static assets only with `--assets`). Disk's Logs
  item covers them. In the app, a Requests tab after Logs lists them newest
  first (time, method, status, path, duration, size; another host of the app
  shown before the path), new ones coming in while the tab is shown. Filter
  by status class, method or path, show assets, Clear; a row opens to its
  request and response headers, protocol, TLS and remote address.
- Laravel Octane: an app with `laravel/octane` gets an `octane` process
  (RoadRunner), left out of Dev start, on a private port of its own (no two
  apps share one, and none is among the ports macOS gives outgoing
  connections). While it runs, the app's address is served by Octane, also
  when laravel/octane was added while Anvil ran; when it stops or crashes,
  PHP-FPM serves the next request, never an error page. Anvil downloads
  RoadRunner 2025.1.15 once into its own folder (checked against its
  SHA-256) and never writes `rr` or `.rr.yaml` into the project; Disk lists
  the downloads as cleanable, and a newer pinned version replaces the old
  one. A worktree's
  Octane uses its own port and database. It runs two workers, not one per
  core (140 MB instead of 455 MB on a 12-core Mac); edit the command for more.
  FrankenPHP (`OCTANE_SERVER=frankenphp`) too: Anvil downloads FrankenPHP
  1.13.1 (177 MB, checked against its SHA-256) once the row first starts. It
  has PHP 8.5 inside, so an app on another version is told so on its row and
  PHP-FPM keeps serving it. It listens on 127.0.0.1 only, gets the app's
  php.ini values, and keeps its own extensions: the ones Anvil builds can't
  load into it. It has APCu and igbinary but not Xdebug, PCOV or msgpack: an
  app with one of those on is told so on its row instead of starting without
  it. Octane itself copies `public/frankenphp-worker.php` into the
  project, as `octane:install` does.
  Swoole (`OCTANE_SERVER=swoole`), experimental on macOS: Anvil builds
  Swoole 6.2.3 for the app's PHP (8.2 to 8.5) on your Mac when the row first
  starts (about a minute; checked against its SHA-256), against the headers of
  the OpenSSL release that PHP has inside, fetched once from OpenSSL's GitHub
  release and checked too (2.9 MB kept, a cleanable cache). Swoole loads into
  the Octane process only, never PHP-FPM or the app's other processes. Two
  workers and one task worker: 200 MB, not 533 MB with one per core.
- A web root of your own for an app Anvil serves (PHP, static): a folder inside
  the project (`web`, `dist`, `.`) in the app's Settings, or `anvil app root
  <site> [<dir>]` (`--reset` goes back to `public/` when present). A worktree
  serves the same folder of its own checkout. A web root that's gone shows a
  page saying so, never the project folder. One in a dot folder
  (`.output/public`) is served too; dotfiles inside it stay hidden.
- PHP 8.1 to 8.4 get OPcache: Anvil builds it from php.net's source of the
  same version on your Mac (checked against php.net's checksum; needs the Xcode
  command line tools), and every version serves with OPcache tuned for
  development, so a saved file shows on the next request. Add OPcache in
  Settings › PHP, or `anvil php opcache <version>`, adds it to a PHP installed
  before.
- PHP extensions for every app on a version: Xdebug, PCOV, APCu, igbinary and
  msgpack, each a switch under the version in Settings › PHP, or `anvil php ext
  add|remove|list [--php <version>]`. Anvil builds each on your Mac from PECL
  against the PHP you run (checked against checksums pinned in Anvil; needs
  the Xcode command line tools), and the web, the terminal's `php` and an
  app's processes load the same ones, as does a PHP those start (queue
  workers, `npm run dev` → `php artisan`, composer scripts), but not one in
  another project started from there. Xdebug starts only when a request
  asks for it (`xdebug.start_with_request=trigger`). Off keeps the build, so
  on again is instant; a new patch release of PHP builds them again, and so
  does moving an app to another version. One that is on without a build (it
  failed) shows as not built, in Settings, `anvil php ext list` and `anvil
  project`, not as on. `anvil php ext add|remove` restarts PHP-FPM in the open
  app.
- PHP extensions for one app: the same switches in the app's Settings tab, or
  `anvil app ext <site> add|remove <ext>`, e.g. Xdebug for the app you debug
  only. That app, and any other address of the same folder, gets a PHP-FPM of
  its own, with its own `xdebug.*` and `opcache.*` php.ini values; its
  terminal `php` and processes load the same extensions. A worktree has its
  app's until one is changed for it; from then on it has its own, so one its
  app has can be off for it.
- A project's `.anvil.toml` can ask for PHP extensions: `extensions =
  ["xdebug"]`. One not on (or not built) for the app or its PHP version shows
  in the Overview's Project line with Add, which turns it on for that app
  (building it first if needed); `anvil project --apply` does the same. A name Anvil
  doesn't offer is reported, like an unknown version.
- `anvil app start|stop|restart <site> [<process>]` and `anvil service
  start|stop <id>` ask the running app to do it, so the change shows there at
  once, and print each process's state (`api: running`). Without the app open
  they say so and exit with 1. A setting changed with `anvil` (link, park,
  https, an app's command) shows in the open app at once.
- What agents and the terminal did through Anvil ("Claude Code restarted api ·
  14:02"): an app's newest five on its Overview, all of them in Processes ›
  Agents, newest first. A failed action says why on its line.
- Agents (MCP), off by default in Settings › Plugins: `anvil mcp` lets AI
  agents list apps and their processes, start, stop and restart them and
  services, read logs and the requests the web server answered, ask what
  failed since their last check (exits, new errors and warnings, new 5xx
  requests), add and remove worktrees, and change an app's PHP or Node.js
  version, HTTPS, aliases and its processes' autostart. They can't change a
  start command or php.ini, which would let them choose what Anvil runs
  outside their shell's permission. A tool's app is optional: it defaults to the app (or worktree)
  of the folder the agent runs `anvil mcp` in. Results are compact JSON,
  also as structured content; every tool has a title and read-only,
  destructive and idempotent hints, so the agent's own permission prompts
  apply. Logs and requests come in pages of up to 200 rows and about 32 KB
  (`truncated`), with a `before` cursor; one request's headers by its id.
  Results with log lines, requests or mail start with a line saying they
  are untrusted app content. Removing a worktree is refused while it runs,
  with uncommitted changes or unpushed commits, or when its folder holds
  files git doesn't keep (a SQLite database, an edited `.env`); the user
  can still remove it with `anvil worktree remove`. Nothing that can't be
  undone is offered: no database drop, no service data removal, no clean,
  no uninstall. Claude Code connects with `claude mcp add anvil -- anvil
  mcp`. While it's off, `anvil mcp` says where to turn it on and exits
  with 1.

### Changed

- Packages tab: no full package list, search or Outdated/All any more. The key
  packages are badges with their installed versions, the detected ones by
  default (Laravel, Livewire, Tailwind, Vite, PHP, Node…); × unpins one, + pins
  any installed package (Composer and npm/pnpm/yarn/bun lock files included)
  from a search, Reset brings back the defaults. Kept per app in the config
  (`packages_pinned`); a worktree follows its app until it changes its own.
  Check outdated and Check security run on request; what they find shows as a
  notice (updates in orange, security advisories in red) and a tab per package
  manager counting the packages concerned, with only those inside: installed →
  latest (major called out), or an advisory's severity, title (opens it), CVE
  and affected versions.
- Sidebar: an app's worktrees sit on a band a tone darker, each with a branch
  mark carrying the same run badge as an app. The header's address list opens
  under its "+N", a tone darker, one row per address without its scheme (a
  lock for HTTPS); the whole row opens it. A linked app's worktrees nest
  under it too, and its aliases are no longer rows of their own (its header
  lists them). Aliases are added to the app only: its worktrees get theirs
  from it, so Add Alias is gone from a worktree's menu and refused for one.
- `anvil start` refuses while the Anvil app runs, which already serves your
  apps.
- Removing a worktree names the files git doesn't keep that go with its
  folder (its SQLite database, an edited `.env`) before you confirm. `anvil
  worktree remove` asks on a terminal and elsewhere refuses until `--yes`.
- Polling costs less while a window is open. The Overview, the Containers tab,
  the app list and the Processes window share one `docker ps` per 2 s, and a
  compose file's services are read again only when a compose or `.env` file
  changes, not twice per 2 s. The Processes window measures only the
  processes Anvil started instead of listing every process on the Mac.
- The Processes window says why Docker couldn't list the containers ("Docker
  isn't running") instead of leaving them out without a word.

### Fixed

- The Processes window and an app's Resources tab show CPU and memory when
  Anvil was started from a terminal whose language writes decimal commas
  (Turkish); they showed nothing measured.

- An app whose folder is inside a folder starting with a dot (`~/.sites/shop`)
  no longer answers 404 for its files (CSS, images, a static site's every
  page): only files and folders starting with a dot inside its web root
  (`.env`, `.git`) are hidden, as intended.
- A .NET app no longer offers a version to pin, which did nothing: dotnet
  picks the SDK from the project's `global.json`. `anvil app runtime` refuses a
  .NET version.
- Uninstalling a Node.js, Go, Rust or Python version apps are pinned to is
  refused, as for PHP, naming the apps; Settings shows them on the version
  and offers no Uninstall until they are unpinned.
- Dev processes an Anvil left running when it was killed no longer keep
  holding their ports, shown as stopped: the next Anvil stops them, child
  processes included, also when it was started from somewhere else (Finder,
  a terminal), and only then forgets them. Killing Anvil (SIGTERM) now stops
  everything as Quit does.
- php.ini values set in Anvil, for every app or for one, now reach an app's
  processes (queue workers, Octane's RoadRunner workers, artisan) and `php`
  and `composer` in a terminal, not only PHP-FPM: `memory_limit` is
  the same on the web and in the CLI. The CLI keeps PHP's own no time limit.
- `anvil app start` and `anvil service start` wait for a start that downloads
  first (FrankenPHP, 177 MB) and print how it went, instead of "Anvil didn't
  answer" after 5 seconds.

## [0.1.0] - 2026-10-19

The first public release of Anvil, a local development environment for macOS.
It comes as a desktop app that carries the `anvil` CLI.

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
- Isolated PHP 8.1 to 8.5 from portable builds, a default version (`anvil use
  8.4`) and a pinned version per application (`anvil app php <site> 8.3`).
- php.ini settings for all applications (`anvil php ini <key> <value>`) or for
  one (`anvil app ini <site> <key> <value>`); an application with its own
  settings gets its own PHP-FPM pool.
- Node.js, Go, Rust, .NET and Python toolchains installed into `~/.anvil`, with
  their package and build caches kept there too. Node.js follows `.nvmrc` or
  `.node-version`, .NET the project's `global.json`, and a Node.js, Go or
  Python app can be pinned to an installed version. Python apps run through uv
  with the Python the project asks for (`.python-version`, `requires-python`);
  their environment lives in `~/.anvil/cache/uv-venvs/<app>`, so a project's
  own `.venv` is left alone.
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
  Mailpit, Composer, rustup, and the .NET SDK against the SHA-512 in
  Microsoft's release list); PHP builds publish none and rely on HTTPS. A failed reinstall keeps the working version.
- The config file is written atomically, so the app and the CLI never read a
  half-written one.
- Git runs with `core.fsmonitor` disabled, so a repository can't make Anvil
  run a program; a folder whose path would break the web server config is not
  served.

[Unreleased]: https://github.com/n1crack/anvil/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/n1crack/anvil/releases/tag/v0.1.0
