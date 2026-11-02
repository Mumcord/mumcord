#!/usr/bin/env bash
# Builds a minimal static FFmpeg (libavcodec and libavutil only) whose sole job is to reach the
# graphics card's AV1 and VP9 encoders for screen sharing (crates/screenshare picks it up through
# FFMPEG_STATIC_DIR). macOS has no such encoders, so it gets no FFmpeg and this script refuses.
#
#   scripts/build-ffmpeg.sh <prefix>
#
# Result: <prefix>/include/{libavcodec,libavutil}/*.h, the static libraries in <prefix>/lib
# (Windows: avcodec.lib, avutil.lib and vpl.lib; Linux: libavcodec.a, libavutil.a), and
# <prefix>/lib/pkgconfig/*.pc, which locate themselves, so the prefix can move; their
# Libs.private name what a final link needs. An existing result is kept, so CI can restore
# <prefix> from its cache and call this script unconditionally.
#
# Licence: the build is LGPL-2.1-or-later. It never enables --enable-gpl, --enable-version3 or
# --enable-nonfree, and contains no codec except the encoders below (in particular no H.264):
#   Windows: av1_nvenc, av1_amf, av1_qsv, vp9_qsv
#   Linux:   av1_nvenc, av1_amf, av1_vaapi, vp9_vaapi
# NVENC and AMF need only headers (MIT, downloaded below); the vendor drivers are loaded at
# runtime. Quick Sync needs Intel's libvpl dispatcher (MIT), built here and linked statically on
# Windows. VA-API links the system's libva at runtime (libva.so.2 and libva-drm.so.2), so Linux
# packages depend on libva.
#
# Windows builds with MSVC (--toolchain=msvc) on the dynamic C runtime, like Rust's MSVC target:
# run it from an MSYS2 bash (make, nasm, pkgconf) inside a Visual Studio developer environment
# (cl.exe and link.exe on PATH), with cmake on PATH for libvpl. Linux needs make, nasm, pkg-config
# and the libva development files (Debian/Ubuntu: libva-dev).
set -euo pipefail

FFMPEG_VERSION=9.0.2
# The oldest headers FFmpeg accepts that contain AV1: older NVIDIA drivers keep working
# (NVENC SDK 12.1 needs driver 531.61 on Windows, 530.41 on Linux).
NV_CODEC_HEADERS_VERSION=12.1.14.0
# FFmpeg's AMF encoders need at least AMF 1.5.2 headers.
AMF_VERSION=1.5.3
LIBVPL_VERSION=2.17.0

prefix=${1:?usage: build-ffmpeg.sh <prefix>}

case "$(uname -s)" in
  MINGW* | MSYS* | CYGWIN*) os=windows; lib=avcodec.lib ;;
  Linux) os=linux; lib=libavcodec.a ;;
  Darwin) echo "build-ffmpeg.sh: macOS has no AV1 or VP9 hardware encoder; nothing to build" >&2; exit 1 ;;
  *) echo "build-ffmpeg.sh: unsupported system $(uname -s)" >&2; exit 1 ;;
esac

mkdir -p "$prefix"
prefix=$(cd "$prefix" && pwd)

if [[ -f "$prefix/lib/$lib" && -f "$prefix/include/libavcodec/avcodec.h" ]]; then
  echo "FFmpeg already present in $prefix"
  exit 0
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
# Build-time dependencies (headers, libvpl) go here; FFmpeg's .pc files refer to it until they
# are rewritten at the end.
deps="$work/deps"
mkdir -p "$deps/include"

curl -fsSL "https://ffmpeg.org/releases/ffmpeg-$FFMPEG_VERSION.tar.xz" | tar -xJ -C "$work"
curl -fsSL "https://github.com/FFmpeg/nv-codec-headers/archive/refs/tags/n$NV_CODEC_HEADERS_VERSION.tar.gz" \
  | tar -xz -C "$work"
curl -fsSL "https://github.com/GPUOpen-LibrariesAndSDKs/AMF/releases/download/v$AMF_VERSION/AMF-headers-v$AMF_VERSION.tar.gz" \
  | tar -xz -C "$work"

# MSVC reads the include paths that pkg-config hands out, so on Windows they must be Windows paths.
deps_pc=$deps
if [[ $os == windows ]]; then
  deps_pc=$(cygpath -m "$deps")
  # MSYS2's coreutils has its own `link`, which would shadow MSVC's linker of the same name.
  cl=$(command -v cl) || { echo "build-ffmpeg.sh: cl.exe not on PATH (run inside a Visual Studio developer environment)" >&2; exit 1; }
  PATH="$(dirname "$cl"):$PATH"
