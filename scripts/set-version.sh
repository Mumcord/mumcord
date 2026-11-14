#!/bin/bash
# Writes a version into every manifest and lock file before a build, so releases need no
# version-bump commit (RELEASING.md). CI runs it on a fresh checkout; the change is never committed.
#   scripts/set-version.sh <X.Y.Z[-pre]>
# Sets package.json and package-lock.json, the workspace version in Cargo.toml (members inherit it
# via version.workspace), the requirements of the internal crates crates.io needs (X.Y, or the
# exact version for a pre-release, which a plain X.Y requirement would not match), Cargo.lock and
# the smoke-test seeder's scripts/smoke/seed/Cargo.lock. Uses node and perl, which every CI
# runner (Linux, macOS, Git Bash on Windows) has; cargo may fetch the registry index for the
# seeder and the final --locked check. For a pre-release it also sets the macOS bundle versions
# in src-tauri/Info.plist to the plain X.Y.Z.
set -euo pipefail
version=${1:?version missing, e.g. 0.4.3}
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] \
  || { echo "'$version' is not a version like 1.2.3 or 1.2.3-rc.1" >&2; exit 2; }
if [[ "$version" == *-* ]]; then
  requirement=$version
else
  requirement=${version%.*}
fi
cd "$(dirname "$0")/.."

npm version "$version" --no-git-tag-version --allow-same-version >/dev/null

# Only the version line of [workspace.package] and the version of path dependencies on the
# workspace's own crates; any other version in the file belongs to a third-party crate.
VERSION=$version REQUIREMENT=$requirement perl -0pi -e '
  s/(^\[workspace\.package\]\n(?:(?!\[).*\n)*?)version = "[^"]*"/$1version = "$ENV{VERSION}"/m
    or die "no version in [workspace.package]\n";
  s/^(mumcord-[a-z0-9-]+ = \{ path = "[^"]+", version = )"[^"]*"/$1"$ENV{REQUIREMENT}"/mg;
' Cargo.toml

# Cargo.lock: only the workspace's own packages (path packages carry no `source`) get the new
# version. Letting cargo re-resolve the whole lock would also move unrelated dependencies away
# from what CI tested on main. For a patch release the edited lock is complete; a new X.Y
# requirement makes cargo re-resolve the internal crates, which the metadata run below writes.
members=$(cargo metadata --no-deps --offline --format-version 1 \
  | node -e 'const m = JSON.parse(require("fs").readFileSync(0, "utf8"));
             console.log(m.packages.map((p) => p.name).join(" "))')
MEMBERS=$members VERSION=$version perl -00 -pi -e '
  BEGIN { %member = map { $_ => 1 } split / /, $ENV{MEMBERS} }
  s/^version = "[^"]*"$/version = "$ENV{VERSION}"/m
    if /^name = "([^"]+)"$/m && $member{$1} && !/^source = /m;
' Cargo.lock
cargo metadata --format-version 1 >/dev/null
# The seeder is its own Cargo project, a CI helper whose lock cargo may refresh freely.
cargo update --manifest-path scripts/smoke/seed/Cargo.toml --quiet -p mumcord-store

# macOS bundle versions allow digits and dots only, and Tauri would write the full version
# (0.8.0-rc.1) into both. For a pre-release the bundle carries X.Y.Z; the app's About shows the
# full version. Tauri lets src-tauri/Info.plist override its generated keys.
if [[ "$version" == *-* ]]; then
  BASE=${version%%-*} perl -0pi -e '
    /<key>CFBundle(Short)?Version(String)?<\/key>/ and die "Info.plist already sets a bundle version\n";
    s{(</dict>\s*</plist>\s*)\z}{  <key>CFBundleShortVersionString</key>\n  <string>$ENV{BASE}</string>\n  <key>CFBundleVersion</key>\n  <string>$ENV{BASE}</string>\n$1}
      or die "no closing </dict> in Info.plist\n";
  ' src-tauri/Info.plist
fi

echo "Version set to $version (internal crate requirement $requirement)."
