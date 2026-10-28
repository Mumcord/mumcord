#!/usr/bin/env bash
# Writes one CycloneDX SBOM per bundle whose contents differ: mumcord-<version>-<platform>.cdx.json.
# Each lists the Rust crates compiled for that platform (cargo-cyclonedx), the npm packages of the
# UI (cyclonedx-npm, production only), and the native libraries we build or bundle: libvpx,
# libopus (vendored by opusic-sys) and, for the AppImage, WebKitGTK, GStreamer and its plugins.
# The release workflow runs it; it also works locally. Usage:
#   scripts/generate-sbom.sh <version> <native-dir> <out-dir>
# <native-dir> holds sbom-native-<build>.txt from scripts/sbom-native.sh for each build job
# (linux, linux-arm64, windows, macos). PLATFORMS limits the output, for example
#   PLATFORMS="appimage-x86_64 deb-rpm-x86_64" scripts/generate-sbom.sh 0.0.0 native out
# Needs cargo-cyclonedx at the version in scripts/license-tools.env, node/npm and jq.
set -euo pipefail

version="${1:?usage: generate-sbom.sh <version> <native-dir> <out-dir>}"
native_dir="$(cd "${2:?native-dir missing}" && pwd)"
mkdir -p "${3:?out-dir missing}"
out_dir="$(cd "$3" && pwd)"

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
# shellcheck source=scripts/license-tools.env
source scripts/license-tools.env

command -v cargo-cyclonedx >/dev/null 2>&1 || {
  echo "cargo-cyclonedx not found. Install: cargo install cargo-cyclonedx --version $CARGO_CYCLONEDX_VERSION --locked" >&2
  exit 1
}

# platform, build job, Rust targets (comma-separated), whether GStreamer is bundled.
table="
appimage-x86_64   linux       x86_64-unknown-linux-gnu                  gst
deb-rpm-x86_64    linux       x86_64-unknown-linux-gnu                  -
appimage-aarch64  linux-arm64 aarch64-unknown-linux-gnu                 gst
deb-rpm-aarch64   linux-arm64 aarch64-unknown-linux-gnu                 -
windows-x86_64    windows     x86_64-pc-windows-msvc                    -
macos-universal   macos       aarch64-apple-darwin,x86_64-apple-darwin  -
"
platforms="${PLATFORMS:-$(awk 'NF { print $1 }' <<<"$table")}"

tmp="$(mktemp -d)"
name="sbom-$$"
cleanup() {
  rm -rf "$tmp"
  # cargo-cyclonedx writes a file next to every workspace member's Cargo.toml.
  find "$root" -maxdepth 3 -path "$root/node_modules" -prune -o -name "$name.json" -print0 \
    | xargs -0 -r rm -f
}
trap cleanup EXIT

npx --yes "@cyclonedx/cyclonedx-npm@$CYCLONEDX_NPM_VERSION" --package-lock-only --omit dev \
  --flatten-components --spec-version 1.5 --output-format JSON --output-file "$tmp/npm.json"

opus_manifest="$(cargo metadata --locked --format-version 1 \
  | jq -r '.packages[] | select(.name == "opusic-sys") | .manifest_path')"
[ -n "$opus_manifest" ] || { echo "opusic-sys not found in the dependency graph" >&2; exit 1; }
opus="$(sed -n 's/^PACKAGE_VERSION="\(.*\)"/\1/p' "$(dirname "$opus_manifest")/opus/package_version")"
[ -n "$opus" ] || { echo "libopus version not found in opusic-sys" >&2; exit 1; }

# One run per Rust target; the result is reused by platforms with the same target.
rust_bom() {
  local target="$1" crate="$2" dir="$tmp/rust-$1"
  if [ ! -d "$dir" ]; then
    mkdir -p "$dir"
    cargo-cyclonedx cyclonedx --manifest-path src-tauri/Cargo.toml --format json --target "$target" \
      --no-build-deps --spec-version 1.5 --override-filename "$name" --quiet
    for f in src-tauri crates/*; do
      if [ -f "$f/$name.json" ]; then mv "$f/$name.json" "$dir/$(basename "$f").json"; fi
    done
  fi
  echo "$dir/$crate.json"
}

for platform in $platforms; do
  read -r _ build targets gst < <(awk -v p="$platform" '$1 == p' <<<"$table") \
    || { echo "unknown platform: $platform" >&2; exit 1; }
  native="$native_dir/sbom-native-$build.txt"
  [ -f "$native" ] || { echo "$native missing (scripts/sbom-native.sh on the $build build)" >&2; exit 1; }
  boms=()
  for target in ${targets//,/ }; do
    boms+=("$(rust_bom "$target" src-tauri)")
    # The Linux packages also ship the in-game overlay layer (src-tauri/tauri.linux.conf.json).
    if [[ "$target" == *linux* ]]; then boms+=("$(rust_bom "$target" overlay-layer)"); fi
  done
  out="$out_dir/mumcord-$version-$platform.cdx.json"
  node scripts/sbom-merge.js --version "$version" --platform "$platform" --root "$root" \
    --native "$native" --gstreamer "$gst" --opus "$opus" --npm "$tmp/npm.json" --out "$out" "${boms[@]}"
  echo "Wrote $out"
done
