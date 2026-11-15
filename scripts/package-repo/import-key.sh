#!/bin/bash
# Imports the package signing key from $PACKAGE_REPO_GPG_KEY into a fresh $GNUPGHOME and prints
# its fingerprint. Fails when the secret does not belong to the public key committed in
# packaging/repo/mumcord.asc, so a wrong or rotated secret never signs a repository that users'
# keyrings would then reject.
#
# Usage: GNUPGHOME=<empty dir> PACKAGE_REPO_GPG_KEY=<armored secret key> import-key.sh
set -euo pipefail

: "${GNUPGHOME:?GNUPGHOME must name an empty directory}"
: "${PACKAGE_REPO_GPG_KEY:?PACKAGE_REPO_GPG_KEY is not set}"
root=$(cd "$(dirname "$0")/../.." && pwd)

chmod 700 "$GNUPGHOME"
gpg --batch --quiet --import <<< "$PACKAGE_REPO_GPG_KEY" 2>/dev/null
expected=$(gpg --batch --with-colons --show-keys "$root/packaging/repo/mumcord.asc" | awk -F: '/^fpr/ { print $10; exit }')
if ! gpg --batch --with-colons --list-secret-keys "$expected" >/dev/null 2>&1; then
  echo "::error::The signing secret does not match packaging/repo/mumcord.asc ($expected)." >&2
  exit 1
fi
echo "$expected"
