#!/usr/bin/env bash
# Prints a directory of symlinks to the system GStreamer plugins minus the ones the AppImage must
# not ship. linuxdeploy-plugin-gstreamer (run by Tauri's bundleMediaFramework) copies every file
# from GSTREAMER_PLUGINS_DIR and bundles their libraries, so leaving a plugin out here also keeps
# its libraries (libavcodec, libx264, ...) out of the AppImage. Usage:
#   echo "GSTREAMER_PLUGINS_DIR=$(scripts/filter-gst-plugins.sh)" >> "$GITHUB_ENV"
set -euo pipefail

src="${1:-/usr/lib/$(uname -m)-linux-gnu/gstreamer-1.0}"
[ -d "$src" ] || src=/usr/lib/gstreamer-1.0
[ -d "$src" ] || { echo "GStreamer plugin directory not found: $src" >&2; exit 1; }

# H.264/H.265 (patent-encumbered) and GPL-only codecs. VP8/VP9 (libgstvpx) stays: YouTube plays via VP9.
excluded=(
  libgstlibav.so libgstx264.so libgstx265.so libgstopenh264.so
  libgsta52dec.so libgstdtsdec.so libgstmpeg2dec.so libgstmpeg2enc.so libgstmplex.so
  libgstdvdread.so libgstdvdlpcmdec.so libgstdvdsub.so libgstresindvd.so
  libgstfaac.so libgstfaad.so
)

out="$(mktemp -d)"
for f in "$src"/*; do
  name="$(basename "$f")"
  skip=0
  for e in "${excluded[@]}"; do [ "$name" = "$e" ] && skip=1; done
  [ "$skip" = 1 ] || ln -s "$f" "$out/$name"
done
echo "$out"
