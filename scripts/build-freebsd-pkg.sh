#!/bin/sh
# Builds the FreeBSD package (.pkg) from a release build of the app.
#
#   scripts/build-freebsd-pkg.sh [--no-build] [--no-strip] [output-dir]
#
# Builds the app by default (`cargo build --release -p mumcord --features custom-protocol`, the
# switch the Tauri CLI sets for a production build); --no-build packages an existing binary.
#
# Expects `target/release/mumcord` (cargo build --release -p mumcord --features custom-protocol)
# and the built UI in `dist/`. The package installs under /usr/local, like every FreeBSD port:
#
#   /usr/local/bin/mumcord
#   /usr/local/lib/mumcord/camera-backgrounds/*.webp     (resources, see resource_dir() in the app)
#   /usr/local/share/applications/app.mumcord.client.desktop
#   /usr/local/share/icons/hicolor/<size>/apps/app.mumcord.client.png
#   /usr/local/share/doc/mumcord/{LICENSE,THIRD_PARTY_NOTICES}
#
# The package also carries a +DISPLAY message (packaging/freebsd/pkg-message) that pkg shows after
# the install: the Intel graphics driver requirement.
#
# The runtime dependencies are read from the linked libraries with pkg(8), so the manifest lists
# what this build actually needs instead of a hand-kept list. `pkg repo <output-dir>` turns the
# result into a repository afterwards; signing is left to the caller (see RELEASING.md).
set -eu

root_dir=$(cd "$(dirname "$0")/.." && pwd)
strip_binary=1
build=1
output=""
for arg in "$@"; do
  case "$arg" in
    --no-strip) strip_binary=0 ;;
    --no-build) build=0 ;;
    -*) echo "unknown option: $arg" >&2; exit 2 ;;
    *) output=$arg ;;
  esac
done
[ -n "$output" ] || output="$root_dir/dist-freebsd"

app_id=app.mumcord.client
binary="$root_dir/target/release/mumcord"
if [ "$build" -eq 1 ]; then
  # --features custom-protocol is what the Tauri CLI turns on for `tauri build`: without it the
  # binary is a development build that expects the Vite dev server and shows an empty window.
  echo "== building the app"
  (cd "$root_dir" && CARGO_PROFILE_RELEASE_DEBUG=0 cargo build --release -p mumcord \
    --features custom-protocol >/dev/null) || { echo "the build failed" >&2; exit 1; }
fi
if [ ! -x "$binary" ]; then
  echo "missing $binary: run 'cargo build --release -p mumcord --features custom-protocol'" >&2
  exit 2
fi
if [ ! -d "$root_dir/dist" ]; then
  echo "missing $root_dir/dist: run 'npm run build' first" >&2
  exit 2
fi

version=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$root_dir/Cargo.toml" | head -1)
[ -n "$version" ] || { echo "cannot read the version from Cargo.toml" >&2; exit 2; }
arch=$(uname -m)
abi="FreeBSD:$(freebsd-version | cut -d. -f1):$arch"

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT

mkdir -p "$stage/usr/local/bin" \
         "$stage/usr/local/lib/mumcord/camera-backgrounds" \
         "$stage/usr/local/share/applications" \
         "$stage/usr/local/share/doc/mumcord"

echo "== staging the app"
cp "$binary" "$stage/usr/local/bin/mumcord"
[ "$strip_binary" -eq 0 ] || strip -s "$stage/usr/local/bin/mumcord"
chmod 755 "$stage/usr/local/bin/mumcord"

