#!/bin/bash
# Checks that a pushed tag may start a release build and prints what to build (RELEASING.md):
#   - `vX.Y.Z` on a commit of main: a final release;
#   - `vX.Y.Z-rc.N` on a commit of milestone/X.Y.Z that is not on main yet: a release candidate,
#     published as a GitHub pre-release.
# Anything else fails with a message that says which tag fits where, before any build starts.
#   scripts/release-tag.sh <tag> [commit]   (commit defaults to HEAD)
# Prints `version=…` and `prerelease=true|false`, ready for $GITHUB_OUTPUT. Needs the remote
# branches as origin/* (actions/checkout with fetch-depth: 0).
set -euo pipefail
tag=${1:?tag missing, e.g. v0.8.0 or v0.8.0-rc.1}
commit=$(git rev-parse --verify "${2:-HEAD}^{commit}")
fail() { echo "$1" >&2; exit 1; }
on() { git merge-base --is-ancestor "$commit" "$1"; }

if [[ "$tag" =~ ^v([0-9]+\.[0-9]+\.[0-9]+)$ ]]; then
  version=${BASH_REMATCH[1]}
  on origin/main \
    || fail "Tag '$tag' points to $commit, which is not on main. Tag a commit on main (release candidates are tagged vX.Y.Z-rc.N on milestone/X.Y.Z)."
  prerelease=false
elif [[ "$tag" =~ ^v([0-9]+\.[0-9]+\.[0-9]+)-rc\.([1-9][0-9]*)$ ]]; then
  version=${tag#v}
  branch=milestone/${BASH_REMATCH[1]}
  git rev-parse --verify --quiet "origin/$branch" >/dev/null \
    || fail "Release candidate '$tag' belongs on the branch $branch, which does not exist."
  on "origin/$branch" \
    || fail "Tag '$tag' points to $commit, which is not on $branch. Tag a commit on $branch."
  ! on origin/main \
    || fail "Tag '$tag' points to $commit, which is already on main. Tag a commit on $branch that is not on main yet, or release vX.Y.Z from main."
  prerelease=true
else
  fail "Tag '$tag' is neither a release (vX.Y.Z on main) nor a release candidate (vX.Y.Z-rc.N on milestone/X.Y.Z)."
fi

echo "version=$version"
echo "prerelease=$prerelease"
