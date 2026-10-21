# Changelog

All notable changes to Anvil. Details are in the [documentation](https://anvil.tr/docs).
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and
[Semantic Versioning](https://semver.org/spec/v2.0.0.html); until 1.0.0, minor versions may
contain breaking changes.

## [Unreleased]

### Added

- Check for Updates… in the Anvil menu opens a Software Update window with what's new.
- The menu bar icon's menu offers a new version when there is one.

### Fixed

- Release notes in an update read as text, without markdown marks.
- Processes: Clear log asks before it empties the log file.
- Processes: the Status column stays readable while a log is open, also at the smallest window size.
- The titlebar's setup and update buttons stay on one line in a small window; Update available looks like a button.
- Settings › PHP: every version row has the same height, and Extensions stays in place on hover and while Uninstall asks.

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

[Unreleased]: https://github.com/n1crack/anvil/compare/v0.2.1...HEAD
[0.2.1]: https://github.com/n1crack/anvil/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/n1crack/anvil/releases/tag/v0.2.0
