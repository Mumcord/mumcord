#!/usr/bin/env bash
# Builds the static SVT-AV1 encoder that screen sharing uses for software AV1 (crates/screenshare
# picks it up through SVTAV1_STATIC_DIR).
#
#   scripts/build-svt-av1.sh <prefix>
#
# Result: <prefix>/include/svt-av1/*.h, <prefix>/lib/SvtAv1Enc.lib (Windows) or
# <prefix>/lib/libSvtAv1Enc.a (macOS: one universal archive with arm64 and x86_64 slices; Linux:
# the host architecture), and <prefix>/lib/pkgconfig/SvtAv1Enc.pc, which locates itself, so the
# prefix can move. An existing result is kept, so CI can restore <prefix> from its cache and call
# this script unconditionally.
#
# Needs cmake and, for x86_64 assembly, nasm 2.14 or newer. Windows builds with MSVC (the Visual
# Studio generator) on the dynamic C runtime, like Rust's MSVC target. Only the library is built,
# without the command-line apps.
set -euo pipefail

SVT_AV1_VERSION=4.2.0
# Matches the oldest macOS that Rust's aarch64 target supports, so the linker does not warn
# about objects built for a newer system.
MACOS_DEPLOYMENT_TARGET=11.0

prefix=${1:?usage: build-svt-av1.sh <prefix>}
mkdir -p "$prefix"
prefix=$(cd "$prefix" && pwd)

case "$(uname -s)" in
  MINGW* | MSYS* | CYGWIN*) os=windows; lib=SvtAv1Enc.lib ;;
  Darwin) os=macos; lib=libSvtAv1Enc.a ;;
  Linux) os=linux; lib=libSvtAv1Enc.a ;;
  *) echo "build-svt-av1.sh: unsupported system $(uname -s)" >&2; exit 1 ;;
esac

if [[ -f "$prefix/lib/$lib" && -f "$prefix/include/svt-av1/EbSvtAv1Enc.h" ]]; then
  echo "SVT-AV1 already present in $prefix"
  exit 0
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

curl -fsSL --retry 5 --retry-delay 5 "https://gitlab.com/AOMediaCodec/SVT-AV1/-/archive/v$SVT_AV1_VERSION/SVT-AV1-v$SVT_AV1_VERSION.tar.gz" \
  | tar -xz -C "$work"
src="$work/SVT-AV1-v$SVT_AV1_VERSION"

# Link-time optimisation is off because it would put compiler-specific bitcode into the archive,
# which Rust's linker cannot read.
options=(
  -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF -DBUILD_APPS=OFF -DBUILD_TESTING=OFF
  -DSVT_AV1_LTO=OFF -DCMAKE_POSITION_INDEPENDENT_CODE=ON -DCMAKE_INSTALL_LIBDIR=lib
)

# build <build dir> <install dir> [extra cmake arguments]
build() {
  local dir=$1 install=$2
  shift 2
  cmake -S "$src" -B "$dir" -DCMAKE_INSTALL_PREFIX="$install" "${options[@]}" "$@"
  cmake --build "$dir" --config Release --parallel
  cmake --install "$dir" --config Release
}

if [[ $os == windows ]]; then
  # cmake and MSVC are native Windows programs and do not understand MSYS paths like /tmp/....
  build "$(cygpath -m "$work/build")" "$(cygpath -m "$work/install")" \
    -A x64 -DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreadedDLL \
    -DCMAKE_POLICY_DEFAULT_CMP0091=NEW
  install="$work/install"
elif [[ $os == linux ]]; then
  build "$work/build" "$work/install"
  install="$work/install"
else
  for arch in arm64 x86_64; do
    # CMAKE_OSX_ARCHITECTURES lets the arm64 runner build the x86_64 half.
    build "$work/build-$arch" "$work/install-$arch" -DCMAKE_OSX_ARCHITECTURES="$arch" \
      -DCMAKE_OSX_DEPLOYMENT_TARGET="$MACOS_DEPLOYMENT_TARGET"
  done
  install="$work/install-arm64"
  lipo -create "$work/install-arm64/lib/libSvtAv1Enc.a" "$work/install-x86_64/lib/libSvtAv1Enc.a" \
    -output "$work/libSvtAv1Enc.a"
  mv "$work/libSvtAv1Enc.a" "$install/lib/libSvtAv1Enc.a"
  lipo -info "$install/lib/libSvtAv1Enc.a"
fi

mkdir -p "$prefix/include" "$prefix/lib/pkgconfig"
# The installed headers do not depend on the architecture.
cp -R "$install/include/svt-av1" "$prefix/include/"
cp "$install/lib/$lib" "$prefix/lib/"
# shellcheck disable=SC2016 # ${pcfiledir} is for pkg-config, not the shell
sed 's|^prefix=.*|prefix=${pcfiledir}/../..|' "$install/lib/pkgconfig/SvtAv1Enc.pc" \
  > "$prefix/lib/pkgconfig/SvtAv1Enc.pc"

echo "SVT-AV1 $SVT_AV1_VERSION installed to $prefix"
