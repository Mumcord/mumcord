#!/bin/bash
# Downloads the pinned official Mumble server for macOS (scripts/smoke/mac-server.env), checks its
# SHA-256 and its signature, and puts the binary at <dest>/mumble-server.
#   scripts/smoke/fetch-mac-server.sh <arm64|x86_64> <dest>
set -euo pipefail
arch=${1:?arch missing}
dest=${2:?dest missing}
here=$(dirname "$0")
# shellcheck source=scripts/smoke/mac-server.env
source "$here/mac-server.env"

case $arch in
  arm64) asset_arch=arm64 ;;
  x86_64) asset_arch=x64 ;;
  *) echo "unknown arch '$arch'" >&2; exit 2 ;;
esac
expected_var="MAC_SERVER_SHA256_$asset_arch"
expected=${!expected_var}
asset="mumble_server-$MAC_SERVER_VERSION.$asset_arch.macos"
url="https://github.com/mumble-voip/mumble/releases/download/$MAC_SERVER_TAG/$asset"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
curl -fsSL --retry 5 --retry-delay 5 -o "$work/$asset" "$url"
curl -fsSL --retry 5 --retry-delay 5 -o "$work/$asset.sig" "$url.sig"

actual=$(shasum -a 256 "$work/$asset" | cut -d' ' -f1)
if [[ "$actual" != "$expected" ]]; then
  echo "SHA256 mismatch for $asset: expected $expected, got $actual" >&2
  exit 1
fi

command -v gpg >/dev/null || brew install gnupg
export GNUPGHOME="$work/gnupg"
mkdir -m 700 "$GNUPGHOME"
gpg --batch --quiet --import "$here/mumble-auto-build-2026.asc"
# VALIDSIG with the pinned fingerprint: a good signature by exactly that key. It is also reported
# once the key has expired, as long as the signature was made while it was valid.
if ! gpg --batch --status-fd 1 --verify "$work/$asset.sig" "$work/$asset" 2>/dev/null \
    | grep -q "^\[GNUPG:\] VALIDSIG $MAC_SERVER_KEY_FPR "; then
  echo "signature check failed for $asset" >&2
  exit 1
fi

mkdir -p "$dest"
install -m 755 "$work/$asset" "$dest/mumble-server"
echo "Mumble server $MAC_SERVER_VERSION ($asset_arch) in $dest"
