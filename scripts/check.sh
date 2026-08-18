#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

if [[ "${ZODEX_ALLOW_RUSTC_WRAPPER:-0}" != "1" ]]; then
  unset RUSTC_WRAPPER
fi

echo "==> cargo fmt --check"
cargo fmt --check

if [[ "$(uname -s)" == "Darwin" ]]; then
  echo "==> Swift menu bar app"
  menubar_check="$(mktemp -d "${TMPDIR:-/tmp}/zodex-menubar-check.XXXXXX")/Zodex.app"
  trap 'rm -rf "$(dirname "$menubar_check")"' EXIT
  apps/menubar/build.sh "$menubar_check"
  codesign --verify --deep --strict "$menubar_check"
  bundle_version="$(/usr/libexec/PlistBuddy -c 'Print :ZodexVersion' "$menubar_check/Contents/Info.plist")"
  package_version="$(sed -n '/^\[package\]/,/^\[/ s/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)"
  [[ "$bundle_version" == "$package_version" ]] || {
    echo "menu bar bundle version $bundle_version does not match package version $package_version" >&2
    exit 1
  }
  rm -rf "$(dirname "$menubar_check")"
  trap - EXIT
fi

echo "==> cargo clippy --all-targets -- -D warnings"
cargo clippy --quiet --all-targets -- -D warnings

echo "==> source file LOC guard"
cargo test --quiet --test source_file_size source_files_stay_under_1000_lines

echo "==> cargo test"
cargo test --quiet
