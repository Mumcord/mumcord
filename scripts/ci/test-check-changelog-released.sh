#!/usr/bin/env bash
# Tests scripts/ci/check-changelog-released.sh and scripts/changelog-section.sh.
set -euo pipefail
cd "$(dirname "$0")/../.."
dir=$(mktemp -d)
trap 'rm -rf "$dir"' EXIT

cat > "$dir/base.md" <<'MD'
# Changelog

## [0.2.0] - Unreleased

- Next thing

## [0.1.0] - 2026-01-01

- First thing

[0.1.0]: https://example.invalid/0.1.0
MD

check() { scripts/ci/check-changelog-released.sh "$dir/base.md" "$dir/head.md" > /dev/null 2>&1; }
fail() { echo "FAIL: $1" >&2; exit 1; }

[[ $(scripts/changelog-section.sh 0.1.0 "$dir/base.md") == $'## [0.1.0] - 2026-01-01\n\n- First thing' ]] || fail "section text"

sed 's/Next thing/Next thing, changed/' "$dir/base.md" > "$dir/head.md"
check || fail "editing the unreleased section must pass"

sed 's/^## \[0.2.0\] - Unreleased/## [0.2.0] - 2026-02-02/' "$dir/base.md" > "$dir/head.md"
check || fail "dating an unreleased section must pass"

sed 's/First thing/First thing, fixed/' "$dir/base.md" > "$dir/head.md"
! check || fail "editing a released section must fail"

sed '/^## \[0.1.0\]/,/^- First thing/d' "$dir/base.md" > "$dir/head.md"
! check || fail "deleting a released section must fail"

sed 's/^## \[0.1.0\] - 2026-01-01/## [0.1.0] - 2026-01-02/' "$dir/base.md" > "$dir/head.md"
! check || fail "changing a release date must fail"
echo "changelog checks ok"
