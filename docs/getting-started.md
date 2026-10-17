# Getting started

## Requirements

- macOS on Apple Silicon (Intel builds are untested)
- [Rust](https://rustup.rs) to build Anvil from source
- Xcode command line tools (`xcode-select --install`) if you want Redis, which
  Anvil builds from source

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
2. `rm -rf ~/.anvil` (this also deletes your service databases).
3. `sudo rm /etc/resolver/test`
4. Delete "Caddy Local Authority" from Keychain Access.
5. Delete `Anvil.app` and run `cargo uninstall anvil`.

## Next

- [Applications](applications.md): what Anvil detects and how each type runs
- [Services](services.md): MySQL, PostgreSQL, Redis and Mailpit
- [The macOS app](app.md)
