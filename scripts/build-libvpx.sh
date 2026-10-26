#!/usr/bin/env bash
# Builds the static libvpx that the Windows and macOS packages link (crates/screenshare/build.rs
# picks it up through LIBVPX_STATIC_DIR). Linux links the distribution's shared libvpx instead.
#
#   scripts/build-libvpx.sh <prefix>
#
# Result: <prefix>/include/vpx/*.h and <prefix>/lib/vpx.lib (Windows) or <prefix>/lib/libvpx.a
# (macOS, one universal archive with arm64 and x86_64 slices). An existing result is kept, so CI
# can restore <prefix> from its cache and call this script unconditionally.
#
# Windows uses vcpkg (preinstalled on GitHub's runners as VCPKG_INSTALLATION_ROOT) with the
# x64-windows-static-md triplet: static libvpx on the dynamic C runtime, like Rust's MSVC target.
# macOS builds the pinned release below from source; x86_64 assembly needs nasm (Homebrew).
set -euo pipefail

# macOS only; Windows gets the version of vcpkg's port (shown in the CI cache key).
LIBVPX_VERSION=1.15.2
# Matches the oldest macOS that Rust's aarch64 target supports, so the linker does not warn
# about objects built for a newer system.
MACOS_DEPLOYMENT_TARGET=11.0

prefix=${1:?usage: build-libvpx.sh <prefix>}
mkdir -p "$prefix"
prefix=$(cd "$prefix" && pwd)

case "$(uname -s)" in
  MINGW* | MSYS* | CYGWIN*) os=windows; lib=vpx.lib ;;
  Darwin) os=macos; lib=libvpx.a ;;
  *) echo "build-libvpx.sh: only Windows and macOS link a static libvpx" >&2; exit 1 ;;
esac

if [[ -f "$prefix/lib/$lib" && -f "$prefix/include/vpx/vpx_encoder.h" ]]; then
  echo "libvpx already present in $prefix"
  exit 0
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

if [[ $os == windows ]]; then
  vcpkg_root=${VCPKG_INSTALLATION_ROOT:?VCPKG_INSTALLATION_ROOT is not set (no vcpkg on this machine)}
  triplet=x64-windows-static-md
  # vcpkg is a native Windows program and does not understand MSYS paths like /tmp/....
  "$vcpkg_root/vcpkg.exe" install "libvpx:$triplet" --x-install-root="$(cygpath -w "$work/installed")"
  installed="$work/installed/$triplet"
  if [[ ! -f "$installed/lib/vpx.lib" ]]; then
    echo "vcpkg did not produce lib/vpx.lib; it installed:" >&2
    ls -R "$installed/lib" >&2
    exit 1
  fi
  mkdir -p "$prefix/include" "$prefix/lib"
  cp -R "$installed/include/vpx" "$prefix/include/"
  cp "$installed/lib/vpx.lib" "$prefix/lib/"
else
  curl -fsSL "https://github.com/webmproject/libvpx/archive/refs/tags/v$LIBVPX_VERSION.tar.gz" \
    | tar -xz -C "$work"
  src="$work/libvpx-$LIBVPX_VERSION"
  export MACOSX_DEPLOYMENT_TARGET=$MACOS_DEPLOYMENT_TARGET
  for arch in arm64 x86_64; do
    build="$work/build-$arch"
    mkdir -p "$build"
    # The explicit -arch flags let the arm64 runner cross-compile the x86_64 half.
    (cd "$build" && "$src/configure" \
      --target="$arch-darwin20-gcc" \
      --prefix="$work/install-$arch" \
      --extra-cflags="-arch $arch -mmacosx-version-min=$MACOS_DEPLOYMENT_TARGET" \
      --extra-cxxflags="-arch $arch -mmacosx-version-min=$MACOS_DEPLOYMENT_TARGET" \
      --enable-static --disable-shared --enable-pic \
      --disable-examples --disable-tools --disable-docs --disable-unit-tests \
      && make -j"$(sysctl -n hw.ncpu)" && make install)
  done
  mkdir -p "$prefix/include" "$prefix/lib"
  # The installed headers do not depend on the architecture (vpx_config.h is not installed).
  cp -R "$work/install-arm64/include/vpx" "$prefix/include/"
  lipo -create "$work/install-arm64/lib/libvpx.a" "$work/install-x86_64/lib/libvpx.a" \
    -output "$prefix/lib/libvpx.a"
  lipo -info "$prefix/lib/libvpx.a"
fi

echo "libvpx installed to $prefix"
