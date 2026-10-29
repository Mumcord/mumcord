#!/bin/bash
# Tests scripts/finalise-changelog.sh on small changelogs in a temporary directory: an open
# "## [X.Y.Z] - Unreleased" section gets no second open section, a plain "## [Unreleased]" is
# renamed and replaced by a new empty one, a dated section stays as is, and a second run changes nothing.
#   scripts/ci/test-finalise-changelog.sh
set -euo pipefail
repo=$(cd "$(dirname "$0")/../.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
fail() { echo "FAIL: $1" >&2; exit 1; }

mkdir -p "$work/scripts"
cp "$repo/scripts/finalise-changelog.sh" "$work/scripts/"

# run <name>: writes the changelog from stdin, finalises 0.4.3, and checks that a second run is a no-op.
run() {
  cat > "$work/CHANGELOG.md"
  "$work/scripts/finalise-changelog.sh" 0.4.3 2026-10-30 > /dev/null
  cp "$work/CHANGELOG.md" "$work/first.md"
  "$work/scripts/finalise-changelog.sh" 0.4.3 2026-10-30 > /dev/null
  cmp -s "$work/first.md" "$work/CHANGELOG.md" || fail "$1: a second run changed the file"
}
open_sections() { grep -c -E '^## \[([0-9.]+\] - )?Unreleased\]?$' "$work/CHANGELOG.md" || true; }

run "next version open" <<'MD'
# Changelog

## [0.4.4] - Unreleased

- next

## [0.4.3] - Unreleased

- current

[0.4.2]: https://example.invalid/v0.4.2
MD
[[ "$(open_sections)" == 1 ]] || fail "next version open: expected exactly one open section"
grep -q '^## \[0.4.3\] - 2026-10-30$' "$work/CHANGELOG.md" || fail "next version open: 0.4.3 not dated"
grep -q '^## \[0.4.4\] - Unreleased$' "$work/CHANGELOG.md" || fail "next version open: 0.4.4 changed"

run "plain unreleased" <<'MD'
# Changelog

## [Unreleased]

- current

[0.4.2]: https://example.invalid/v0.4.2
MD
grep -q '^## \[0.4.3\] - 2026-10-30$' "$work/CHANGELOG.md" || fail "plain unreleased: not renamed"
[[ "$(sed -n 3p "$work/CHANGELOG.md")" == "## [Unreleased]" ]] || fail "plain unreleased: no new section on top"
[[ "$(open_sections)" == 1 ]] || fail "plain unreleased: expected exactly one open section"
grep -q '^\[0.4.3\]: ' "$work/CHANGELOG.md" || fail "plain unreleased: link missing"

run "already dated" <<'MD'
# Changelog

## [0.4.4] - Unreleased

- next

## [0.4.3] - 2026-10-29

- current

[0.4.3]: https://github.com/Mumcord/mumcord/releases/tag/v0.4.3
MD
grep -q '^## \[0.4.3\] - 2026-10-29$' "$work/CHANGELOG.md" || fail "already dated: date changed"
[[ "$(open_sections)" == 1 ]] || fail "already dated: expected exactly one open section"

echo "finalise-changelog tests passed."
