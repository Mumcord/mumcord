#!/bin/bash
# Tests scripts/release-tag.sh against a throwaway repository whose origin has main and
# milestone/0.8.0: final releases only from main, release candidates only from the matching
# milestone branch and never from a commit that is already on main.
set -euo pipefail
script=$(cd "$(dirname "$0")/.." && pwd)/release-tag.sh
repo=$(mktemp -d)
trap 'rm -rf "$repo"' EXIT
fail() { echo "FAIL: $1" >&2; exit 1; }

cd "$repo"
git init -q -b main
git config user.email test@example.invalid
git config user.name test
commit() { git commit -q --allow-empty -m "$1"; git rev-parse HEAD; }
on_main=$(commit "on main")
git checkout -q -b milestone/0.8.0
on_milestone=$(commit "feature")
git checkout -q main
fix_on_main=$(commit "fix")
# The remote-tracking branches the release workflow sees after a full checkout.
git update-ref refs/remotes/origin/main main
git update-ref refs/remotes/origin/milestone/0.8.0 milestone/0.8.0

ok() {
  local out
  out=$("$script" "$1" "$2" 2>&1) || fail "$1 on $2 was rejected: $out"
  [[ "$out" == "$3" ]] || fail "$1: expected '$3', got '$out'"
}
rejected() {
  local out
  if out=$("$script" "$1" "$2" 2>&1); then fail "$1 on $2 was accepted"; fi
  [[ "$out" == *"$3"* ]] || fail "$1: message '$out' does not mention '$3'"
}

ok v0.7.2 "$fix_on_main" $'version=0.7.2\nprerelease=false'
ok v0.8.0-rc.1 "$on_milestone" $'version=0.8.0-rc.1\nprerelease=true'
ok v0.8.0-rc.12 "$on_milestone" $'version=0.8.0-rc.12\nprerelease=true'

rejected v0.8.0 "$on_milestone" "not on main"
rejected v0.8.0-rc.1 "$fix_on_main" "not on milestone/0.8.0"
rejected v0.8.0-rc.1 "$on_main" "already on main"
rejected v0.9.0-rc.1 "$on_milestone" "milestone/0.9.0, which does not exist"
for tag in v0.8.0-beta.1 v0.8.0-rc.0 v0.8.0-rc v0.8.0-rc.1.2 v0.8 0.8.0 nightly; do
  rejected "$tag" "$on_milestone" "neither a release"
done

echo "release-tag.sh: tags accepted and rejected as expected."
