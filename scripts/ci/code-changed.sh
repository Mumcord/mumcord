#!/usr/bin/env bash
# Prints "code=true" or "code=false": whether a change touches source code or a build/test input.
# Pull requests are judged by their file list against an allowlist (see is_code); anything not
# listed (docs, specs, wiki, screenshots, LICENSE, .github/*, scripts/ci/*, ...) skips the build
# and E2E jobs. Every other trigger (push to main, manual run) counts as a code change so main is
# always fully tested. Because workflow and CI-script changes alone skip the jobs, they are only
# fully exercised on the push to main or via workflow_dispatch. Needs GH_TOKEN, GITHUB_EVENT_NAME,
# GITHUB_REPOSITORY and, for pull requests, PR_NUMBER.
set -euo pipefail

if [[ "$GITHUB_EVENT_NAME" != "pull_request" ]]; then
  echo "code=true"
  exit 0
fi

# Allowlist: only these paths can change what gets built or tested.
is_code() {
  case "$1" in
    scripts/ci/*) return 1 ;;
    # Rust
    *.rs | Cargo.toml | */Cargo.toml | Cargo.lock | */Cargo.lock | .cargo/* | rust-toolchain* \
      | clippy.toml | rustfmt.toml | about.toml | crates/* | tools/* | proto/*) return 0 ;;
    # React / TypeScript UI
    ui/* | *.ts | *.tsx | *.js | *.mjs | *.cjs | *.css | *.html | package.json | package-lock.json \
      | tsconfig*.json | vite.config.* | vitest.config.* | eslint.config.* | assets/*) return 0 ;;
    # App build and test inputs
    src-tauri/* | e2e/* | testenv/* | scripts/* | licenses/* | THIRD_PARTY_NOTICES) return 0 ;;
    *) return 1 ;;
  esac
}

code=false
while IFS= read -r file; do
  if is_code "$file"; then
    echo "code change: $file" >&2
    code=true
    break
  fi
done < <(gh api --paginate "repos/$GITHUB_REPOSITORY/pulls/$PR_NUMBER/files" -q '.[].filename')
echo "code=$code"
