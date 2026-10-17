# Security policy

## Supported versions

Anvil is in early development. Security fixes go into the latest release only.

## Reporting a vulnerability

Please do not open a public issue for a security problem. Report it privately
through GitHub's private vulnerability reporting: the **Report a vulnerability**
button under the repository's **Security** tab.

Include what you found, how to reproduce it, and the Anvil version
(`anvil --version`). You can expect a first reply within a week. Once a fix is
released, the report is credited in the changelog unless you prefer otherwise.

## Design

Anvil is meant for your own machine only:

- The web server, the DNS server and every service listen on 127.0.0.1 (the
  web server also on `::1`). Nothing is exposed to the network by Anvil.
- Apps with their own server are started with `HOST=127.0.0.1` (and
  `ASPNETCORE_URLS` on 127.0.0.1 for .NET). A dev server that ignores these and
  binds to all interfaces is outside Anvil's control.
- Dotfiles such as `.env` and `.git` are never served.
- Services start with development credentials: MySQL `root` without a password,
  PostgreSQL `postgres` with trust authentication. Do not use them for
  anything but local development.
- `anvil trust` adds Caddy's local certificate authority to your login keychain.
  Its private key lives in `~/.anvil/data/caddy`; anyone who can read that
  folder can issue certificates your Mac trusts. Remove the authority from
  Keychain Access when you uninstall Anvil.
- Anvil downloads PHP, Caddy, language toolchains and service programs over
  HTTPS from their upstream projects. The downloads are not checksum-verified
  yet; HTTPS is the only integrity check.

Reports about these boundaries (for example something reachable from another
machine, or a file served that should not be) are welcome.
