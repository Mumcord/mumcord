#!/usr/bin/env bash
# Lints the GitHub workflows with actionlint and every shell script with shellcheck, exactly as
# the "Lint workflows and scripts" CI job does. Needs both tools on PATH (CI pins the versions in the
# "Lint workflows and scripts" job of .github/workflows/ci.yml). Run from anywhere: ./scripts/ci/lint-ci.sh
set -euo pipefail

cd "$(dirname "$0")/../.."

for tool in actionlint shellcheck; do
  command -v "$tool" >/dev/null || { echo "$tool is not installed" >&2; exit 1; }
done

status=0

echo "actionlint"
actionlint || status=1

# The shell comes from each script's shebang; the POSIX installer must stay POSIX, so it is
# checked as sh whatever its shebang says. -x follows the `source`d env files.
echo "shellcheck"
mapfile -t scripts < <(git ls-files '*.sh')
for script in "${scripts[@]}"; do
  shell_args=()
  [[ "$script" == scripts/install.sh ]] && shell_args=(-s sh)
  shellcheck -x --source-path=SCRIPTDIR "${shell_args[@]}" "$script" || status=1
done
exit "$status"
