# Contributing to Anvil

Thanks for helping. Bug reports, fixes and documentation are all welcome. For
larger changes, open an issue first so we can agree on the approach.

## Build

You need macOS, [Rust](https://rustup.rs) (stable) and the Xcode command line
tools.

```sh
cargo build --workspace          # the anvil CLI and the macOS app
cargo run -q -- --help           # run the CLI from the source tree
```

To bundle the app:

```sh
cargo install tauri-cli --version '^2' --locked
cd app && cargo tauri build      # target/release/bundle/macos/Anvil.app
```

Use `ANVIL_HOME=/tmp/anvil-dev` to keep a development run away from your real
`~/.anvil`.

## Layout

- `src/`: the `anvil` crate, a library with the core plus the CLI in
  `src/main.rs`
- `app/`: the `anvil-app` crate, the Tauri 2 app; the UI in `app/ui` is plain
  HTML, CSS and JavaScript without a build step
- `docs/`: user documentation

See [docs/architecture.md](docs/architecture.md).

## Before you open a pull request

CI runs these; run them locally first:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
for f in app/ui/*.js; do node --check "$f"; done
```

- Add a test for logic you change in `src/` where it is practical.
- Update the docs in `docs/` and the `[Unreleased]` section of
  [CHANGELOG.md](CHANGELOG.md) for anything a user would notice. Write the
  changelog for users, not commit by commit.
- Keep CLI help text and [docs/cli.md](docs/cli.md) in sync.

## Commit messages

- English.
- A short subject in the imperative mood, capitalised, without a trailing
  period: `Add a Logs tab with a log viewer per application`,
  `Fix Vite host check`. Around 50 to 70 characters.
- A body, when useful, that says what changed and why, wrapped at about 72
  characters.
- One logical change per commit.

## Releases

Anvil follows [Semantic Versioning](https://semver.org). The version lives in
one place, `[workspace.package] version` in the root `Cargo.toml`; both crates
inherit it and the app bundle reads it from there (`app/tauri.conf.json` has
no `version` on purpose).

1. Make sure `[Unreleased]` in `CHANGELOG.md` describes the release.
2. Run `scripts/bump-version.sh <version>` (macOS `sed`). It sets the workspace
   version, refreshes `Cargo.lock`, and turns `[Unreleased]` into
   `[<version>] - <today>` with compare links.
3. Check `git diff` and `cargo run -q -- --version`, commit
   (`Release <version>`), and tag: `git tag v<version>`.
4. Pushing the tag runs `.github/workflows/release.yml`, which builds the app
   and the CLI, and creates a **draft** GitHub Release with the changelog
   section as notes. Review it and publish.

Without the script: edit the version in `Cargo.toml`, run
`cargo update --workspace`, and add the changelog heading and link by hand.

The changelog links use `OWNER` as a placeholder until the repository has its
public home; replace it then.

## License

By contributing you agree that your contributions are licensed under the
[Apache License 2.0](LICENSE).
