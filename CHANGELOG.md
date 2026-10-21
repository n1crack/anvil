# Changelog

All notable changes to Anvil. Details are in the [documentation](https://anvil.tr/docs).
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and
[Semantic Versioning](https://semver.org/spec/v2.0.0.html); until 1.0.0, minor versions may
contain breaking changes.

## [Unreleased]

## [0.3.0] - 2026-10-21

Anvil for Linux, in beta; WordPress, more PHP extensions, and a round of polish and speed across the app and the CLI. Still a beta.

### Added

- Linux (beta): the app and the CLI for x86_64 Linux with glibc 2.35 or later (Ubuntu 22.04+, Debian 12+, Fedora, Arch), as an AppImage that updates itself, a .deb, and the CLI alone. Apps answer at `http://<name>.localhost:8080`; `.test` addresses, ports 80/443 and HTTPS trust come in a later release. PHP, Node.js, Go, Rust, .NET, Python, MySQL, PostgreSQL, Redis, Mailpit, S3 storage, every app template, worktrees and Octane work; OPcache comes with PHP 8.5, and PHP extensions can't be added yet. MySQL and Redis are built in part from source the first time, so they need gcc and make.
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

[Unreleased]: https://github.com/n1crack/anvil/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/n1crack/anvil/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/n1crack/anvil/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/n1crack/anvil/releases/tag/v0.2.0
