#!/bin/bash
# Builds the Mumcord 0.6.2 history peer used by the history-image E2E (AC-1.51f, issue #402).
#
# Usage: scripts/build-old-peer.sh [root]
#   root defaults to ~/.cache/mumcord-build/old-peer. The script checks out the v0.6.2 tag as a
#   git worktree at <root>/v062, copies crates/engine/tests/old_peer/history_peer.rs into its
#   engine examples and builds it with the target dir <root>/target-v062. The last line printed
#   is the binary's path; run the live test with MUMCORD_OLD_PEER=<that path>.
# Clean up afterwards with: git worktree remove --force <root>/v062 && rm -rf <root>/target-v062
set -euo pipefail

repo="$(cd "$(dirname "$0")/.." && pwd)"
root="${1:-$HOME/.cache/mumcord-build/old-peer}"
tag="v0.6.2"
checkout="$root/v062"
target="$root/target-v062"

mkdir -p "$root"
if [ ! -d "$checkout" ]; then
  git -C "$repo" worktree add --detach "$checkout" "$tag" >&2
fi
mkdir -p "$checkout/crates/engine/examples"
cp "$repo/crates/engine/tests/old_peer/history_peer.rs" "$checkout/crates/engine/examples/history_peer.rs"
(
  cd "$checkout"
  CARGO_TARGET_DIR="$target" cargo build --quiet -p mumcord-engine --features test-backend --example history_peer >&2
)
echo "$target/debug/examples/history_peer"
