#!/usr/bin/env bash
# Prints the native libraries of this build host that go into the bundles, for the SBOM
# (scripts/generate-sbom.sh). The release build job runs it after the build and uploads the
# output; the SBOM is assembled later on another runner, which cannot see this host. Usage:
#   scripts/sbom-native.sh [libvpx-version] > sbom-native-<build>.txt
# The libvpx version defaults to the one scripts/build-libvpx.sh builds; Windows passes vcpkg's.
# It adds SVT-AV1, dav1d and, except on macOS, the hardware encoder interfaces at the versions of
# their build scripts. On Linux it adds what the AppImage bundles from the build host: WebKitGTK, GStreamer, and the
# plugins in GSTREAMER_PLUGINS_DIR (scripts/filter-gst-plugins.sh).
set -euo pipefail

libvpx="${1:-$(sed -n 's/^LIBVPX_VERSION=//p' "$(dirname "$0")/build-libvpx.sh")}"
[ -n "$libvpx" ] || { echo "libvpx version not found" >&2; exit 1; }
echo "libvpx $libvpx"
version() { sed -n "s/^$1=//p" "$(dirname "$0")/$2"; }
echo "svt-av1 $(version SVT_AV1_VERSION build-svt-av1.sh)"
echo "dav1d $(version DAV1D_VERSION build-dav1d.sh)"
# macOS has no hardware encoder, so it gets none of the hardware encoder interfaces; the VA-API
# headers are Linux only (scripts/build-hwenc-sdk.sh).
if [ "$(uname -s)" != Darwin ]; then
  echo "nv-codec-headers $(version NV_CODEC_HEADERS_VERSION build-hwenc-sdk.sh)"
  echo "amf-headers $(version AMF_VERSION build-hwenc-sdk.sh)"
  echo "libvpl $(version LIBVPL_VERSION build-hwenc-sdk.sh)"
fi
if [ "$(uname -s)" = Linux ]; then
  echo "libva-headers $(version LIBVA_VERSION build-hwenc-sdk.sh)"
fi

if [ "$(uname -s)" = Linux ]; then
  [ -d "${GSTREAMER_PLUGINS_DIR:-}" ] || { echo "GSTREAMER_PLUGINS_DIR is not set" >&2; exit 1; }
  echo "webkitgtk $(dpkg-query -W -f='${Version}' libwebkit2gtk-4.1-0)"
  echo "gstreamer $(dpkg-query -W -f='${Version}' libgstreamer1.0-0)"
  for f in "$GSTREAMER_PLUGINS_DIR"/*.so; do
    echo "gst-plugin $(basename "$f")"
  done
fi
