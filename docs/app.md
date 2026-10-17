# The macOS app

Anvil also comes as a native macOS app with a menu bar icon. When it launches,
it starts Anvil, the enabled services and the applications set to start with
Anvil. Build it from source as described in
[getting-started.md](getting-started.md#the-macos-app-optional).

- **Applications list** with search, a filter by type (only the types you have,
  with counts) and running only, sorting by name or date modified, your own
  groups (Favorites first), a section per parked folder, linked folders, and
  extra linked domains under their application. Add applications with the +
  button: New Application, Add Folder of Applications (park) or Add Application
  Folder (link).
- **Application panel** with Start, Stop and Restart for apps with their own
  server, quick buttons for browser, Finder, Terminal and editor, and these
  tabs:
  - **Overview:** type, runtime, folder, web root, disk, services in use and
    Git (branch, changes, last commit, remote).
  - **Logs:** the app's log files (Laravel, Symfony, app output, PHP-FPM) as
    readable entries with level filters, search, follow, expandable context and
    stack traces.
  - **Packages:** framework versions, then each package manager's dependencies
    (Composer, npm, pnpm, yarn, Cargo, Go, .NET) with the version the lock file
    pins, the scripts, and a check for newer versions on demand.
  - **Resources:** disk breakdown with clean-up, processes and services.
  - **Settings:** type, PHP version and php.ini settings, or start command,
    port and "Start with Anvil".
- **Services** next to Applications in the sidebar, see
  [services.md](services.md).
- **Settings window:**
  - **General:** site folders, launch at login, Dock icon, theme, editor and
    terminal, domain and ports.
  - **PHP:** install and remove versions, set the default, php.ini defaults for
    all applications.
  - **Languages:** install and remove Node.js, Go, Rust and .NET.
  - **Storage:** Anvil's own footprint and every application's size.
  - **Shortcuts** and **About**.
- **Menu bar menu:** status, Start/Stop, Restart, switch the default PHP
  version, Logs, Settings and Quit.

Closing the window keeps Anvil running in the menu bar; quit from the menu bar
icon. The app applies changes right away.

## Keyboard shortcuts

| Applications window | |
|---|---|
| Search applications | ⌘F |
| Move through the list | ↑ ↓ |
| Show or hide linked domains | ← → |
| Application tabs, in order | ⌘1 ⌘2 ⌘3 … |
| Open in browser | ⌘O |
| Open in editor | ⌘E |
| Open Terminal in the project | ⌘T |
| Show the app's logs | ⌘L |
| Reload applications | ⌘R |
| Settings | ⌘, |

| Menu bar | |
|---|---|
| Use a PHP version (in the menu) | ⌘1 … ⌘9 |
| Quit Anvil | ⌘Q |