echo "== staging the resources and the desktop entry"
cp "$root_dir"/assets/backgrounds/*.webp "$stage/usr/local/lib/mumcord/camera-backgrounds/"
install -m 644 "$root_dir/packaging/freebsd/mumcord.desktop" \
  "$stage/usr/local/share/applications/$app_id.desktop"

for icon in 32x32 128x128 256x256 512x512; do
  case "$icon" in
    32x32) source="$root_dir/src-tauri/icons/32x32.png" ;;
    128x128) source="$root_dir/src-tauri/icons/128x128.png" ;;
    256x256) source="$root_dir/src-tauri/icons/128x128@2x.png" ;;
    512x512) source="$root_dir/src-tauri/icons/icon.png" ;;
  esac
  target="$stage/usr/local/share/icons/hicolor/$icon/apps/$app_id.png"
  mkdir -p "$(dirname "$target")"
  install -m 644 "$source" "$target"
done

install -m 644 "$root_dir/LICENSE" "$stage/usr/local/share/doc/mumcord/LICENSE"
mkdir -p "$stage/usr/local/share/licenses/mumcord-$version"
install -m 644 "$root_dir/LICENSE" "$stage/usr/local/share/licenses/mumcord-$version/LICENSE"
if [ -f "$root_dir/THIRD_PARTY_NOTICES" ]; then
  install -m 644 "$root_dir/THIRD_PARTY_NOTICES" \
    "$stage/usr/local/share/doc/mumcord/THIRD_PARTY_NOTICES"
fi

# Runtime dependencies: every linked library outside the base system, mapped to the package that
# installs it, plus the sonames the package needs (pkg checks them at install time).
echo "== collecting the runtime dependencies"
linked=$(ldd "$stage/usr/local/bin/mumcord")
shlibs=$(printf '%s\n' "$linked" | awk '/=>/ {print $1}' | sort -u | paste -sd, - | sed 's/,/, /g')
deps=""
for lib in $(printf '%s\n' "$linked" | awk '/=>/ {print $3}' | sort -u); do
  case "$lib" in
    /usr/local/*) ;;
    *) continue ;;
  esac
  # `pkg which` answers with the port origin; the manifest keys its map by package name.
  origin=$(pkg which -q -o "$lib" 2>/dev/null || true)
  [ -n "$origin" ] || continue
  name=$(pkg query -e "%o == $origin" '%n' | head -1)
  [ -n "$name" ] || continue
  case " $deps " in *" $name "*) continue ;; esac
  deps="$deps $name"
done
dep_lines=""
for name in $deps; do
  # Separate names: `read` would otherwise overwrite the app version below.
  read -r dep_origin dep_version <<EOF_DEP
$(pkg query -e "%n == $name" '%o %v' | head -1)
EOF_DEP
  dep_lines="$dep_lines
  $name: { origin: \"$dep_origin\", version: \"$dep_version\" }"
done

flatsize=$(find "$stage/usr/local" -type f -exec stat -f %z {} + | awk '{sum += $1} END {print sum}')
case "$arch" in
  amd64) pkg_arch=freebsd:15:x86:64 ;;
  aarch64) pkg_arch=freebsd:15:arm64:64 ;;
  *) pkg_arch="freebsd:15:$arch" ;;
esac

# Shown by pkg after the install: the graphics driver requirement. Newer pkg keeps install
# messages in the manifest (a legacy +DISPLAY file is ignored).
message=$(sed -e 's|\\|\\\\|g' -e 's|"|\\"|g' "$root_dir/packaging/freebsd/pkg-message" \
  | awk '{printf "%s\\n", $0}')

cat > "$stage/+MANIFEST" <<EOF
name: mumcord
version: "$version"
origin: net-im/mumcord
comment: "Modern voice chat client compatible with Mumble"
www: "https://github.com/Mumcord/mumcord"
maintainer: "mumcord@invalid"
prefix: "/usr/local"
abi: "$abi"
arch: "$pkg_arch"
flatsize: $flatsize
categories: [net-im, audio]
licenselogic: single
licenses: [AGPLv3]
desc: <<EOD
Mumcord is a modern voice chat client that speaks the Mumble protocol and stays compatible with
standard Mumble servers.

Screen sharing captures the whole X11 screen (there is no xdg-desktop-portal on FreeBSD) and
carries no sound; camera sharing and the in-game overlay are not available. Updates come from
the package manager: "pkg upgrade mumcord".
EOD
deps: {$dep_lines
}
shlibs_required: [$shlibs]
messages: [{ message: "$message", type: "install" }]
scripts: {
  post-install: "if [ -x /usr/local/bin/update-desktop-database ]; then /usr/local/bin/update-desktop-database -q /usr/local/share/applications; fi",
  post-deinstall: "if [ -x /usr/local/bin/update-desktop-database ]; then /usr/local/bin/update-desktop-database -q /usr/local/share/applications; fi"
}
EOF

# The plist lists every installed file and, below them, the directories the package creates for
# itself; the shared parents (bin, share/applications, hicolor) belong to the base system.
{
  find "$stage/usr/local" \( -type f -o -type l \) | sed "s|^$stage||"
  for dir in /usr/local/lib/mumcord/camera-backgrounds /usr/local/lib/mumcord \
             /usr/local/share/doc/mumcord; do
    echo "@dir $dir"
  done
  for icon in 32x32 128x128 256x256 512x512; do
    echo "@dir /usr/local/share/icons/hicolor/$icon/apps"
  done
} | sort -u > "$stage/+PLIST"

mkdir -p "$output"
echo "== creating the package in $output"
# `pkg create` wants the metadata inside the staged tree; -m/-p point at them.
(cd "$stage" && pkg create -m +MANIFEST -p +PLIST -r "$stage" -o "$output")

package_file=$(ls -t "$output"/mumcord-"$version"*.pkg 2>/dev/null | head -1)
if [ -z "$package_file" ]; then
  echo "pkg create produced no package" >&2
  exit 1
fi
echo "== $package_file ($(du -h "$package_file" | cut -f1))"
echo "   contents: pkg info -l $package_file"
echo "   install:  sudo pkg add -f $package_file"
