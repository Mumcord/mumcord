#!/bin/sh
# Builds the signed FreeBSD pkg repository from the release's .pkg (run on FreeBSD).
#
# Layout: <out>/freebsd/FreeBSD:15:amd64/{meta.conf,packagesite.pkg,data.pkg,All/*.pkg}. The
# catalogue is signed with the repository's RSA key and lists each package's checksum, so pkg
# rejects a changed catalogue or package. Users point pkg at …/freebsd/${ABI}.
#
# Usage: build-freebsd.sh <version> <package dir> <out dir> <private key file>
set -eu

version=$1 pkgs=$2 out=$3 key=$4
abi="FreeBSD:15:amd64"
dir="$out/freebsd/$abi"

mkdir -p "$dir/All"
cp "$pkgs/mumcord-$version.pkg" "$dir/All/"
pkg repo -q "$dir" "$key"
