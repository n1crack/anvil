# Changelog

All notable changes to Anvil. Details are in the [documentation](https://anvil.tr/docs).
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and
[Semantic Versioning](https://semver.org/spec/v2.0.0.html); until 1.0.0, minor versions may
contain breaking changes.

## [Unreleased]

### Changed

- First run shows the steps in order: set up Anvil, install PHP, add your projects folder, each ticked once done.
- The first PHP is 8.5 unless you pick another; 8.6 is offered as a preview. PHP version pickers show the full release, such as 8.5.1.
- Before setup, an app's address is dimmed with a Needs setup button.

## [0.4.1] - 2026-11-11

### Added

- A project's `dekit.yaml` or `mprocs.yaml` gives its Dev rows, each in its own folder; its autostart tasks are in Dev start. A compose file beside it no longer makes it a Docker Compose app.

### Changed

- An application's Settings tab reads like Settings: Application, PHP or Server, and Worktrees groups, each setting with its note beside it. Docker Compose apps and folders no longer show PHP settings.
- A crash notification says the last line the process printed, rather than only its exit code.

### Fixed

- Windows: Anvil installed in another account's profile and started as the signed-in account (the installer's Run Anvil, when it ran as administrator) asks Windows for administrator rights and runs as the account it was installed for, instead of setting itself up anew in the signed-in account.

## [0.4.0] - 2026-11-11

### Changed

- An application's Settings tab lists only the PHP extensions that are on; add one with Add, as in Settings › PHP.

### Fixed

- Windows: the Add service sheet opens at once, and checks of whether a port is in use no longer wait a quarter second for each free port.
- Windows: after Anvil crashed, MySQL starts again; the server it had running (a child of the one Anvil started) no longer keeps its port.
- Windows: updating no longer stops at "Error opening file for writing: anvil.exe" while a shim runs (an MCP server, a terminal's php); running shims also move to the new version at once.
- Windows: an update installs into the folder Anvil runs from and starts it again as the same user, with the same rights (an Anvil run as administrator comes back as administrator).
- Windows: PHP 8.5 no longer warns "Failed loading Zend extension 'opcache'" at every start (OPcache is built in); installed 8.5 drops the line with its next PHP update.
- Windows: Settings › PHP's Built in list includes what Anvil's php.ini loads (curl, mbstring, intl, gd, zip…).
- Another tab starts at its top instead of where the last one was scrolled to.
- Packages: the unchecked tiles say which check they are.
- Settings: Update and Install buttons beside a version look like every other button.
- Services: the link hint no longer says to copy lines that Write to .env writes.

## [0.3.6] - 2026-11-11

### Added

- PHP extensions: sqlsrv and pdo_sqlsrv for SQL Server (PHP 8.3 and later).
- Each PHP version has a folder for your own extensions (Settings › PHP › Extensions › Show folder): turn one on with `extension=name` in php.ini; PHP updates keep them.
- `anvil php ext build <folder>` builds an extension of your own into it on macOS.
- Laravel apps with Reverb get a reverb process, started with Dev start; it uses the app's own .env.
- MCP tools `php_extensions` and `php_extension`: agents can list a PHP version's extensions and turn Anvil's on or off.

### Changed

- Settings › PHP › Extensions lists only what's on; add one from Anvil's list with Add. What PHP has built in is shown under Built in.
- Shorter `anvil --help`.

### Fixed

- macOS: turning on redis no longer prints "Module redis is already loaded" on every PHP start (it's built in).
- The selected app's highlight follows the sidebar's width; the list no longer scrolls sideways.
- The tab underline doesn't slide when the same app is clicked again.
- Windows: the Add service sheet opens faster.
- The silk threads run behind a service's header too.

## [0.3.5] - 2026-11-10

### Added

- PHP extensions: phpredis.

### Changed

- New app icons.

### Fixed

- The tray icon no longer disappears on dark Windows and Linux panels; on Windows it follows the taskbar's light or dark setting.
- Windows' PHP loads the sockets extension, which Pest's browser plugin needs; installed versions gain it with their next PHP update.
- When the HTTPS port is taken, Windows names the program holding it instead of asking whether Anvil is already running.
- Windows' PHP also loads exif, ftp and soap; installed versions gain them with their next PHP update.
- Settings › PHP and the rest of Settings no longer wait minutes on Windows while it tries the PHP versions that aren't installed.
- Settings on Windows finds installed editors, terminals and git apps, and opens them.
- An app's Packages list won't start composer install while composer update runs, or the other way round.
- PostgreSQL services start on Windows when Anvil runs as administrator.
- Windows' PHP also loads ffi, pgsql and shmop; installed versions gain them with their next PHP update.

## [0.3.4] - 2026-11-03

### Fixed

- An app whose folder is renamed or moved, or whose parked folder is, keeps its address, settings, database, mail and worktrees.
- PHP's GD reads and writes JPEG, WebP and AVIF and draws text with FreeType fonts; installed versions update to the new builds.
- Scripts that `pnpm` runs, such as Vite plugins, find `php` and `composer` again.
- Install, Update and Scripts show their output as a terminal would: pnpm's progress redraws in place.
- Composer's progress bar redraws in place instead of adding a line per step, wherever Anvil runs it.
- An app's processes and scripts always get Anvil's `composer`, also when the terminal's PATH puts another first.

## [0.3.3] - 2026-10-29

### Changed

- A warmer, layered look with motion: selection and tabs slide, faint threads drift behind each app's header, the sidebar shows what kind each app is, and Overview shows what runs, with uptime and the latest requests.
- Forms and buttons read more clearly in light and dark, with a quieter focus ring.
- An app's worktree setup is edited in its own window.

### Fixed

- On macOS, `node`, `php` and the other terminal commands keep working after Anvil.app is moved or deleted; with Anvil gone they run the system's own.

## [0.3.2] - 2026-10-24

### Added

- Clean Rust builds (Settings › Storage, off unless you turn it on): after 7 days without a code change, a stopped Rust application's target folder goes; with cargo-sweep, the other stopped ones lose only files unused for 30 days.
- OPcache can be turned off per PHP version (Settings › PHP › Extensions); it stays on unless you do.
- macOS: Setup warns when another program already uses .test or ports 80/443.

### Changed

- Esc closes Settings.
- `anvil apps` lists address, type and web root; the PHP version column is gone.
- Settings: a version's Make default and Uninstall, and a package manager's Uninstall, are in a ⋯ menu at the end of its row.
- Settings › PHP: a click on an installed version opens its extensions; the default is marked by "Default" alone, without a check.
- Packages: the window of updates and advisories has Check again and an Update per package manager.

### Fixed

- Setup no longer blames another program for ports 80/443 that an earlier Anvil's forwarding still answers, and a long program path stays inside its warning.
- `laravel` no longer installs the Laravel installer by itself: it runs yours, else the one Settings › PHP installs, as `forge` does.
- The update, Settings and Processes windows open in the middle of Anvil's window.
- Software Update is small when Anvil is up to date, and grows for a release's notes.
- The Update bubble keeps showing the download if a click elsewhere hid it.
- Selecting an application or service in the sidebar no longer scrolls the list.
- Dev: "Not running" sits at the same height with or without a ▶ beside it.
- The selected choice of a segmented control (e.g. Configure's Table/JSON) stands out in the dark theme.
- Windows: PHP no longer crashes in OPcache when Anvil starts or switches versions, which took every app down until a restart; a PHP worker that stops in a request starts again on its own.
- Windows: a static app's address serves its index.html instead of "No input file specified" (404).
- Windows: a new PHP's first start no longer answers 502 for a minute while Windows checks its files; installing it does that wait.
- Linux: a running app's icon shows in the list again, not only its green dot.
- `anvil uninstall` removes ~/.anvil after the Database viewer, Mail catcher, browser bridge or Share were used.
- `anvil service list` lines up its columns for long service names.
- `anvil https` and `anvil tld` no longer say to restart `anvil start` while the app runs; `anvil app start` says the app, not `anvil start`, runs an app's processes.
- A new Django app keeps answering after `anvil tld` changes the suffix: its settings.py reads the suffix when the app starts, rather than having it written in.
- A Nuxt app's loading and error pages open at its .test address instead of answering "Forbidden: this host is not allowed" (403) while it builds.
- `anvil app php` warns when composer.json requires a newer PHP than the one pinned.
- `anvil service add --docker` says Docker isn't installed or running before it adds anything, instead of "Pulling…".
- `anvil service create-db` and `drop-db` say what they did.
- `anvil new` says when the new repository has no first commit, e.g. because Git has no name or email yet, or when Git isn't installed.
- `anvil usage <app>` leaves the port blank for a service the app names no port for, instead of "port -".

## [0.3.1] - 2026-10-22

### Added

- PHP keeps itself on the newest patch release (Settings › PHP › Update automatically); off, a version offers Update to the new one, and `anvil php update` does it from the CLI.
- Packages tab: Install and Update for Composer and the app's JS package manager, with their output in a window.
- Settings › PHP lists Composer's global packages with Update, and installs Laravel Forge's CLI (`forge`) on request.
- Docker services: Anvil runs an image for you or adds a container you already have, and starts, stops and restarts it (`anvil service add --docker mongo:7 --port 27017:27017`).
- External services: a MySQL, PostgreSQL or Redis server elsewhere, with its .env lines and database apps; its dot is green while it can be reached (`anvil service add --external db.lan:3306 --kind mysql`).
- A service's right-click menu has Restart and Rename….
- PHP 8.6 as a preview.
- Packages shows when vendor or node_modules isn't installed.

### Changed

- PHP comes from Anvil's own builds of php.net's releases, so new patch releases arrive within a day; OPcache, mysqli, intl and sodium are built in.
- Settings: plainer words, shorter hints, and rarely used options under Advanced.
- Settings: "Features" in the sidebar, Tunnel explained in its row instead of a page of its own, "Applications" throughout, and storage sizes lined up.
- Main window: plainer words on service, Mail and Settings pages; an external server shows Reachable, a crashed container says so.
- An app's Settings › Worktrees reads as a short checklist, and Customize… is a tidier form with Done at the bottom.
- Packages: a dashboard, with tiles for the framework, updates and advisories, then the key packages and the package managers, a row each with its Install and Update buttons; what a check finds opens in a window.
- Resources: space that can be rebuilt has a colour of its own.

### Fixed

- WordPress can be created on Linux.
- The update bubble keeps its size while Anvil restarts.

## [0.3.0] - 2026-10-21

Anvil for Linux, in beta; WordPress, more PHP extensions, and a round of polish and speed across the app and the CLI. Still a beta.

### Added

- Linux (beta): the app and the CLI for x86_64 Linux with glibc 2.35 or later (Ubuntu 22.04+, Debian 12+, Fedora, Arch), as an AppImage that updates itself, a .deb, and the CLI alone. Apps answer at `http://<name>.localhost:8080`; `.test` addresses, ports 80/443 and HTTPS trust come in a later release. PHP, Node.js, Go, Rust, .NET, Python, MySQL, PostgreSQL, Redis, Mailpit, S3 storage, every app template but WordPress, worktrees and Octane work; OPcache comes with PHP 8.5, and PHP extensions can't be added yet. MySQL and Redis are built in part from source the first time, so they need gcc and make.
- Linux: Settings finds your editor, terminal and git app (from PATH and Flatpak), and opens files at the line in them.
- WordPress: existing sites are recognised, and New Application creates one on a MySQL service, with PHP's mysqli built when it's missing.
- PHP extensions MongoDB and Memcached, and Imagick on a Mac with Homebrew's ImageMagick: `anvil php ext add mongodb`.
- Check for Updates… in the Anvil menu opens a Software Update window with what's new; the menu bar menu offers a new version when there is one.
- Settings › Features › Request log: on as before; off, the web server keeps no request logs and apps lose the Requests tab.
- Services: "Write to <app>'s .env…" writes a service's connection keys into an app's .env, after showing them.
- A service in the sidebar has a right-click menu: Start or Stop, open in a database app, its data, its log and Remove Service….
- `anvil apps` lists the applications; `anvil sites` still works.

### Changed

- Windows: Ctrl+1–9 open the app's tabs as they show, a hidden tab (Git, Dev, Mail) not counted.
- Anvil needs macOS 12 or later. MySQL (macOS 14), PostgreSQL (15), Mailpit (13) and Node.js 24 or later (13.5) say so before downloading on an older Mac.
- Settings › Plugins is now Settings › Features: they come with Anvil and only need turning on.
- Worktree settings say in plain sentences what a new worktree gets; Customize… edits files, env keys and commands as rows with plain choices.
- Every confirm is asked in the window, most in the row itself, never in a system dialog. Remove Service offers Remove, Keep Data or Remove and Delete Data.
- Unfinished setup is one titlebar button; it lists each step and says which one asks for your password.
- Add Folder of Applications… on a folder that is one project offers to add it as one application; an empty folder says it has none yet.
- New Application says a missing PHP or runtime in the form, with Install, before anything is made; after creating, Open in Browser opens the app.
- Changing an app's PHP version, type, start command, port, version or php.ini says beside the field what it builds or restarts, and what failed.
- Dev: amber while a server's port doesn't answer yet; a crash says its exit code; All processes is red while one has crashed, as in the sidebar.
- Adding a service says what it downloads and how much has come in. Worktrees say Creating… and Removing… where you asked.
- Processes works from the keyboard and says Restarting… or Stopping… in a row while it acts; the menu bar menu does the same.
- Processes › Agents: a failed action reads "couldn't start vite: <reason>"; actions from earlier days show their date.
- Settings › Features › Agents (MCP) shows the whole Claude Code command with its own Copy.
- Settings › PHP and the other language pages say a failed Install, Uninstall or Make default in its version's row.
- Mail tab: deleting one message asks first; after catching is switched it offers to restart the processes still sending the old way.
- The CLI explains every argument and flag with examples, says "application", prints what each setting command set, and gives one "No app named …" or "No service named …" error with what to run.
- Windows: shortcuts work with Ctrl, and the app says Ctrl and File Explorer instead of ⌘ and Finder.
- Menu bar menu: Quit reads Quit Anvil and Stop Everything with ⌥⌘Q; ⌘Q only closes the windows.
- Icons are one set of outline icons, and only a dot shows a process's state: names are no longer red, orange or green.

### Fixed

- Stopping an app also ends the processes it started in the background, such as Next.js's telemetry flusher.
- The sidebar shows an app red when one of its processes or Docker Compose containers crashed, also while others run.
- Less CPU at start with many apps, while an app starts, and while Dev, Requests, Overview or Processes is open; Processes asks Docker every 15 s while no container runs.
- A closed window no longer reloads after every action, and Anvil frees the main window's memory after it has been closed for 10 minutes.
- Release notes in an update read as text, without markdown marks.
- A failed action is said where it was taken, not in a banner far from it; Logs and Requests say when a log can't be read instead of looking empty.
- In a narrow window every tab fits, Requests' filters wrap, bubbles and lists open inside the window, and caught mail rows are cut with "…".
- Settings › Shortcuts lists the keys Anvil has: ← → fold an app's worktrees, ⌘Q closes the windows, ⌥⌘Q quits.
- `anvil unlink` and `anvil forget` say what they did and fail for a name Anvil doesn't serve; empty lists say what to run.
- The CLI's warnings carry no color codes when stderr isn't a terminal.
- Grey text reads at 4.5:1 or better in both themes; screen readers hear process states and the notes beside fields; Tab and Esc work in every bubble and sheet.
- Settings › Storage: no measuring and empty at once; safe to delete is grey, not orange; a language's row points to its own page.

## [0.2.1] - 2026-10-21

Anvil for Windows, in beta, next to macOS.

### Added

- Windows (beta): the app and the CLI for Windows 10 and 11, with a per-user installer. `.test` addresses, HTTPS, PHP, Node.js, Go, .NET, MySQL, PostgreSQL, Redis (Garnet), Mailpit, S3 storage, dev processes and worktrees; Python and Rust come later.
- Settings › Performance on Windows: whether your projects are on a Dev Drive, which antivirus scanning slows down less.
- Install Node.js in the bar at the top when an app needs it (a Node.js app, or one with a package.json) and none is installed.

### Fixed

- Terminal setup is offered again when the shell's PATH line points to another Anvil folder.
- New application: a folder can be chosen when none is parked, and the folders of linked apps are offered.
- New application: the steps' spinner no longer starts over while the app is created.
- Dev › Configure: Add and × of a new process sit side by side.
- `anvil uninstall` run a second time no longer says `~/.anvil` is kept.
- Settings › About › Credits says "Downloaded when used": Anvil fetches each the first time it's needed.

## [0.2.0] - 2026-10-21

The first public release, a beta for macOS: a desktop app that carries the `anvil` CLI.

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
- pnpm comes with Anvil: apps without a pnpm of yours use Anvil's, so Node.js 25+ apps install without Corepack.
- Shared dependencies: a pnpm app's node_modules can link into pnpm's store; on for worktrees (app Settings tab).
- Switch to pnpm: move an npm or yarn app to pnpm from the Packages tab (`anvil app pnpm <app>`).
- Storage shows pnpm's store and can prune it.
- `pnpm` in a terminal follows each app, like `node` and `npm`.
- PHP extensions: Xdebug, PCOV, APCu, igbinary and msgpack, per PHP version, per app or from `.anvil.toml`.
- OPcache for PHP 8.1–8.4.
- Laravel Octane with RoadRunner, FrankenPHP or Swoole (experimental).
- Requests tab: every request an app served, with headers (`anvil app requests`).
- Browser errors: an app's JavaScript errors, failed requests and page loads, for agents (`anvil app browser`).
- Plugins, off by default:
  - Agents (MCP): requests, mail, browser errors, worktree removal, aliases; the app defaults to the agent's folder.
  - Overview: Aliases, and a Mail line for apps that send to Mailpit.
- A web root of your choice per app (`anvil app root`).
- `anvil app start|stop|restart` and `anvil service start|stop` act in the running app.
- Processes › Agents: what agents and the terminal did through Anvil.

### Changed

- Each window can run only the actions it offers; Settings and Processes can no longer change apps or processes.
- New apps use pnpm by default (`anvil new --pm` picks another).
- Settings: the sidebar is grouped (app, Languages, Plugins), and a plugin that is on gets its own page.
- Settings › PHP: each version's extensions, OPcache included, slide open from "Extensions n/6"; OPcache can be built or removed, after a confirm.
- Settings: Make default and Uninstall are quiet text actions; Uninstall asks in the row.
- Switches show a muted green when on.
- Sidebar: worktrees fold under their app; a lock shows HTTP or HTTPS.
- Packages tab: key packages as badges, with Check outdated and Check security.
- New worktree: a branch that doesn't exist is created.
- Removing a worktree lists the files git doesn't keep before you confirm.
- php.ini values also apply to an app's processes and the terminal.
- Lower CPU use while windows are open.

### Fixed

- A dev server left behind by an Anvil that was killed stops before its app starts again, instead of failing with "address already in use".
- A service port picked for you is never Anvil's own web or DNS port.
- Share no longer says the web server isn't running when it is.
- Tunnel: a shared app whose HTTPS is turned on keeps working at its public address.
- An address no app has answers "No app at …" (404) instead of an empty page.
- Apps inside a dot folder (`~/.sites/shop`) returned 404 for their files.
- CPU and memory were missing under locales with decimal commas.
- Dev processes left by a killed Anvil kept their ports.
- Uninstalling a Node.js, Go, Rust or Python version apps are pinned to is refused.
- .NET apps no longer offer a version to pin.
- The first MySQL on a new install failed to start.

### Security

- Everything listens on 127.0.0.1 only; dotfiles are never served.
- Downloads are checked against their published checksums.

[Unreleased]: https://github.com/n1crack/anvil/compare/v0.4.1...HEAD
[0.4.1]: https://github.com/n1crack/anvil/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/n1crack/anvil/compare/v0.3.6...v0.4.0
[0.3.6]: https://github.com/n1crack/anvil/compare/v0.3.5...v0.3.6
[0.3.5]: https://github.com/n1crack/anvil/compare/v0.3.4...v0.3.5
[0.3.4]: https://github.com/n1crack/anvil/compare/v0.3.3...v0.3.4
[0.3.3]: https://github.com/n1crack/anvil/compare/v0.3.2...v0.3.3
[0.3.2]: https://github.com/n1crack/anvil/compare/v0.3.1...v0.3.2
[0.3.1]: https://github.com/n1crack/anvil/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/n1crack/anvil/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/n1crack/anvil/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/n1crack/anvil/releases/tag/v0.2.0
