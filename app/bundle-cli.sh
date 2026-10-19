#!/bin/sh
# The anvil CLI the app bundle carries in Contents/MacOS (tauri.cli.conf.json,
# bundle > externalBin). Tauri finds it by target triple; a universal app needs
# each architecture's for the build and the lipo'd one for the bundle.
set -eu
cd "$(dirname "$0")/.."
triple=${TAURI_ENV_TARGET_TRIPLE:-$(rustc -vV | sed -n 's/^host: //p')}
mkdir -p app/binaries
build() {
  cargo build --release --locked -p anvil --target "$1"
  cp "target/$1/release/anvil" "app/binaries/anvil-$1"
}
if [ "$triple" = universal-apple-darwin ]; then
  build aarch64-apple-darwin
  build x86_64-apple-darwin
  lipo -create -output app/binaries/anvil-universal-apple-darwin \
    app/binaries/anvil-aarch64-apple-darwin app/binaries/anvil-x86_64-apple-darwin
else
  build "$triple"
fi
