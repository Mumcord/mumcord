#!/usr/bin/env bash
# Prepares everything the plugin E2E specs need from a fixture registry (spec 06, sections 10/11).
# Per run it: builds the `mumcord-plugin` CLI, the safety fixtures and the E2E test plugins
# (e2e/fixtures/plugins), and generates fresh TEST signing keys. The official plugins are tested in
# their own repository (Mumcord/plugins), so nothing here depends on it.
# It does not start any server: e2e/helpers/registry.ts starts `mumcord-plugin serve-registry`
# on free ports (so the ports and process lifetime belong to the spec run).
#
# Usage: scripts/e2e-plugin-registry.sh [--build-only] <work-dir>
#   --build-only stops after the CLI and the packages (bin/, staged/, packages/): no keys.
#   CI runs it on the runner host, because the E2E container has no wasm32 target, and passes the
#   result in with MUMCORD_E2E_PLUGIN_PACKAGES.
# Layout written below <work-dir>:
#   keys/registry.{key,pub}   test key of the official registry (new each run, never committed)
#   keys/repo.{key,pub}       test key of the third-party repository (new each run)
#   packages/official/        the E2E test plugins + fx-webview-probe
#   packages/repo/            the other safety fixtures and the third-party test plugins
#   staged/                   every fixture package + routes.txt ("<file> <repo>"), for publishing later
#   bin/mumcord-plugin        the CLI
# Environment: CARGO_TARGET_DIR (default ~/.cache/mumcord-build/plugin-e2e-target; the test plugins
#   build into e2e/fixtures/plugins/*/target, as `mumcord-plugin build` always does),
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
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cache/mumcord-build/plugin-e2e-target}"

mkdir -p "$work"
rm -rf "$work/keys" "$work/packages" "$work/staged" "$work/built" "$work/built-one"
mkdir -p "$work/keys" "$work/packages/official" "$work/packages/repo" "$work/staged" "$work/bin"
cli="$work/bin/mumcord-plugin"

prebuilt="${MUMCORD_E2E_PLUGIN_PACKAGES:-}"
if [ -n "$prebuilt" ]; then
  echo "using the prebuilt CLI and packages from $prebuilt" >&2
  cp "$prebuilt/bin/mumcord-plugin" "$cli"
  cp "$prebuilt"/staged/*.mcplugin "$prebuilt/staged/routes.txt" "$work/staged/"
else
  echo "building the mumcord-plugin CLI" >&2
  cargo build --quiet --manifest-path "$repo/Cargo.toml" -p mumcord-plugin
  cp "$CARGO_TARGET_DIR/debug/mumcord-plugin" "$cli"

  echo "building the safety fixtures" >&2
  "$repo/scripts/build-plugin-fixtures.sh" "$work/fixtures" >&2
  cp "$work"/fixtures/*.mcplugin "$work/staged/"

  # Each fixture folder may carry an e2e-fixture.json marker:
  #   {"repo": "official" | "third-party", "check": "pass" | "fail", "versions": ["."] or subfolders}
  # Without a marker a folder is one official version that must pass `check --official`. A "fail"
  # fixture must be rejected by `check --official` (the host refuses it at install time).
  : > "$work/staged/routes.txt"
  for dir in "$repo"/e2e/fixtures/plugins/*/; do
    name="$(basename "$dir")"
    marker="$dir/e2e-fixture.json"
    # test-api10 is built once with the 0.6.2 SDK; its committed package is used as-is.
    if [ "$name" = test-api10 ] && [ ! -f "$marker" ]; then
      mkdir -p "$work/built"
      cp "$dir"test-api10.mcplugin "$work/built/"
      echo "test-api10.mcplugin official" >> "$work/staged/routes.txt"
      continue
    fi
    target=official check=pass versions=.
    if [ -f "$marker" ]; then
      target="$(jq -er '.repo | select(. == "official" or . == "third-party")' "$marker")"
      check="$(jq -er '.check | select(. == "pass" or . == "fail")' "$marker")"
      versions="$(jq -er '.versions | join("\n")' "$marker")"
    fi
    while IFS= read -r version; do
      echo "building the test plugin $name/$version" >&2
      one="$work/built-one"
      rm -rf "$one"
      mkdir -p "$one" "$work/built"
      "$cli" build "$dir/$version" --out "$one" >&2
      for f in "$one"/*.mcplugin; do
        if [ "$check" = pass ]; then
          "$cli" check --official "$f" >&2
        elif "$cli" check --official "$f" >/dev/null 2>&1; then
          echo "error: $name/$version is marked check=fail but passes check --official" >&2
          exit 1
        fi
        echo "$(basename "$f") $target" >> "$work/staged/routes.txt"
        mv "$f" "$work/built/"
      done
    done <<< "$versions"
  done
  cp "$work"/built/*.mcplugin "$work/staged/"
fi

# The test plugins and fx-webview-probe (the one safety fixture signed as official) go to the
# official registry, the other fixtures to the repository; fx-update-v2 stays staged until a spec
# publishes it.
for f in "$work"/staged/*.mcplugin; do
  case "$(basename "$f")" in
    fx-update-v2-fails.mcplugin) ;;
    fx-webview-probe.mcplugin | fx-strip-*.mcplugin) cp "$f" "$work/packages/official/" ;;
    fx-*) cp "$f" "$work/packages/repo/" ;;
    *)
      if grep -qx "$(basename "$f") third-party" "$work/staged/routes.txt"; then
        cp "$f" "$work/packages/repo/"
      else
        cp "$f" "$work/packages/official/"
      fi
      ;;
  esac
done

if "$build_only"; then
  echo "built: $work" >&2
  exit 0
fi

"$cli" keygen --unencrypted --out "$work/keys" --name registry >/dev/null
"$cli" keygen --unencrypted --out "$work/keys" --name repo >/dev/null
echo "ready: $work" >&2
