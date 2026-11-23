#!/usr/bin/env bash
# Releases all workspace crates to crates.io; see RELEASING.md.
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 <version> <next-version>    e.g. $0 0.5.0 0.5.1-alpha" >&2
  exit 2
fi
version=$1
next=$2
cd "$(dirname "$0")"

# Sets the package version and the version of every internal dependency.
set_version() {
  perl -pi -e '
    s/^version = "[^"]*"/version = "'"$1"'"/;
    s/^(dbmigrator\w*) = \{ version = "[^"]*"/$1 = { version = "'"$1"'"/;
  ' */Cargo.toml
}

git checkout main
git pull --ff-only
git diff --quiet HEAD || { echo "error: uncommitted changes" >&2; exit 1; }

set_version "$version"
cargo test --workspace
cargo publish --dry-run --workspace --allow-dirty
git diff --quiet || git commit -am "Bump version $version"
git tag -a "v$version" -m "v$version"

read -r -p "Push v$version and publish it to crates.io? [y/N] " answer
[[ $answer == [yY] ]] || exit 1
git push origin main "v$version"
cargo publish --workspace

set_version "$next"
git commit -am "Bump version $next"
git push origin main
