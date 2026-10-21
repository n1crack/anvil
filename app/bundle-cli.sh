#!/bin/sh
# The anvil CLI the app bundle carries in Contents/MacOS, or on Windows next to
# anvil-app.exe in the install folder (tauri.cli.conf.json, bundle > externalBin).
# Tauri finds it by target triple; a universal app needs each architecture's for
# the build and the lipo'd one for the bundle.
set -eu
cd "$(dirname "$0")/.."
triple=${TAURI_ENV_TARGET_TRIPLE:-$(rustc -vV | sed -n 's/^host: //p')}
mkdir -p app/binaries
build() {
  case "$1" in
  *-windows-*)
    # From Linux or macOS, cargo-xwin brings Microsoft's linker and libraries.
    if [ "$(uname -s)" = Linux ] || [ "$(uname -s)" = Darwin ]; then
      cargo xwin build --release --locked -p anvil --target "$1"
    else
      cargo build --release --locked -p anvil --target "$1"
    fi
    target=${CARGO_TARGET_DIR:-target}
    cp "$target/$1/release/anvil.exe" "app/binaries/anvil-$1.exe"
    ;;
  *)
    cargo build --release --locked -p anvil --target "$1"
    target=${CARGO_TARGET_DIR:-target}
    cp "$target/$1/release/anvil" "app/binaries/anvil-$1"
    ;;
  esac
}
if [ "$triple" = universal-apple-darwin ]; then
  build aarch64-apple-darwin
  build x86_64-apple-darwin
  lipo -create -output app/binaries/anvil-universal-apple-darwin \
    app/binaries/anvil-aarch64-apple-darwin app/binaries/anvil-x86_64-apple-darwin
else
  build "$triple"
fi
