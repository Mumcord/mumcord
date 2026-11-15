#!/bin/bash
# Builds the signed APT repository (Debian, Ubuntu, Mint) from the release's .deb files.
#
# Layout: <out>/apt/pool/main/m/mumcord/*.deb and <out>/apt/dists/stable/{InRelease,Release,
# Release.gpg,main/binary-<arch>/Packages[.gz]}. apt checks InRelease (or Release + Release.gpg)
# against the key named in the user's Signed-By, and every file below it against the hashes in
# there, so a changed package or index is rejected.
#
# Usage: GNUPGHOME=… build-apt.sh <fingerprint> <version> <package dir> <out dir>
# Needs: apt-ftparchive (apt-utils), gpg.
set -euo pipefail

fpr=$1 version=$2 pkgs=$3 out=$4
apt=$out/apt
pool=pool/main/m/mumcord

install -d "$apt/$pool"
for arch in amd64 arm64; do
  cp "$pkgs/Mumcord_${version}_${arch}.deb" "$apt/$pool/"
done

cd "$apt"
for arch in amd64 arm64; do
  dir=dists/stable/main/binary-$arch
  install -d "$dir"
  apt-ftparchive --arch "$arch" packages "$pool" > "$dir/Packages"
  gzip -9nk "$dir/Packages"
  grep -q "^Package: " "$dir/Packages" || { echo "::error::No $arch package in the APT index." >&2; exit 1; }
done

# Written outside dists/stable first: apt-ftparchive would otherwise hash a half-written Release.
release=$(mktemp)
apt-ftparchive \
  -o APT::FTPArchive::Release::Origin=Mumcord \
  -o APT::FTPArchive::Release::Label=Mumcord \
  -o APT::FTPArchive::Release::Suite=stable \
  -o APT::FTPArchive::Release::Codename=stable \
  -o "APT::FTPArchive::Release::Architectures=amd64 arm64" \
  -o APT::FTPArchive::Release::Components=main \
  -o "APT::FTPArchive::Release::Description=Mumcord stable releases" \
  release dists/stable > "$release"
mv "$release" dists/stable/Release
chmod 644 dists/stable/Release

gpg --batch --yes --local-user "$fpr" --digest-algo SHA512 --clearsign \
  -o dists/stable/InRelease dists/stable/Release
gpg --batch --yes --local-user "$fpr" --digest-algo SHA512 --armor --detach-sign \
  -o dists/stable/Release.gpg dists/stable/Release
gpg --batch --verify dists/stable/InRelease 2>/dev/null
