#!/usr/bin/env bash
# Prints a directory holding only GStreamer's helper binaries (gst-plugin-scanner, gst-ptp-helper)
# for linuxdeploy-plugin-gstreamer. It copies every file of GSTREAMER_HELPERS_DIR into the
# AppImage and only guesses the Debian path on its own, so builds on Arch, Fedora or openSUSE
# shipped without a scanner: the web process then warns "External plugin loader failed" for
# every plugin. Pointing it at the plugin folder itself would copy all plugins, libav included.
# Usage:
#   echo "GSTREAMER_HELPERS_DIR=$(scripts/gst-helpers-dir.sh)" >> "$GITHUB_ENV"
set -euo pipefail

candidates=(
  "/usr/lib/$(uname -m)-linux-gnu/gstreamer1.0/gstreamer-1.0" # Debian, Ubuntu
  /usr/libexec/gstreamer-1.0                                  # Fedora, openSUSE
  /usr/lib/gstreamer-1.0                                      # Arch
)
for dir in "${candidates[@]}"; do
  [ -x "$dir/gst-plugin-scanner" ] || continue
  out="$(mktemp -d)"
  for helper in gst-plugin-scanner gst-ptp-helper; do
    [ -e "$dir/$helper" ] && ln -s "$dir/$helper" "$out/$helper"
  done
  echo "$out"
  exit 0
done
echo "gst-plugin-scanner not found in: ${candidates[*]}" >&2
exit 1
