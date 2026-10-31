#!/usr/bin/env bash
# Signs the bundles the in-app updater installs (issue #168) and writes the update manifest
# latest.json for the release. Run in the folder with the release files:
#
#   scripts/sign-updater-bundles.sh <version> <repo-root>
#
# Needs TAURI_SIGNING_PRIVATE_KEY (and TAURI_SIGNING_PRIVATE_KEY_PASSWORD when the key has one)
# and the public key in src-tauri/tauri.conf.json (plugins.updater.pubkey); see
# docs/release-signing.md. Fails when either is missing, so a release never ships without
# updater signatures. Each signature is checked against the public key the app carries, so a
# key that does not match the app fails here and not on users' machines.
set -euo pipefail

version=${1:?version}
root=${2:?repo root}
repo=${GITHUB_REPOSITORY:-Mumcord/mumcord}

if [[ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ]]; then
  echo "::error::The secret TAURI_SIGNING_PRIVATE_KEY is missing: release bundles must carry updater signatures (docs/release-signing.md)."
  exit 1
fi
pubkey=$(jq -r '.plugins.updater.pubkey // ""' "$root/src-tauri/tauri.conf.json")
if [[ -z "$pubkey" ]]; then
  echo "::error::src-tauri/tauri.conf.json has no plugins.updater.pubkey: add the updater public key (docs/release-signing.md)."
  exit 1
fi
# During a key rotation the release is still signed with the old key, which the installed apps
# trust, while the app it ships already carries the new public key (docs/release-signing.md).
verify_key=${UPDATER_ROTATION_OLD_PUBKEY:-$pubkey}

# The manifest key for each bundle: {os}-{arch}-{installer}, as the app asks for it
# (src-tauri/src/updater/install_kind.rs). The universal macOS app serves both CPUs.
declare -A targets=(
  ["Mumcord_${version}_amd64.AppImage"]="linux-x86_64-appimage"
  ["Mumcord_${version}_aarch64.AppImage"]="linux-aarch64-appimage"
  ["Mumcord_${version}_x64-setup.exe"]="windows-x86_64-nsis"
  ["Mumcord_${version}_universal.app.tar.gz"]="darwin-aarch64-app darwin-x86_64-app"
)

tauri=(npx --yes "@tauri-apps/cli@$(jq -r '.devDependencies["@tauri-apps/cli"] | ltrimstr("^")' "$root/package.json")")
platforms='{}'
for file in "${!targets[@]}"; do
  if [[ ! -f "$file" ]]; then
    echo "::error::$file is missing; the updater needs it."
    exit 1
  fi
  # --app-version binds the signature to this release, so an old bundle cannot pose as a new one.
  "${tauri[@]}" signer sign --app-version "$version" "$file" >/dev/null
  node "$root/scripts/verify-updater-signature.js" "$file" "$file.sig" "$verify_key" "$version"
  url="https://github.com/$repo/releases/download/v$version/$file"
  for target in ${targets[$file]}; do
    platforms=$(jq --arg t "$target" --arg url "$url" --rawfile sig "$file.sig" \
      '.[$t] = {url: $url, signature: ($sig | rtrimstr("\n"))}' <<< "$platforms")
  done
done

jq -n --arg version "$version" --arg date "$(date -u +%Y-%m-%dT%H:%M:%SZ)" --argjson platforms "$platforms" \
  '{version: $version, notes: "", pub_date: $date, platforms: $platforms}' > latest.json
cat latest.json
