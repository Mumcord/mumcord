#!/usr/bin/env bash
# Prints the CHANGELOG.md section of one version, from its "## [X.Y.Z]" heading up to the next
# "## " heading or the link list at the bottom, without trailing blank lines:
#   scripts/changelog-section.sh <X.Y.Z> [file]      (file defaults to CHANGELOG.md in the repository root)
# Prints nothing if the version has no section.
set -euo pipefail
version=${1:?version missing, e.g. 0.4.3}
file=${2:-$(dirname "$0")/../CHANGELOG.md}

awk -v v="$version" '
  index($0, "## [" v "]") == 1 { on = 1 }
  on && /^## / && index($0, "## [" v "]") != 1 { on = 0 }
  on && /^\[[^]]+\]: / { on = 0 }
  on { lines[++n] = $0 }
  END {
    while (n > 0 && lines[n] ~ /^[ \t]*$/) n--
    for (i = 1; i <= n; i++) print lines[i]
  }' "$file"
