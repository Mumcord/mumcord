#!/usr/bin/env bash
# Builds the safety fixture plugins (AC-6.6 ... AC-6.19) from their WAT sources.
# Output: one folder and one .mcplugin per fixture; nothing is committed as a binary.
#
# Usage: scripts/build-plugin-fixtures.sh [out-dir]
#   out-dir defaults to $MUMCORD_FIXTURE_OUT, then ~/.cache/mumcord-build/plugin-fixtures.
#   CARGO_TARGET_DIR is respected; without it the builder uses a target dir next to the output.
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
src="$repo/crates/plugins/tests/fixtures/safety"
out="${1:-${MUMCORD_FIXTURE_OUT:-$HOME/.cache/mumcord-build/plugin-fixtures}}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$out.target}"

cargo run --quiet --manifest-path "$src/builder/Cargo.toml" -- \
  "$src" "$repo/crates/plugins/wit" "$out"
