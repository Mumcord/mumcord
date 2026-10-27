#!/bin/bash
# Downloads the pinned mumbot release (scripts/mumbot.env) for one target, checks its SHA256 and
# unpacks the binary into <dest> (dest/mumbot, or dest/mumbot.exe on Windows).
#   scripts/fetch-mumbot.sh <linux-x86_64|linux-aarch64|windows-x86_64|macos-universal> <dest>
# Archives are kept in MUMBOT_CACHE (default ~/.cache/mumbot) under their versioned name, so CI
# can cache that directory by version and a re-run does not download again.
set -euo pipefail
target=${1:?target missing}
dest=${2:?dest missing}
# shellcheck source=scripts/mumbot.env
source "$(dirname "$0")/mumbot.env"

expected_var="MUMBOT_SHA256_${target//-/_}"
expected=${!expected_var:-}
[ -n "$expected" ] || { echo "unknown mumbot target '$target'" >&2; exit 2; }
case $target in
  windows-*) ext=zip ;;
  *) ext=tar.gz ;;
esac
asset="mumbot-$MUMBOT_VERSION-$target.$ext"
cache=${MUMBOT_CACHE:-$HOME/.cache/mumbot}
archive="$cache/$asset"

sha256() {
  if command -v sha256sum >/dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d' ' -f1
}

mkdir -p "$cache" "$dest"
if [ ! -f "$archive" ] || [ "$(sha256 "$archive")" != "$expected" ]; then
  echo "downloading $asset"
  curl -fsSL --retry 3 -o "$archive.part" \
    "https://github.com/Mumcord/mumbot/releases/download/v$MUMBOT_VERSION/$asset"
  mv "$archive.part" "$archive"
fi
actual=$(sha256 "$archive")
if [ "$actual" != "$expected" ]; then
  rm -f "$archive"
  echo "SHA256 mismatch for $asset: expected $expected, got $actual" >&2
  exit 1
fi

# Unpack aside first: the archive also carries README, LICENSE and an example config.
unpacked=$(mktemp -d)
trap 'rm -rf "$unpacked"' EXIT
case $ext in
  zip) unzip -q "$archive" -d "$unpacked" && cp "$unpacked/mumbot.exe" "$dest/" ;;
  *) tar -xzf "$archive" -C "$unpacked" && cp "$unpacked/mumbot" "$dest/" ;;
esac
echo "mumbot $MUMBOT_VERSION ($target) in $dest"
