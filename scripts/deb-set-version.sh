#!/bin/bash
# Rewrites the package version of a .deb in place. Release candidates need it: Tauri writes the
# app version 0.8.0-rc.1 as is, which dpkg reads as upstream 0.8.0 with Debian revision "rc.1" and
# sorts AFTER the final 0.8.0, so apt would refuse the upgrade. 0.8.0~rc.1 sorts before it.
#   scripts/deb-set-version.sh <package.deb> <deb version>
# Only the Version field changes; the files, their modes and the other control fields stay.
set -euo pipefail
deb=${1:?package missing}
version=${2:?version missing, e.g. 0.8.0~rc.1}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

dpkg-deb --raw-extract "$deb" "$work/root"
VERSION=$version perl -pi -e 's/^Version: .*$/Version: $ENV{VERSION}/ and $done = 1;
  END { die "no Version field\n" unless $done }' "$work/root/DEBIAN/control"
# Tauri builds the package as root:root; the extraction above belongs to the runner's user.
# xz, not Ubuntu's default zstd, which dpkg before Debian 12 cannot read.
dpkg-deb --root-owner-group -Zxz --build "$work/root" "$work/new.deb" >/dev/null
mv "$work/new.deb" "$deb"
echo "$(basename "$deb"): version $(dpkg-deb --field "$deb" Version)"
