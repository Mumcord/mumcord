#!/usr/bin/env bash
# Records or re-checks the fingerprint of a published release (see RELEASING.md, "Release integrity").
#
#   scripts/release-manifest.sh vX.Y.Z              prints the manifest of the release's current state
#   scripts/release-manifest.sh --verify FILE       compares that state with the manifest FILE
#
# The manifest holds the tag and the commit it points to, every release asset (name, size, SHA-256
# as GitHub computed it on upload), the hash of SHA256SUMS and, per library crate, the checksum
# crates.io reports for that version and whether it is yanked ("crates.io": null = not published).
# "changelog" holds the text of the version's CHANGELOG.md section (scripts/changelog-section.sh)
# and its SHA-256, taken from the CHANGELOG.md of the checkout (main). The SBOMs are release assets, so their hashes are in "assets". Nothing in it changes unless the
# release does, so a verification can compare it byte for byte after normalising.
# Needs gh (GH_TOKEN), curl, jq and a checkout of main; GITHUB_REPOSITORY defaults to Mumcord/mumcord.
set -euo pipefail

repo=${GITHUB_REPOSITORY:-Mumcord/mumcord}
crates=(protocol codec audio store p2p)

build() {
  local tag=$1 version=${1#v} release commit changelog crates_json='{}' crate response
  release=$(gh api "repos/$repo/releases/tags/$tag")
  if [[ $(jq -r '.draft or .prerelease' <<< "$release") != false ]]; then
    echo "$tag is a draft or pre-release; only published releases have a manifest." >&2
    return 1
  fi
  # The commits endpoint resolves annotated tags to the commit they point to.
  commit=$(gh api "repos/$repo/commits/$tag" -q .sha)

  changelog=$("$(dirname "$0")/changelog-section.sh" "$version")
  if [[ -z "$changelog" ]]; then
    echo "CHANGELOG.md has no section for $version; run this from a checkout of main after the sync." >&2
    return 1
  fi

  for crate in "${crates[@]}"; do
    response=$(curl -sS --retry 3 --retry-all-errors -A "mumcord-release-integrity (https://github.com/$repo)" \
      -w '\n%{http_code}' "https://crates.io/api/v1/crates/mumcord-$crate/$version")
    case ${response##*$'\n'} in
      200) crates_json=$(jq --arg c "mumcord-$crate" --argjson r "${response%$'\n'*}" \
             '.[$c] = {cksum: $r.version.checksum, yanked: $r.version.yanked}' <<< "$crates_json") ;;
      404) crates_json=$(jq --arg c "mumcord-$crate" '.[$c] = null' <<< "$crates_json") ;;
      *) echo "crates.io answered ${response##*$'\n'} for mumcord-$crate $version" >&2; return 1 ;;
    esac
  done

  jq -S --arg tag "$tag" --arg commit "$commit" --argjson crates "$crates_json" \
    --arg changelog "$changelog" --arg changelog_sha "$(sha256sum <<< "$changelog" | cut -d' ' -f1)" '
    (.assets | map({name, size, sha256: (.digest | ltrimstr("sha256:"))}) | sort_by(.name)) as $assets
    | {
        tag: $tag,
        commit: $commit,
        sha256sums: ($assets | map(select(.name == "SHA256SUMS").sha256) | first),
        assets: $assets,
        changelog: {sha256: $changelog_sha, text: $changelog},
        "crates.io": $crates
      }' <<< "$release"
}

case ${1:-} in
  --verify)
    file=${2:?usage: $0 --verify FILE}
    tag=$(jq -r .tag "$file")
    current=$(build "$tag")
    if diff -u --label "$file (recorded)" --label "$tag (now)" <(jq -S . "$file") <(echo "$current"); then
      echo "$tag is unchanged."
    else
      exit 1
    fi
    ;;
  v[0-9]*) build "$1" ;;
  *) echo "usage: $0 vX.Y.Z | --verify FILE" >&2; exit 2 ;;
esac
