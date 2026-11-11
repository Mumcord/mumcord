#!/usr/bin/env bash
# Builds the H.264 and AAC decoders that every Linux package of Mumcord bundles for video playback
# in the webview (Twitch, .mp4 links): an LGPL-only, decoders-only FFmpeg and the GStreamer libav
# plugin (gst-libav) built against it and against the build host's GStreamer.
#
#   scripts/build-ffmpeg.sh <prefix>
#
# Result, laid out like /usr/lib/mumcord in the .deb and .rpm (src-tauri/tauri.ffmpeg.linux.json):
#   <prefix>/libavutil.so.N, libavcodec.so.N, libavformat.so.N, libavfilter.so.N  (RUNPATH $ORIGIN)
#   <prefix>/gstreamer-1.0/libgstlibav.so                                         (RUNPATH $ORIGIN/..)
# An existing result is kept, so CI can restore <prefix> from its cache and call this script
# unconditionally.
#
# FFmpeg is configured without --enable-gpl and --enable-nonfree, so it stays LGPL-2.1-or-later,
# and with --disable-everything, so only the decoders and parsers listed below are built: no
# encoders, no demuxers, no network protocols, no programs. gst-libav must be built for the
# GStreamer minor version it is loaded into; the bundles use Ubuntu 24.04's GStreamer 1.24
# (RELEASING.md), so this script refuses any other. gst-libav 1.24 does not build with FFmpeg 7,
# so FFmpeg stays on its maintained 6.1 branch, the one Ubuntu 24.04 pairs with gst-libav 1.24.
# The .so files run on newer GStreamer too (the .deb and .rpm load the plugin into the system's
# GStreamer). Needs a C compiler, make, pkg-config, meson,
# ninja, patchelf, the GStreamer development files (libgstreamer1.0-dev,
# libgstreamer-plugins-base1.0-dev) and, on x86_64, nasm for FFmpeg's assembly.
set -euo pipefail

FFMPEG_VERSION=6.1.6
FFMPEG_SHA256=d4fcb164028dd3beee5d92c0ac72e46aac6973c75ea12dc14de07bf8f407370a
GST_LIBAV_VERSION=1.24.2
GST_LIBAV_SHA256=962838648e14ce8a78322ab16f84c7d84d86a6d7bebb6574ac05dea84a7c7c9b

# One flag per line: scripts/notices.js prints them in the licence notices, as the LGPL asks.
# shellcheck disable=SC2054 # the commas separate FFmpeg's component lists, not array elements
ffmpeg_configure=(
  --disable-gpl
  --disable-nonfree
  --disable-version3
  --enable-shared
  --disable-static
  --disable-programs
  --disable-doc
  --disable-autodetect
  --disable-network
  --disable-avdevice
  --disable-swscale
  --disable-swresample
  --disable-postproc
  --disable-everything
  --enable-decoder=h264,aac,aac_latm
  --enable-parser=h264,aac,aac_latm
  --disable-debug
)

prefix=${1:?usage: build-ffmpeg.sh <prefix>}
mkdir -p "$prefix"
prefix=$(cd "$prefix" && pwd)

if [[ -f "$prefix/gstreamer-1.0/libgstlibav.so" ]]; then
  echo "FFmpeg and gst-libav already present in $prefix"
  exit 0
fi

gst_version=$(pkg-config --modversion gstreamer-1.0)
if [[ "${gst_version%.*}" != "${GST_LIBAV_VERSION%.*}" ]]; then
  echo "build-ffmpeg.sh: GStreamer $gst_version found, gst-libav $GST_LIBAV_VERSION needs ${GST_LIBAV_VERSION%.*}" >&2
  exit 1
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# fetch <url> <sha256>: downloads and unpacks a pinned source tarball into $work.
fetch() {
  local url=$1 sha256=$2 file
  file="$work/$(basename "$url")"
  curl -fsSL -o "$file" "$url"
  echo "$sha256  $file" | sha256sum -c --quiet -
  tar -xJf "$file" -C "$work"
}
fetch "https://ffmpeg.org/releases/ffmpeg-$FFMPEG_VERSION.tar.xz" "$FFMPEG_SHA256"
fetch "https://gstreamer.freedesktop.org/src/gst-libav/gst-libav-$GST_LIBAV_VERSION.tar.xz" "$GST_LIBAV_SHA256"

install="$work/install"
jobs=$(nproc)

(
  cd "$work/ffmpeg-$FFMPEG_VERSION"
  ./configure --prefix="$install" --libdir="$install/lib" "${ffmpeg_configure[@]}"
  make -j"$jobs"
  make install
)

# The build must not have picked up anything that changes the licence.
config_h="$work/ffmpeg-$FFMPEG_VERSION/config.h"
for flag in CONFIG_GPL CONFIG_NONFREE CONFIG_VERSION3; do
  grep -qx "#define $flag 0" "$config_h" || { echo "build-ffmpeg.sh: $flag is not 0" >&2; exit 1; }
done

PKG_CONFIG_PATH="$install/lib/pkgconfig" meson setup "$work/gst-libav-build" "$work/gst-libav-$GST_LIBAV_VERSION" \
  --prefix="$install" --libdir=lib --buildtype=release -Ddoc=disabled -Dtests=disabled \
  -Dpackage-name="Mumcord gst-libav" -Dpackage-origin="https://github.com/Mumcord/mumcord"
meson compile -C "$work/gst-libav-build"
meson install -C "$work/gst-libav-build"

# Only the files under their soname (libavcodec.so.61), without the development symlinks.
mkdir -p "$prefix/gstreamer-1.0"
for lib in avutil avcodec avformat avfilter; do
  soname=$(objdump -p "$install/lib/lib$lib.so" | awk '$1 == "SONAME" { print $2 }')
  cp -L "$install/lib/$soname" "$prefix/$soname"
  strip --strip-unneeded "$prefix/$soname"
  # shellcheck disable=SC2016 # $ORIGIN is for the dynamic loader, not the shell
  patchelf --set-rpath '$ORIGIN' "$prefix/$soname"
done
plugin="$prefix/gstreamer-1.0/libgstlibav.so"
cp -L "$install/lib/gstreamer-1.0/libgstlibav.so" "$plugin"
strip --strip-unneeded "$plugin"
# shellcheck disable=SC2016 # $ORIGIN is for the dynamic loader, not the shell
patchelf --set-rpath '$ORIGIN/..' "$plugin"

echo "FFmpeg $FFMPEG_VERSION and gst-libav $GST_LIBAV_VERSION installed to $prefix"
