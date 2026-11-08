#!/usr/bin/env bash
# Builds the static dav1d AV1 decoder that screen sharing uses when the webview cannot decode AV1
# itself (crates/screenshare picks it up through DAV1D_STATIC_DIR).
#
#   scripts/build-dav1d.sh <prefix>
#
# Result: <prefix>/include/dav1d/*.h, <prefix>/lib/dav1d.lib (Windows) or <prefix>/lib/libdav1d.a
# (macOS: one universal archive with arm64 and x86_64 slices; Linux: the host architecture), and
# <prefix>/lib/pkgconfig/dav1d.pc, which locates itself, so the prefix can move. An existing
# result is kept, so CI can restore <prefix> from its cache and call this script unconditionally.
#
# Needs meson, ninja and, for x86_64 assembly, nasm. Windows builds with MSVC (meson finds Visual
# Studio itself) on the dynamic C runtime, like Rust's MSVC target. Only 8-bit decoding is built:
# screen sharing never sends 10-bit video.
set -euo pipefail

DAV1D_VERSION=1.5.4
# Matches the oldest macOS that Rust's aarch64 target supports, so the linker does not warn
# about objects built for a newer system.
MACOS_DEPLOYMENT_TARGET=11.0

prefix=${1:?usage: build-dav1d.sh <prefix>}
mkdir -p "$prefix"
prefix=$(cd "$prefix" && pwd)

case "$(uname -s)" in
  MINGW* | MSYS* | CYGWIN*) os=windows; lib=dav1d.lib ;;
  Darwin) os=macos; lib=libdav1d.a ;;
  Linux) os=linux; lib=libdav1d.a ;;
  *) echo "build-dav1d.sh: unsupported system $(uname -s)" >&2; exit 1 ;;
esac

if [[ -f "$prefix/lib/$lib" && -f "$prefix/include/dav1d/dav1d.h" ]]; then
  echo "dav1d already present in $prefix"
  exit 0
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

curl -fsSL --retry 5 --retry-delay 5 "https://code.videolan.org/videolan/dav1d/-/archive/$DAV1D_VERSION/dav1d-$DAV1D_VERSION.tar.gz" \
  | tar -xz -C "$work"
src="$work/dav1d-$DAV1D_VERSION"

options=(
  --buildtype=release --default-library=static
  -Dbitdepths=8 -Denable_tools=false -Denable_examples=false -Denable_tests=false
  -Denable_docs=false -Dlogging=false
)

# build <build dir> <install dir> [extra meson arguments]
build() {
  local dir=$1 install=$2
  shift 2
  meson setup "$dir" "$src" --prefix="$install" --libdir=lib "${options[@]}" "$@"
  meson compile -C "$dir"
  meson install -C "$dir"
}

if [[ $os == windows ]]; then
  # meson and MSVC are native Windows programs and do not understand MSYS paths like /tmp/....
  build "$(cygpath -m "$work/build")" "$(cygpath -m "$work/install")" --vsenv -Db_vscrt=md
  install="$work/install"
  # meson names static archives libdav1d.a even with MSVC; the .pc file's -ldav1d means dav1d.lib.
  mv "$install/lib/libdav1d.a" "$install/lib/dav1d.lib"
elif [[ $os == linux ]]; then
  build "$work/build" "$work/install"
  install="$work/install"
else
  for arch in arm64 x86_64; do
    cpu=$arch
    [[ $arch == arm64 ]] && cpu=aarch64
    # A cross file for both halves: the explicit -arch flags let the arm64 runner build x86_64.
    cat > "$work/$arch.ini" <<EOF
[binaries]
c = ['cc', '-arch', '$arch', '-mmacosx-version-min=$MACOS_DEPLOYMENT_TARGET']
ar = 'ar'
strip = 'strip'
nasm = 'nasm'

[host_machine]
system = 'darwin'
cpu_family = '$cpu'
cpu = '$cpu'
endian = 'little'
EOF
    build "$work/build-$arch" "$work/install-$arch" --cross-file="$work/$arch.ini"
  done
  install="$work/install-arm64"
  lipo -create "$work/install-arm64/lib/libdav1d.a" "$work/install-x86_64/lib/libdav1d.a" \
    -output "$install/lib/libdav1d.a.universal"
  mv "$install/lib/libdav1d.a.universal" "$install/lib/libdav1d.a"
  lipo -info "$install/lib/libdav1d.a"
fi

mkdir -p "$prefix/include" "$prefix/lib/pkgconfig"
# The installed headers do not depend on the architecture.
cp -R "$install/include/dav1d" "$prefix/include/"
cp "$install/lib/$lib" "$prefix/lib/"
# shellcheck disable=SC2016 # ${pcfiledir} is for pkg-config, not the shell
sed 's|^prefix=.*|prefix=${pcfiledir}/../..|' "$install/lib/pkgconfig/dav1d.pc" \
  > "$prefix/lib/pkgconfig/dav1d.pc"

echo "dav1d $DAV1D_VERSION installed to $prefix"
