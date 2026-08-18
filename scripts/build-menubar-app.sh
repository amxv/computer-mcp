#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
output="${1:-${repo_root}/target/debug/Zodex.app}"
binary="${output}/Contents/MacOS/zodex-menubar"
version="$(sed -n '/^\[package\]/,/^\[/ s/^version = "\([^"]*\)"/\1/p' "${repo_root}/Cargo.toml" | head -n 1)"
bundle_version="${version%%[-+]*}"

[[ -n "${version}" ]] || {
  echo "failed to resolve package version from Cargo.toml" >&2
  exit 1
}

rm -rf "${output}"
mkdir -p "$(dirname "${binary}")"
cp "${repo_root}/macos/ZodexMenuBar-Info.plist" "${output}/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString ${bundle_version}" "${output}/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion ${bundle_version}" "${output}/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :ZodexVersion ${version}" "${output}/Contents/Info.plist"

swiftc -O \
  -target arm64-apple-macos13.0 \
  "${repo_root}/macos/ZodexMenuBar.swift" \
  -o "${binary}"

codesign --force --sign - "${output}" >/dev/null
