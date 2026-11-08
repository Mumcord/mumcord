#!/bin/bash
# Tests scripts/deb-set-version.sh on a small package shaped like Tauri's: the version changes,
# the files and their modes stay, and the release candidate sorts before the final release.
set -euo pipefail
script=$(cd "$(dirname "$0")/.." && pwd)/deb-set-version.sh
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
fail() { echo "FAIL: $1" >&2; exit 1; }

mkdir -p "$work/root/DEBIAN" "$work/root/usr/bin"
cat > "$work/root/DEBIAN/control" <<'EOF'
Package: mumcord
Version: 0.8.0-rc.1
Architecture: amd64
Maintainer: test
Description: test package
EOF
printf '#!/bin/sh\n' > "$work/root/usr/bin/mumcord"
chmod 755 "$work/root/usr/bin/mumcord"
deb=$work/Mumcord_0.8.0-rc.1_amd64.deb
dpkg-deb --root-owner-group --build "$work/root" "$deb" >/dev/null

"$script" "$deb" 0.8.0~rc.1 >/dev/null

[[ "$(dpkg-deb --field "$deb" Version)" == "0.8.0~rc.1" ]] || fail "Version not rewritten"
[[ "$(dpkg-deb --field "$deb" Package)" == "mumcord" ]] || fail "other control fields lost"
listing=$(dpkg-deb --contents "$deb")
grep -q '^-rwxr-xr-x root/root .* ./usr/bin/mumcord$' <<<"$listing" \
  || fail "file mode or owner changed: $listing"
dpkg --compare-versions 0.8.0~rc.1 lt 0.8.0 || fail "the release candidate does not sort first"
dpkg --compare-versions 0.8.0~rc.1 lt 0.8.0~rc.2 || fail "release candidates out of order"

echo "deb-set-version.sh: version rewritten, contents kept."
