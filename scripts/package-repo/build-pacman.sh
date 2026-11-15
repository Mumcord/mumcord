#!/bin/bash
# Builds the signed pacman repository (Arch Linux and derivatives) from the release's .deb files,
# with the same PKGBUILD as the AUR package mumcord-bin, so AUR users who add the repository get
# their updates from it.
#
# Layout: <out>/arch/<arch>/{mumcord.db,mumcord.files,…,*.pkg.tar.zst,*.sig}. Packages and the
# database are both signed; pacman's default SigLevel checks the package signature and the
# database signature when present.
#
# Usage (as root, in an Arch Linux container): GNUPGHOME=… build-pacman.sh <fingerprint>
#   <version> <package dir with the .debs and SHA256SUMS> <out dir>
# Needs: makepkg, fakeroot, repo-add, gpg.
set -euo pipefail

fpr=$1 version=$2 pkgs=$3 out=$4
root=$(cd "$(dirname "$0")/../.." && pwd)

sum() { awk -v f="$1" '$2 == f || $2 == "*" f { print $1 }' "$pkgs/SHA256SUMS"; }
amd64=$(sum "Mumcord_${version}_amd64.deb")
arm64=$(sum "Mumcord_${version}_arm64.deb")
work=$(mktemp -d)
cp "$pkgs/Mumcord_${version}_amd64.deb" "$pkgs/Mumcord_${version}_arm64.deb" "$work/"
cp "$root/LICENSE" "$work/LICENSE-$version"
license=$(sha256sum "$work/LICENSE-$version" | cut -d' ' -f1)
for s in "$amd64" "$arm64"; do
  [[ "$s" =~ ^[0-9a-f]{64}$ ]] || { echo "::error::Missing checksum in SHA256SUMS." >&2; exit 1; }
done
sed \
  -e "s/^pkgver=.*/pkgver=$version/" \
  -e "s/^pkgrel=.*/pkgrel=1/" \
  -e "s/^sha256sums_x86_64=.*/sha256sums_x86_64=('$amd64')/" \
  -e "s/^sha256sums_aarch64=.*/sha256sums_aarch64=('$arm64')/" \
  -e "s/^sha256sums=.*/sha256sums=('$license')/" \
  "$root/packaging/aur/PKGBUILD" > "$work/PKGBUILD"

# makepkg refuses to run as root. The package only repackages the .deb, so one machine builds
# both architectures by setting CARCH; --cleanbuild drops the other architecture's .deb from
# src/, and --nodeps skips the runtime dependencies, which building does not need.
id builder >/dev/null 2>&1 || useradd -m builder
chown -R builder "$work"
for carch in x86_64 aarch64; do
  sed "s/^CARCH=.*/CARCH=\"$carch\"/" /etc/makepkg.conf > "$work/makepkg-$carch.conf"
  su builder -c "cd '$work' && PKGDEST='$work/out-$carch' makepkg --config makepkg-$carch.conf --cleanbuild --nodeps --noconfirm --noprogressbar >/dev/null"

  dir=$out/arch/$carch
  install -d "$dir"
  cp "$work/out-$carch"/*.pkg.tar.zst "$dir/"
  for p in "$dir"/*.pkg.tar.zst; do
    gpg --batch --yes --local-user "$fpr" --detach-sign -o "$p.sig" "$p"
  done
  repo-add --quiet --verify --sign --key "$fpr" "$dir/mumcord.db.tar.gz" "$dir"/*.pkg.tar.zst
  # repo-add links mumcord.db to mumcord.db.tar.gz; static hosting serves files, not links.
  for link in "$dir"/*; do
    [[ -L "$link" ]] && cp --remove-destination "$(readlink -f "$link")" "$link"
  done
done
rm -rf "$work"
