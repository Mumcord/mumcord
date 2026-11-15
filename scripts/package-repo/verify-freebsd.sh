#!/bin/sh
# Checks the built FreeBSD repository with pkg itself (run as root on FreeBSD, after
# build-freebsd.sh): a catalogue signed with another key and a changed package are rejected, then
# the package downloads from the real repository with the published config file. Only the
# Mumcord package is fetched, not its dependencies from the FreeBSD repository.
#
# Usage: verify-freebsd.sh <site>
set -eu

site=$1
root=$(cd "$(dirname "$0")/../.." && pwd)
abi="FreeBSD:15:amd64"
work=$(mktemp -d)

# A catalogue signed with a throwaway key, and a package with one byte appended.
mkdir -p "$work/bad-meta/freebsd" "$work/bad-pkg/freebsd"
cp -R "$site/freebsd/$abi" "$work/bad-meta/freebsd/"
openssl genrsa -out "$work/other.key" 2048 2>/dev/null
pkg repo -q "$work/bad-meta/freebsd/$abi" "$work/other.key"
cp -R "$site/freebsd/$abi" "$work/bad-pkg/freebsd/"
for p in "$work/bad-pkg/freebsd/$abi"/All/*.pkg; do
  printf '\0' >> "$p"
done

mkdir -p /usr/local/etc/pkg/repos /usr/local/etc/pkg/keys
cp "$root/packaging/repo/mumcord.pub" /usr/local/etc/pkg/keys/mumcord.pub
use() {
  sed "s#https://packages.mumcord.app#file://$1#" "$root/packaging/repo/mumcord.conf" \
    > /usr/local/etc/pkg/repos/mumcord.conf
  rm -rf /var/db/pkg/repos/mumcord /var/cache/pkg/mumcord-*
}
expect_fail() {
  if "$@" > "$work/out" 2>&1; then
    cat "$work/out"
    echo "::error::A tampered repository was accepted: $*" >&2
    exit 1
  fi
  echo "Rejected as expected: $*"
}

use "$work/bad-meta"
expect_fail pkg update -f -r mumcord
use "$work/bad-pkg"
pkg update -f -r mumcord
expect_fail pkg fetch -y -r mumcord mumcord
use "$site"
pkg update -f -r mumcord
pkg fetch -y -r mumcord mumcord
rm -f /usr/local/etc/pkg/repos/mumcord.conf /usr/local/etc/pkg/keys/mumcord.pub
rm -rf "$work"
echo "Repository verified for FreeBSD."
