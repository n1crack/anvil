# Architecture

```
browser ──▶ Anvil DNS (*.test → 127.0.0.1)
        ──▶ Caddy (HTTP/HTTPS, local certificates, 127.0.0.1 only)
            ├─▶ PHP-FPM: a shared pool per PHP version, own pools for customised sites
            ├─▶ static files
            └─▶ app dev servers on private ports (Node.js, Rust, Go, .NET)

services (MySQL, PostgreSQL, Redis, Mailpit) ◀── your applications, 127.0.0.1 only
```

## Pieces

- **DNS.** A small DNS server built into Anvil listens on 127.0.0.1 (port 5354
  by default). It answers `127.0.0.1` for every name under the domain suffix
  and refuses everything else. macOS only asks it about `*.test` because of
  `/etc/resolver/test`; all other lookups are untouched.
- **Caddy** is the web server. Anvil downloads it into `~/.anvil/bin` and
  generates `~/.anvil/run/Caddyfile` from your sites: one block per site that
  serves files and forwards PHP to PHP-FPM, or proxies to an app's dev server.
  Caddy issues certificates from its own local certificate authority, which
  `anvil trust` adds to your keychain. Dotfiles are never served.
- **PHP-FPM.** One shared pool per installed PHP version serves every site on
  that version. A site with its own php.ini values gets its own pool so its
  settings never leak into other sites.
- **App dev servers.** Applications that bring their own server (Node.js, Rust,
  Go, .NET) are started by Anvil with the language toolchain from
  `~/.anvil/runtimes`, on a private port passed in `$PORT`, in their own
  process group so their child processes stop with them.
- **Services** run separately from the web server, so a site change never
  restarts a database. Each has its own port, data folder, log and pid file.

## Supervision

`anvil start` (or the app) runs everything as child processes and records their
pids in `~/.anvil/run`. If Anvil is killed, it cleans up those leftover processes
on the next start. Stopping sends SIGTERM with a grace period before SIGKILL.

## Code layout

- `src/` is the `anvil` crate: a library with the core (configuration, sites,
  runtimes, services, the supervisor) and the `anvil` CLI in `src/main.rs`.
- `app/` is the `anvil-app` crate: the Tauri 2 macOS app. Its Rust side calls
  the same library; its UI is plain HTML, CSS and JavaScript in `app/ui`, with
  no build step.

Everything lives in `~/.anvil` (see [configuration.md](configuration.md)).
