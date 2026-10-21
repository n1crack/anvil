# Security policy

## Supported versions

Security fixes go into the latest release.

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
  folder can issue certificates your Mac trusts. `anvil uninstall` removes the
  authority again.
- Anvil downloads PHP, Caddy, language toolchains and service programs over
  HTTPS from their upstream projects, and checks each download against the
  checksum its publisher lists before unpacking or running it:
  - Node.js: SHA-256 from the release's `SHASUMS256.txt` (the file's GPG
    signature is not checked).
  - Go: SHA-256 from `go.dev/dl/?mode=json`.
  - Caddy: SHA-512 from the release's `caddy_<version>_checksums.txt` (its
    cosign signature is not checked).
  - PostgreSQL: SHA-256 from the `.sha256` file next to each theseus-rs build.
  - Redis: SHA-256 from the `redis/redis-hashes` list.
  - Versity S3 Gateway: SHA-256 from the release's `checksums.txt`.
  - SILO: SHA-256 from the release's `silo_<version>.0.0_checksums.txt`.
  - Composer: SHA-256 from `composer.phar.sha256`.
  - Rust: SHA-256 from the `rustup-init.sha256` file next to the installer.
  - Python: uv, pinned to the SHA-256 its release lists, installs
    python-build-standalone builds and checks them against the checksums
    built into uv.
  - Mailpit publishes no checksums; Anvil uses the SHA-256 GitHub records for
    each release file, which shows the download is the uploaded file.
  - MySQL publishes only MD5 (`.md5` next to each download). That catches a
    broken download but not a deliberately altered one.
  - The .NET install script publishes no checksums; for it HTTPS is the
    only integrity check. PHP (Anvil's builds, github.com/n1crack/anvil-php)
    is checked against the SHA-256 in that repository's index.json.

  A checksum fetched from the same host as the file guards against broken or
  swapped downloads, not against a compromised upstream. A failed or
  mismatched download leaves an existing install untouched.

Reports about these boundaries (for example something reachable from another
machine, or a file served that should not be) are welcome.
