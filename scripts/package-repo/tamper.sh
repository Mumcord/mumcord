#!/bin/bash
# Makes two damaged copies of a built repository, for verify.sh: <bad-meta> with every signed
# index changed after signing, <bad-pkg> with every package changed after signing (one byte
# appended). The package managers must reject both.
#
# Usage: tamper.sh <site> <bad-meta out> <bad-pkg out>
set -euo pipefail

site=$1 meta=$2 pkg=$3
cp -a "$site" "$meta"
cp -a "$site" "$pkg"

# APT: change the first hash lines inside the signed text of InRelease and Release.
for f in "$meta/apt/dists/stable/InRelease" "$meta/apt/dists/stable/Release"; do
  sed -i "0,/^ [0-9a-f]\{64\} /s/^ [0-9a-f]/ x/" "$f"
  cmp -s "$f" "$site/apt/dists/stable/${f##*/}" && { echo "::error::$f was not changed" >&2; exit 1; }
done
# RPM: the signed repomd.xml gains a comment; repomd.xml.asc no longer matches.
echo '<!-- changed after signing -->' >> "$meta/rpm/repodata/repomd.xml"
# pacman: the signed database gains a byte; mumcord.db.sig no longer matches.
for db in "$meta"/arch/*/mumcord.db; do
  printf '\0' >> "$db"
done

while IFS= read -r -d '' f; do
  printf '\0' >> "$f"
done < <(find "$pkg" \( -name '*.deb' -o -name '*.rpm' -o -name '*.pkg.tar.zst' \) -print0)
