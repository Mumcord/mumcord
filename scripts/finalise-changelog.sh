#!/bin/bash
# Finalises CHANGELOG.md for a published release (.github/workflows/post-release.yml):
#   scripts/finalise-changelog.sh <X.Y.Z> <YYYY-MM-DD>
# - "## [X.Y.Z] - Unreleased" gets the release date. Without such a heading, a filled
#   "## [Unreleased]" section becomes "## [X.Y.Z] - <date>". An already dated section stays as is.
# - Adds the "[X.Y.Z]: …/releases/tag/vX.Y.Z" link to the link list at the bottom, if missing.
# - Adds an empty "## [Unreleased]" section on top, unless an open section already exists: a plain
#   "## [Unreleased]" or a "## [X.Y.Z] - Unreleased" of the next version.
# Running it twice changes nothing more.
set -euo pipefail
version=${1:?version missing, e.g. 0.4.3}
date=${2:?date missing, e.g. 2026-10-30}
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "'$version' is not a version like 1.2.3" >&2; exit 2; }
[[ "$date" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}$ ]] || { echo "'$date' is not a date like 2026-10-30" >&2; exit 2; }
cd "$(dirname "$0")/.."

VERSION=$version DATE=$date perl -0pi -e '
  my ($v, $d) = @ENV{qw(VERSION DATE)};
  my $has_section = /^## \[\Q$v\E\]/m;
  s/^## \[\Q$v\E\] - Unreleased[ \t]*$/## [$v] - $d/m;
  # A filled "[Unreleased]" section is the release itself when the version has no section yet.
  if (!$has_section && /^## \[Unreleased\][ \t]*\n(?!\s*(?:## |\[|\z))/m) {
    s/^## \[Unreleased\][ \t]*$/## [$v] - $d/m;
  }
  s/^(\[[0-9][^\n]*\n)/[$v]: https:\/\/github.com\/Mumcord\/mumcord\/releases\/tag\/v$v\n$1/m
    if /^\[[0-9]/m && !/^\[\Q$v\E\]:/m;
  s/^(# Changelog\n\n)/$1## [Unreleased]\n\n/ unless /^## \[(?:[0-9]+\.[0-9]+\.[0-9]+\] - )?Unreleased\]?[ \t]*$/m;
' CHANGELOG.md
echo "CHANGELOG.md finalised for $version ($date)."
