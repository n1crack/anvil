# Changelog

All notable changes to Anvil. Details are in the [documentation](https://anvil.tr/docs).
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and
[Semantic Versioning](https://semver.org/spec/v2.0.0.html); until 1.0.0, minor versions may
contain breaking changes.

## [Unreleased]

### Changed

- Settings › Plugins is now Settings › Features: they come with Anvil and only need turning on.
- Settings › Features › Request log: on as before; off, the web server keeps no request logs and apps lose the Requests tab.
- Unfinished setup is one titlebar button; it lists each step and says which one asks for your password before anything runs.
- Processes works from the keyboard: arrows move between rows, Enter opens a row's log, Shift+F10 its menu.
- Settings › Storage asks before deleting in the row itself, not in a dialog, and its sizes line up in one column.
- Software Update: Return presses the main button and Esc closes the window, except while an update downloads.
- Settings › General: the menus on the right are all one width.
- Settings › Features › Agents (MCP) shows the whole Claude Code command with its own Copy, next to `anvil mcp` for other agents.
- Drop, Flush, Delete All, Clear inbox, Clear log, Resources' Delete and Remove Service ask in the window, not in a system dialog; Remove Service names its apps and labels both answers.
- A service's Applications says it changes no .env, can list linked apps, and offers "Write to <app>'s .env…" with the keys it sets.
- Settings › General: removing an application folder asks first, in its row.
- An app's php.ini fields restart the web server once per change and say so beside the field while it restarts.
- A share whose Stop fails says why on its line.
- Adding a service says what it is downloading and how much has come in, or that Redis is being built.
- Mail tab: after catching is turned on or off, it offers to restart the processes still sending the old way; inboxes count "messages".
- Worktree settings say in plain sentences what a new worktree gets; Customize… edits files, env keys and commands as rows with plain choices instead of text boxes.
- Add Folder of Applications… on a folder that is one project offers to add it as one application instead of serving its subfolders, and an empty folder says it has none yet.
- Changing an app's PHP version says which extensions it is building beside the version, and under it which ones the new version runs without.
- Remove Link… asks first and says what goes: the app's processes stop and its settings in Anvil are deleted; its folder stays.
- New Application says a missing PHP or runtime in the form, with an Install button, before anything is made; reopening the sheet keeps the typed name.
- A new app's Open is Open in Browser and opens it there, and selects it in the sidebar as before; with the web server stopped it is Show.
- Dev: a server whose port doesn't answer yet has an amber dot, a crashed process says its exit code, and dismissing a crash is ✕, not ■.
- Worktrees: Create says Creating… while it works, and a worktree being removed says Removing… in its row; a failed removal is said under its name.
- Storage and Resources mark what is safe to delete in grey instead of orange.
- Mail tab: deleting one message asks first, in its row, as Clear does.
- Worktree settings: "Add Install packages" reads "Add Package Install".
- A failed New Application's "Show in Finder" is "Show Log in Finder", which is what it shows.
- Changing a running app's type, start command, port or version says beside the field that it restarts the app; Start with Anvil no longer restarts it.

### Added

- Check for Updates… in the Anvil menu opens a Software Update window with what's new.
- The menu bar icon's menu offers a new version when there is one.

### Changed

- Process names in Dev and Containers no longer use red, orange or green, so only the dot shows a process's state.

### Fixed

- The sidebar shows an app red when one of its processes crashed, even while others run, as its header does.
- Release notes in an update read as text, without markdown marks.
- Processes: Clear log asks before it empties the log file.
- Processes: the Status column stays readable while a log is open, also at the smallest window size.
- The titlebar's setup and update buttons stay on one line in a small window; Update available looks like a button.
- Settings › PHP: every version row has the same height, and Extensions stays in place on hover and while Uninstall asks.
- Software Update: a failed check shows why under its title and offers Try Again; without release notes the window's content sits in the middle.
- Settings › Storage: Applications no longer says it is measuring and that no applications take space at the same time.
- Settings › Storage: a language's row points to its own Settings page, not the former Languages page.
- In a narrow window every tab of an app fits, Settings included, the header stays short and Dev's output keeps its room.
- An app's header counts a crashed Docker Compose container as crashed, not running.
- New Worktree…, Dev's Run and the Start/Stop of all processes look like the other buttons; Run no longer looks disabled.
- The keyboard focus ring on an app's tabs is drawn whole, not cut at the top and bottom.
- Service tags in the sidebar line up with the apps' type labels; short of room, a tag is left out instead of cut.
- The line under an app's or service's name wraps without a stray "·" at a line's start or end.
- Stopping an app also ends the processes it started in the background, such as Next.js's telemetry flusher, which were left running.
- The main window's small buttons and marks (close, remove, more, keys, start, stop, done, failed) use the same outline icons as the rest of Anvil.
- An app's Overview says how many of its processes run once, in the header; the Dev and Containers lines below keep their dots.
- The sidebar shows a Docker Compose app as crashed when one of its containers exited with an error, as its header does.
- A service in the sidebar has a right-click menu: Start or Stop, open in a database app, its data, its log and Remove Service….
- Hide from Anvil leaves "name hidden · Undo" in the app's place in the sidebar for a few seconds, instead of a notice at the bottom of the window.
- Caught mail rows fit their box in a narrow window: sender and subject are cut with "…" and shown whole on hover, and the recipient gives way first.
- A failed action shows where it was taken: an app's under its name, a service's on its page, Load older's and Clear's beside them. Other errors lie over the top of the pane and no longer push it down.
- Less CPU while an app's Dev, Requests or Overview tab, or the Processes window, is open.
- The Processes window asks Docker every 15 s instead of every 2 s while no container runs.
- A closed or minimized window no longer reloads after every action; it catches up when it is shown again.
- Anvil frees the main window's memory after it has been closed for 10 minutes; opening it again shows the same app and tab.
- Anvil uses much less CPU while it starts with many apps.
- Less CPU while an app starts.
- Less CPU while the Requests tab is open and no requests come in.

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
