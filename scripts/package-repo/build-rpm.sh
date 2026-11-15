#!/bin/bash
# Builds the signed RPM repository (Fedora, openSUSE) from the release's .rpm files.
#
# Every package gets an RPM signature (rpm --addsign) and repodata/repomd.xml a detached
# signature (repomd.xml.asc), so the .repo file can set both gpgcheck=1 and repo_gpgcheck=1.
# repomd.xml.key is the public key under the name zypper looks for.
#
# Usage: GNUPGHOME=… build-rpm.sh <fingerprint> <version> <package dir> <out dir>
# Needs: rpmsign and rpmkeys (rpm), createrepo_c, gpg.
set -euo pipefail

fpr=$1 version=$2 pkgs=$3 out=$4
root=$(cd "$(dirname "$0")/../.." && pwd)
rpm=$out/rpm

install -d "$rpm"
for arch in x86_64 aarch64; do
  cp "$pkgs/Mumcord-${version}-1.${arch}.rpm" "$rpm/"
done

rpmsign --addsign \
  --define "_gpg_name $fpr" \
  --define "_gpg_path $GNUPGHOME" \
  --define "_gpg_digest_algo sha256" \
  --define "__gpg $(command -v gpg)" \
  "$rpm"/*.rpm </dev/null >/dev/null

# Checks the signatures against the committed public key in a throwaway rpm database, the way
# dnf and zypper will.
db=$(mktemp -d)
rpmkeys --dbpath "$db" --import "$root/packaging/repo/mumcord.asc"
for f in "$rpm"/*.rpm; do
  result=$(rpmkeys --dbpath "$db" --checksig "$f")
  [[ "$result" == *": digests signatures OK" ]] || { echo "::error::$result" >&2; exit 1; }
done
rm -rf "$db"

# gzip metadata: older zypper (openSUSE Leap) cannot read zstd.
createrepo_c --quiet --general-compress-type=gz "$rpm"
gpg --batch --yes --local-user "$fpr" --digest-algo SHA512 --armor --detach-sign \
  -o "$rpm/repodata/repomd.xml.asc" "$rpm/repodata/repomd.xml"
cp "$root/packaging/repo/mumcord.asc" "$rpm/repodata/repomd.xml.key"
