# Contributing to Anvil

Thanks for helping. Bug reports, fixes and documentation are all welcome. For
larger changes, open an issue first so we can agree on the approach.

## Build

You need macOS, [Rust](https://rustup.rs) (stable), the Xcode command line
tools, and [pnpm](https://pnpm.io) for the app's UI.

```sh
cargo build --workspace          # the anvil CLI and the macOS app
cargo run -q -- --help           # run the CLI from the source tree
```

The app's UI is a TypeScript project built with [Vite+](https://viteplus.dev):

```sh
pnpm --dir app/ui install
cargo install tauri-cli --version '^2' --locked
cd app && cargo tauri dev        # the app with the UI hot-reloading
cd app && cargo tauri build      # target/release/bundle/macos/Anvil.app
```

`cargo tauri` builds the UI itself (`pnpm build` in `app/ui`). Plain `cargo
build` of the app crate needs `app/ui/dist`, so run `pnpm --dir app/ui build`
once first.

Settings › About › Credits lists the software Anvil downloads (a hand-kept table,
`SOFTWARE` in `app/ui/src/settings.ts`: check the license at the source when you
add or change one) and the libraries compiled into Anvil. `pnpm build` generates
the second part first (`pnpm credits`, `app/ui/scripts/credits.mjs`): crates from
`cargo metadata` and npm packages from `pnpm licenses list --prod`, written to
`app/ui/public/credits.json` and `THIRD_PARTY_NOTICES.txt`, which ship in the app.
Both files are generated, not committed.

Use `ANVIL_HOME=/tmp/anvil-dev` to keep a development run away from your real
`~/.anvil`.

## Layout

- `src/`: the `anvil` crate, a library with the core plus the CLI in
  `src/main.rs`
- `app/`: the `anvil-app` crate, the Tauri 2 app
- `app/ui`: the windows' UI: HTML and CSS, TypeScript modules in `src/`
  (`types.ts` mirrors what the backend sends), tests in `test/`

User documentation lives on the website, [anvil.tr/docs](https://anvil.tr/docs), from its own repository; see [Architecture](https://anvil.tr/docs/architecture).

## Before you open a pull request

CI runs these; run them locally first:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd app/ui && pnpm typecheck && pnpm lint && pnpm fmt:check && pnpm test && pnpm build
```

- Add a test for logic you change in `src/` where it is practical.
- Describe anything a user would notice in the pull request, so the website's documentation can follow.
- Leave [CHANGELOG.md](CHANGELOG.md) alone: it is written once per release
  from the merged pull requests, so describe the change for users in the pull
  request instead.
- Keep CLI help text in sync with the [CLI reference](https://anvil.tr/docs/cli).

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

1. Write `[Unreleased]` in `CHANGELOG.md` from the pull requests merged since
   the last tag (`gh pr list --state merged --search "merged:>YYYY-MM-DD"`),
   for users, not pull request by pull request.
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

## License

By contributing you agree that your contributions are licensed under the
[GNU General Public License v3.0](LICENSE).
