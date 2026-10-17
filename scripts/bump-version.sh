#!/bin/sh
# Usage: scripts/bump-version.sh 0.2.0
# Sets the workspace version (both crates and the app bundle inherit it),
# refreshes Cargo.lock, and turns the [Unreleased] changelog section into the
# new release. Review with `git diff`, then commit and tag v<version>.
set -eu

new=${1:?usage: $0 <version>, e.g. 0.2.0}
echo "$new" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$' || {
  echo "not a SemVer version: $new" >&2
  exit 1
}
cd "$(dirname "$0")/.."

old=$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml)
[ -n "$old" ] || { echo "no [workspace.package] version in Cargo.toml" >&2; exit 1; }
grep -q "^## \[$new\]" CHANGELOG.md && { echo "CHANGELOG.md already has $new" >&2; exit 1; }

# Only the version line inside [workspace.package].
sed -i '' "/^\[workspace.package\]/,/^\[/s/^version = \".*\"/version = \"$new\"/" Cargo.toml
cargo update --workspace --quiet

# New heading under [Unreleased], and compare links at the bottom.
today=$(date +%Y-%m-%d)
sed -i '' \
  -e "s/^## \[Unreleased\]$/## [Unreleased]\\
\\
## [$new] - $today/" \
  -e "s|^\[Unreleased\]: \(.*\)/compare/v$old\.\.\.HEAD$|[Unreleased]: \1/compare/v$new...HEAD\\
[$new]: \1/compare/v$old...v$new|" \
  CHANGELOG.md

echo "Version $old -> $new. Next: git diff, commit, git tag v$new"
