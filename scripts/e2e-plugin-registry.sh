#!/usr/bin/env bash
# Prepares everything the plugin E2E specs need from a fixture registry (spec 06, sections 10/11).
# Per run it: builds the `mumcord-plugin` CLI, builds the safety fixtures, fetches and builds the
# four official plugins from Mumcord/plugins, and generates fresh TEST signing keys.
# It does not start any server: e2e/helpers/registry.ts starts `mumcord-plugin serve-registry`
# on free ports (so the ports and process lifetime belong to the spec run).
#
# Usage: scripts/e2e-plugin-registry.sh [--build-only] <work-dir>
#   --build-only stops after the CLI and the packages (bin/, staged/): no keys, no package folders.
#   CI runs it on the runner host, because the E2E container has neither the wasm32 target nor the
#   plugins' pinned toolchain, and passes the result in with MUMCORD_E2E_PLUGIN_PACKAGES.
# Layout written below <work-dir>:
#   keys/registry.{key,pub}   test key of the official registry (new each run, never committed)
#   keys/repo.{key,pub}       test key of the third-party repository (new each run)
#   packages/official/        the 4 official plugins + fx-webview-probe
#   packages/repo/            the other safety fixtures (third-party repository)
#   staged/                   every fixture package, for publishing later (updates, AC-6.19)
#   bin/mumcord-plugin        the CLI
# Environment: CARGO_TARGET_DIR (default ~/.cache/mumcord-build/plugin-e2e-target),
#   MUMCORD_PLUGINS_CACHE (default ~/.cache/mumcord-build/plugins-src),
#   MUMCORD_E2E_PLUGIN_PACKAGES (a --build-only work dir: its CLI and packages are used instead of
#   building them).
set -euo pipefail

build_only=false
if [ "${1:-}" = --build-only ]; then
  build_only=true
  shift
fi
work="${1:?usage: e2e-plugin-registry.sh [--build-only] <work-dir>}"
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cache="${MUMCORD_PLUGINS_CACHE:-$HOME/.cache/mumcord-build/plugins-src}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cache/mumcord-build/plugin-e2e-target}"
plugins_url="${MUMCORD_PLUGINS_URL:-https://github.com/Mumcord/plugins}"

# plugin directory in Mumcord/plugins -> branch that carries its source
official=("poll:feat/poll" "dice:feat/dice" "echolings:feat/echolings" "mumbot-music:feat/mumbot-music")

mkdir -p "$work" "$cache"
rm -rf "$work/keys" "$work/packages" "$work/staged"
mkdir -p "$work/keys" "$work/packages/official" "$work/packages/repo" "$work/staged" "$work/bin"
cli="$work/bin/mumcord-plugin"

prebuilt="${MUMCORD_E2E_PLUGIN_PACKAGES:-}"
if [ -n "$prebuilt" ]; then
  echo "using the prebuilt CLI and packages from $prebuilt" >&2
  cp "$prebuilt/bin/mumcord-plugin" "$cli"
  cp "$prebuilt"/staged/*.mcplugin "$work/staged/"
else
  echo "building the mumcord-plugin CLI" >&2
  cargo build --quiet --manifest-path "$repo/Cargo.toml" -p mumcord-plugin
  cp "$CARGO_TARGET_DIR/debug/mumcord-plugin" "$cli"

  echo "building the safety fixtures" >&2
  "$repo/scripts/build-plugin-fixtures.sh" "$work/fixtures" >&2
  cp "$work"/fixtures/*.mcplugin "$work/staged/"

  for entry in "${official[@]}"; do
    dir="${entry%%:*}"
    branch="${entry#*:}"
    src="$cache/${branch//\//-}"
    echo "fetching $plugins_url $branch" >&2
    if [ ! -d "$src/.git" ]; then
      mkdir -p "$src"
      git -C "$src" init --quiet
      git -C "$src" remote add origin "$plugins_url"
    fi
    git -C "$src" fetch --quiet --depth 1 origin "$branch"
    git -C "$src" checkout --quiet --force FETCH_HEAD
    echo "building $dir" >&2
    "$cli" build "$src/$dir" --out "$work/built/$dir" >&2
    cp "$work/built/$dir/"*.mcplugin "$work/staged/"
  done
fi

if "$build_only"; then
  echo "built: $work" >&2
  exit 0
fi

"$cli" keygen --unencrypted --out "$work/keys" --name registry >/dev/null
"$cli" keygen --unencrypted --out "$work/keys" --name repo >/dev/null

# The official plugins and fx-webview-probe (the one fixture signed as official) go to the official
# registry, the other fixtures to the repository; fx-update-v2 stays staged until a spec publishes it.
for f in "$work"/staged/*.mcplugin; do
  case "$(basename "$f")" in
    fx-update-v2-fails.mcplugin) ;;
    fx-webview-probe.mcplugin) cp "$f" "$work/packages/official/" ;;
    fx-*) cp "$f" "$work/packages/repo/" ;;
    *) cp "$f" "$work/packages/official/" ;;
  esac
done
echo "ready: $work" >&2
