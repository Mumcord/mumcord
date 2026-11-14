#!/bin/bash
# Tests scripts/set-version.sh on a temporary copy of the committed tree: every manifest and lock
# file carries the new version, the locks stay valid for --locked builds, and mumcord-codec
# still packages for crates.io. The real release runs the same script in release.yml.
#   scripts/ci/test-set-version.sh [version]   (default 9.8.7; CI also runs a pre-release)
set -euo pipefail
version=${1:-9.8.7}
repo=$(cd "$(dirname "$0")/../.." && pwd)
copy=$(mktemp -d)
trap 'rm -rf "$copy"' EXIT
fail() { echo "FAIL: $1" >&2; exit 1; }

git -C "$repo" archive HEAD | tar -x -C "$copy"
# Reuse the build cache; the copy itself is thrown away.
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$repo/target}
cd "$copy"
scripts/set-version.sh "$version"

[[ "$(node -p "require('./package.json').version")" == "$version" ]] || fail "package.json"
[[ "$(node -p "require('./package-lock.json').version")" == "$version" ]] || fail "package-lock.json"
[[ "$(node -p "require('./package-lock.json').packages[''].version")" == "$version" ]] \
  || fail "package-lock.json root package"

cargo metadata --locked --no-deps --format-version 1 \
  | node -e '
    const want = process.argv[1];
    const meta = JSON.parse(require("fs").readFileSync(0, "utf8"));
    const wrong = meta.packages.filter((p) => p.version !== want).map((p) => p.name);
    if (wrong.length) { console.error("crates not at " + want + ": " + wrong); process.exit(1); }
  ' "$version" || fail "workspace crate versions"
cargo metadata --locked --format-version 1 >/dev/null || fail "Cargo.lock is stale"
cargo metadata --locked --format-version 1 --manifest-path scripts/smoke/seed/Cargo.toml >/dev/null \
  || fail "scripts/smoke/seed/Cargo.lock is stale"

requirement=$version
[[ "$version" == *-* ]] || requirement=${version%.*}
grep -q "^mumcord-codec = { path = \"crates/codec\", version = \"$requirement\" }" Cargo.toml \
  || fail "internal crate requirement is not $requirement"

plist=src-tauri/Info.plist
if [[ "$version" == *-* ]]; then
  base=${version%%-*}
  for key in CFBundleShortVersionString CFBundleVersion; do
    grep -A1 "<key>$key</key>" "$plist" | grep -q "<string>$base</string>" \
      || fail "$plist: $key is not $base"
  done
  tail -n 1 "$plist" | grep -qx '</plist>' || fail "$plist no longer ends with </plist>"
else
  cmp -s "$plist" "$repo/$plist" || fail "$plist changed for a release version"
fi

if [[ "$version" != *-* ]]; then
  # --allow-dirty: the copy has no git repository.
  cargo publish -p mumcord-codec --dry-run --locked --allow-dirty --quiet \
    || fail "cargo publish --dry-run of mumcord-codec"
fi
echo "set-version.sh $version: all manifests and locks consistent."
