#!/usr/bin/env bash
# Release build for local testing.
#
# `npm run tauri build` rebuilds the frontend on every run, and Tauri embeds `dist/` into the
# binary at compile time: unchanged sources still cost minutes for the app crate and its link.
# This script builds the frontend only when its sources changed and then calls the same build
# with `beforeBuildCommand` overridden to nothing. Keep using the Tauri CLI, so the cargo
# feature set (`--features tauri/custom-protocol`) and its plugin-config environment stay
# exactly what a normal build uses; a plain `cargo build` would invalidate all of that.
#
# Unchanged rebuild: seconds. Frontend changed: about a minute. Rust changed: same as before.
set -euo pipefail
cd "$(dirname "$0")/.."

frontend_inputs=(ui assets package.json package-lock.json vite.config.ts tsconfig.json tsconfig.app.json tsconfig.node.json)

needs_frontend=0
if [ ! -f dist/index.html ]; then
  needs_frontend=1
elif [ -n "$(find "${frontend_inputs[@]}" -newer dist/index.html -print -quit)" ]; then
  needs_frontend=1
fi

if [ "$needs_frontend" = 1 ]; then
  npm run build
else
  echo "frontend unchanged, skipping npm run build"
fi

npm run build:overlay-layer
npm run tauri build -- --no-bundle --config '{"build":{"beforeBuildCommand":""}}'
