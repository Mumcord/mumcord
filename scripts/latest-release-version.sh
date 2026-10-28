#!/bin/bash
# Prints the version of the newest release tag (vX.Y.Z, pre-releases ignored) on origin. Dev
# bundles use it as their numeric package version, which installer formats require
# (RELEASING.md). Asks the remote, so it works in shallow CI checkouts without tags.
set -euo pipefail
latest=$(git ls-remote --tags --refs origin 'v*' \
  | sed -n 's|.*refs/tags/v\([0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*\)$|\1|p' \
  | sort -t. -k1,1n -k2,2n -k3,3n \
  | tail -n 1)
[ -n "$latest" ] || { echo "no release tag vX.Y.Z found on origin" >&2; exit 1; }
echo "$latest"
