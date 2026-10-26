#!/usr/bin/env bash
# Prints "code=true" or "code=false": whether a change touches anything besides documentation.
# Pull requests are judged by their file list; every other trigger (push to main, manual run)
# counts as a code change so main is always fully tested. Needs GH_TOKEN, GITHUB_EVENT_NAME,
# GITHUB_REPOSITORY and, for pull requests, PR_NUMBER.
set -euo pipefail

if [[ "$GITHUB_EVENT_NAME" != "pull_request" ]]; then
  echo "code=true"
  exit 0
fi

# Documentation only: these paths never need a build or test run.
is_docs() {
  case "$1" in
    *.md | docs/* | specs/* | wiki/* | screenshots/* | LICENSE) return 0 ;;
    *) return 1 ;;
  esac
}

code=false
while IFS= read -r file; do
  if ! is_docs "$file"; then
    echo "code change: $file" >&2
    code=true
    break
  fi
done < <(gh api --paginate "repos/$GITHUB_REPOSITORY/pulls/$PR_NUMBER/files" -q '.[].filename')
echo "code=$code"
