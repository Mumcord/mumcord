#!/usr/bin/env bash
# Renders every raster logo asset from the SVG masters in this folder: the Tauri app icons
# (PNG, ICO, ICNS), the tray icon, the UI favicon, the brand PNGs and the social preview.
# Needs rsvg-convert (librsvg), ImageMagick 7 and Python 3. Run it after changing a master.
#
# Masters: mumcord-icon.svg (app icon on its tile), mumcord-logo.svg (no tile, for docs),
# mumcord-icon-small.svg (32 px and below: no cap strap or beanie stripes, which would turn to noise),
# mumcord-tray.svg (one flat colour with an outline, readable on light and dark panels).
# reference.png is the original artwork the masters were traced from.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
icons=$root/src-tauri/icons
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

render() { rsvg-convert -w "$2" -h "$2" "$here/$1.svg" -o "$3"; }
# The simplified mark for small sizes, the full icon above.
variant() { if [ "$1" -le 32 ]; then echo mumcord-icon-small; else echo mumcord-icon; fi; }

render mumcord-icon-small 32 "$icons/32x32.png"
render mumcord-icon 128 "$icons/128x128.png"
render mumcord-icon 256 "$icons/128x128@2x.png"
render mumcord-icon 512 "$icons/icon.png"
render mumcord-tray 64 "$icons/tray.png"
render mumcord-icon 128 "$root/ui/public/favicon.png"
render mumcord-icon 1024 "$here/mumcord-icon.png"
render mumcord-logo 1024 "$here/mumcord-logo.png"
rsvg-convert -w 1280 -h 640 "$here/social-preview.svg" -o "$here/social-preview.png"

for size in 16 24 32 48 64 256; do
    render "$(variant "$size")" "$size" "$work/ico-$size.png"
done
magick "$work"/ico-{16,24,32,48,64,256}.png "$icons/icon.ico"

# ICNS is a list of (type, length, PNG) records; each type is one size.
for size in 16 32 64 128 256 512 1024; do
    render "$(variant "$size")" "$size" "$work/icns-$size.png"
done
python3 -I - "$work" "$icons/icon.icns" <<'EOF'
import struct, sys
work, out = sys.argv[1], sys.argv[2]
types = [("icp4", 16), ("icp5", 32), ("ic11", 32), ("icp6", 64), ("ic12", 64), ("ic07", 128),
         ("ic08", 256), ("ic13", 256), ("ic09", 512), ("ic14", 512), ("ic10", 1024)]
body = b""
for kind, size in types:
    data = open(f"{work}/icns-{size}.png", "rb").read()
    body += kind.encode() + struct.pack(">I", len(data) + 8) + data
open(out, "wb").write(b"icns" + struct.pack(">I", len(body) + 8) + body)
EOF