fi
make -C "$work/nv-codec-headers-n$NV_CODEC_HEADERS_VERSION" PREFIX="$deps_pc" install
cp -R "$work/amf-headers-v$AMF_VERSION/AMF" "$deps/include/"

configure=(
  --prefix="$work/install"
  --disable-everything --disable-autodetect
  --disable-programs --disable-doc --disable-network --disable-debug --disable-iconv
  --disable-avformat --disable-avfilter --disable-avdevice --disable-swscale --disable-swresample
  --enable-static --disable-shared --enable-pic
  --enable-ffnvcodec --enable-cuda --enable-nvenc --enable-amf
  "--enable-encoder=av1_nvenc,av1_amf"
  --extra-cflags="-I$deps_pc/include"
)

if [[ $os == windows ]]; then
  curl -fsSL "https://github.com/intel/libvpl/archive/refs/tags/v$LIBVPL_VERSION.tar.gz" | tar -xz -C "$work"
  # cmake and MSVC are native Windows programs and do not understand MSYS paths like /tmp/....
  cmake -S "$(cygpath -m "$work/libvpl-$LIBVPL_VERSION")" -B "$(cygpath -m "$work/libvpl-build")" \
    -A x64 -DBUILD_SHARED_LIBS=OFF -DBUILD_TESTS=OFF \
    -DUSE_MSVC_STATIC_RUNTIME=OFF -DCMAKE_INSTALL_PREFIX="$deps_pc" \
    -DCMAKE_INSTALL_LIBDIR=lib
  cmake --build "$(cygpath -m "$work/libvpl-build")" --config Release --parallel
  cmake --install "$(cygpath -m "$work/libvpl-build")" --config Release
  configure+=(
    --toolchain=msvc --target-os=win64 --arch=x86_64 --extra-cflags=-MD
    --enable-d3d11va --enable-libvpl "--enable-encoder=av1_qsv,vp9_qsv"
  )
else
  configure+=(--enable-vaapi "--enable-encoder=av1_vaapi,vp9_vaapi")
fi

src="$work/ffmpeg-$FFMPEG_VERSION"
mkdir "$work/build"
(cd "$work/build" && PKG_CONFIG_PATH="$deps/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}" \
  "$src/configure" "${configure[@]}" && make -j"$(nproc)" && make install)
install="$work/install"

# Fail the build rather than ship an FFmpeg that silently lost an encoder or gained a codec.
expected="av1_amf av1_nvenc"
if [[ $os == windows ]]; then
  expected="$expected av1_qsv vp9_qsv"
else
  expected="$expected av1_vaapi vp9_vaapi"
fi
actual=$(sed -n 's/^#define CONFIG_\([A-Z0-9_]*\)_\(ENCODER\|DECODER\) 1$/\1_\2/p' "$work/build/config_components.h" \
  | tr '[:upper:]' '[:lower:]' | sed 's/_encoder$//' | sort | tr '\n' ' ' | sed 's/ $//')
if [[ $actual != "$expected" ]]; then
  echo "build-ffmpeg.sh: expected the encoders '$expected' and no decoders, got '$actual'" >&2
  exit 1
fi

mkdir -p "$prefix/include" "$prefix/lib/pkgconfig"
cp -R "$install/include/libavcodec" "$install/include/libavutil" "$prefix/include/"
if [[ $os == windows ]]; then
  # FFmpeg names its static archives libavcodec.a even with MSVC; -lavcodec means avcodec.lib.
  cp "$install/lib/libavcodec.a" "$prefix/lib/avcodec.lib"
  cp "$install/lib/libavutil.a" "$prefix/lib/avutil.lib"
  cp "$deps/lib/vpl.lib" "$prefix/lib/"
  cp "$deps/lib/pkgconfig/vpl.pc" "$prefix/lib/pkgconfig/"
else
  cp "$install/lib/libavcodec.a" "$install/lib/libavutil.a" "$prefix/lib/"
fi
cp "$install/lib/pkgconfig/libavcodec.pc" "$install/lib/pkgconfig/libavutil.pc" "$prefix/lib/pkgconfig/"
# Paths into the temporary build directories become paths relative to the .pc file.
# shellcheck disable=SC2016 # ${pcfiledir} and ${prefix} are for pkg-config, not the shell
sed -i -e 's|^prefix=.*|prefix=${pcfiledir}/../..|' \
  -e 's|^libdir=.*|libdir=${prefix}/lib|' -e 's|^includedir=.*|includedir=${prefix}/include|' \
  -e "s|$deps_pc/lib|\${prefix}/lib|g" \
  "$prefix"/lib/pkgconfig/*.pc

echo "FFmpeg $FFMPEG_VERSION installed to $prefix (encoders: $expected)"
