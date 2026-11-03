#!/usr/bin/env bash
# Collects what screen sharing needs to talk to the graphics card's AV1 and VP9 encoders directly
# (crates/screenshare picks it up through HWENC_SDK_DIR). macOS has no such encoders, so this
# script refuses to run there.
#
#   scripts/build-hwenc-sdk.sh <prefix>
#
# Result (all MIT licensed):
#   <prefix>/include/ffnvcodec/  NVIDIA NVENC API headers (nv-codec-headers); the driver's
#                                nvEncodeAPI64.dll / libnvidia-encode.so.1 is loaded at runtime
#   <prefix>/include/AMF/        AMD AMF headers; amfrt64.dll / libamfrt64.so.1 is loaded at runtime
#   <prefix>/include/vpl/        Intel VPL (Quick Sync) headers and its static dispatcher,
#   <prefix>/lib/vpl.lib         which finds Intel's runtime at runtime; the dispatcher is C++, so
#   <prefix>/lib/libvpl.a        the final link needs the C++ standard library (Linux: libstdc++)
#   <prefix>/include/va/         Linux only: VA-API headers (libva), for loading the system's
#                                libva.so.2 and libva-drm.so.2 at runtime
# and <prefix>/lib/pkgconfig/vpl.pc, which locates itself, so the prefix can move. An existing
# result is kept, so CI can restore <prefix> from its cache and call this script unconditionally.
#
# Needs cmake (and make on Linux). Windows builds the dispatcher with MSVC (the Visual Studio
# generator) on the dynamic C runtime, like Rust's MSVC target.
set -euo pipefail

# The oldest NVENC API with AV1 temporal layers (SDK 13.0, driver 570 or newer). With an older
# driver NVENC reports itself unavailable and screen sharing falls back to the software encoders.
NV_CODEC_HEADERS_VERSION=13.0.19.0
AMF_VERSION=1.5.3
LIBVPL_VERSION=2.17.0
# VA-API 1.22. Headers only: the code checks the runtime's version before using newer calls.
LIBVA_VERSION=2.22.0

prefix=${1:?usage: build-hwenc-sdk.sh <prefix>}

case "$(uname -s)" in
  MINGW* | MSYS* | CYGWIN*) os=windows; lib=vpl.lib ;;
  Linux) os=linux; lib=libvpl.a ;;
  Darwin) echo "build-hwenc-sdk.sh: macOS has no AV1 or VP9 hardware encoder; nothing to build" >&2; exit 1 ;;
  *) echo "build-hwenc-sdk.sh: unsupported system $(uname -s)" >&2; exit 1 ;;
esac

mkdir -p "$prefix"
prefix=$(cd "$prefix" && pwd)

if [[ -f "$prefix/lib/$lib" && -f "$prefix/include/ffnvcodec/nvEncodeAPI.h" ]]; then
  echo "Hardware encoder SDK already present in $prefix"
  exit 0
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

curl -fsSL "https://github.com/FFmpeg/nv-codec-headers/archive/refs/tags/n$NV_CODEC_HEADERS_VERSION.tar.gz" \
  | tar -xz -C "$work"
curl -fsSL "https://github.com/GPUOpen-LibrariesAndSDKs/AMF/releases/download/v$AMF_VERSION/AMF-headers-v$AMF_VERSION.tar.gz" \
  | tar -xz -C "$work"
curl -fsSL "https://github.com/intel/libvpl/archive/refs/tags/v$LIBVPL_VERSION.tar.gz" | tar -xz -C "$work"

mkdir -p "$prefix/include" "$prefix/lib/pkgconfig"
cp -R "$work/nv-codec-headers-n$NV_CODEC_HEADERS_VERSION/include/ffnvcodec" "$prefix/include/"
cp -R "$work/amf-headers-v$AMF_VERSION/AMF" "$prefix/include/"

vpl_src="$work/libvpl-$LIBVPL_VERSION"
vpl_build="$work/libvpl-build"
install="$work/install"
options=(-DBUILD_SHARED_LIBS=OFF -DBUILD_TESTS=OFF -DCMAKE_INSTALL_LIBDIR=lib)
if [[ $os == windows ]]; then
  # cmake and MSVC are native Windows programs and do not understand MSYS paths like /tmp/....
  vpl_src=$(cygpath -m "$vpl_src")
  vpl_build=$(cygpath -m "$vpl_build")
  install=$(cygpath -m "$install")
  options+=(-A x64 -DUSE_MSVC_STATIC_RUNTIME=OFF)
else
  options+=(-DCMAKE_BUILD_TYPE=Release)
fi
cmake -S "$vpl_src" -B "$vpl_build" -DCMAKE_INSTALL_PREFIX="$install" "${options[@]}"
cmake --build "$vpl_build" --config Release --parallel
cmake --install "$vpl_build" --config Release
cp -R "$work/install/include/vpl" "$prefix/include/"
cp "$work/install/lib/$lib" "$prefix/lib/"
# Paths into the temporary install directory become paths relative to the .pc file.
# shellcheck disable=SC2016 # ${pcfiledir} and ${prefix} are for pkg-config, not the shell
sed -e 's|^prefix=.*|prefix=${pcfiledir}/../..|' \
  -e 's|^libdir=.*|libdir=${prefix}/lib|' -e 's|^includedir=.*|includedir=${prefix}/include|' \
  "$work/install/lib/pkgconfig/vpl.pc" > "$prefix/lib/pkgconfig/vpl.pc"

if [[ $os == linux ]]; then
  curl -fsSL "https://github.com/intel/libva/archive/refs/tags/$LIBVA_VERSION.tar.gz" | tar -xz -C "$work"
  va="$work/libva-$LIBVA_VERSION/va"
  mkdir -p "$prefix/include/va"
  # The public headers of libva and libva-drm; va_version.h is generated from the build's version.
  (cd "$va" && cp va.h va_backend.h va_backend_prot.h va_backend_vpp.h va_compat.h va_dec_*.h \
    va_drmcommon.h va_enc_*.h va_fei*.h va_str.h va_tpi.h va_prot.h va_vpp.h drm/va_drm.h \
    "$prefix/include/va/")
  IFS=. read -r major minor micro < <(sed -n \
    "s/^va_api_\(major\|minor\|micro\)_version = \([0-9]*\)$/\2/p" "$work/libva-$LIBVA_VERSION/meson.build" \
    | paste -sd.)
  sed -e "s/@VA_API_MAJOR_VERSION@/$major/" -e "s/@VA_API_MINOR_VERSION@/$minor/" \
    -e "s/@VA_API_MICRO_VERSION@/$micro/" -e "s/@VA_API_VERSION@/$major.$minor.$micro/" \
    "$va/va_version.h.in" > "$prefix/include/va/va_version.h"
fi

echo "Hardware encoder SDK installed to $prefix"
