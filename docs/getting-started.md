# Getting started

## Requirements

- macOS on Apple Silicon (Intel builds are untested)
- [Rust](https://rustup.rs) to build Anvil from source
- Xcode command line tools (`xcode-select --install`) if you want Redis, which
  Anvil builds from source

The details (macOS versions, disk space, network hosts, ports and password
prompts) are in [System requirements](requirements.md).

## Install from source

```sh
git clone <repository-url> anvil && cd anvil
cargo install --path .
anvil --version
```

This installs the `anvil` command into `~/.cargo/bin`.

### The macOS app (optional)

```sh
cargo install tauri-cli --version '^2' --locked
cd app && cargo tauri build
open ../target/release/bundle/macos/Anvil.app
```

The app uses the same `~/.anvil` folder and configuration as the CLI. See
[app.md](app.md) for what it offers.

## First run

Install a PHP version (the first one becomes the default) and run setup, which
downloads the web server and prints a one-time DNS command:

```sh
anvil php install 8.4
anvil setup
```

PHP is optional if you only serve static sites or apps with their own server
(Node.js, Rust, Go, .NET); install those languages with
`anvil runtime install`, see [cli.md](cli.md).

## Resolver setup

macOS sends lookups for `*.test` to Anvil's DNS server once this file exists.
Run the command `anvil setup` printed; it needs your password once:

```sh
sudo mkdir -p /etc/resolver && printf 'nameserver 127.0.0.1\nport 5354\n' | sudo tee /etc/resolver/test
```

The file name is the domain suffix and the port is `dns_port` from
[configuration.md](configuration.md). If you change either setting, update
the file to match.

## Add projects and start

```sh
anvil park ~/Sites         # every folder in ~/Sites becomes <folder>.test
anvil start                # runs in the foreground; Ctrl-C stops it
```

Or serve a single folder: `cd ~/code/shop && anvil link` makes it `shop.test`.

## Trusting HTTPS

The first `anvil start` creates a local certificate authority in
`~/.anvil/data/caddy`. While Anvil runs, trust it in another terminal:

```sh
anvil trust
```

This adds the authority to your login keychain (macOS asks for your password).
Then open `https://<folder>.test:8443`. Plain HTTP works on port 8080 too.

## Uninstall

1. Quit the app or stop `anvil start`.
2. Run `anvil uninstall` to see what Anvil changed on this Mac, then
   `anvil uninstall --yes` to remove it. That covers the `/etc/resolver` files
   Anvil wrote (macOS asks for your password), its certificate authority in your
   login keychain, the login item, the app's folders in `~/Library` and
   `~/.anvil`. `~/.anvil` holds your service databases too: back them up first
   if you need them. Your projects are not touched.

   It only removes what it can tell is Anvil's: a resolver file with other
   lines (or an old DNS port) and a certificate authority trusted from a
   `~/.anvil` that was already deleted stay; remove those by hand
   (`/etc/resolver/<tld>`, "Caddy Local Authority" in Keychain Access). With
   `ANVIL_HOME` set, that folder is only deleted if it holds nothing but
   Anvil's own files.
3. Delete `Anvil.app` and run `cargo uninstall anvil`.

## Next

- [Applications](applications.md): what Anvil detects and how each type runs
- [Services](services.md): MySQL, PostgreSQL, Redis and Mailpit
- [The macOS app](app.md)
