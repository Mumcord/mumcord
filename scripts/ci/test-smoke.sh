#!/usr/bin/env bash
# Exercises the packaged-app smoke helper without a display, server or real user profile.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/bin"
cat > "$work/bin/uname" <<'SH'
#!/usr/bin/env bash
printf '%s\n' "${TEST_OS:-FreeBSD}"
SH
cat > "$work/bin/app" <<'SH'
#!/usr/bin/env bash
logs="$XDG_DATA_HOME/app.mumcord.client/logs"
mkdir -p "$logs"
printf 'INFO version=%s portable=false Mumcord starting\nINFO connected server_id=1\n' "${TEST_VERSION-9.8.7-rc.2}" > "$logs/app.log"
exec sleep 30
SH
cat > "$work/bin/seed" <<'SH'
#!/usr/bin/env bash
mkdir -p "$1"
SH
cat > "$work/bin/bot" <<'SH'
#!/usr/bin/env bash
exit "${TEST_BOT_EXIT:-0}"
SH
cat > "$work/bin/observer" <<'SH'
#!/usr/bin/env bash
[[ $# == 3 && $1 == 64738 && $2 == SmokeTester && $3 == 2 ]] || exit 2
exit "${TEST_OBSERVER_EXIT:-0}"
SH
chmod +x "$work/bin/"*
export PATH="$work/bin:$PATH" CI=1 SMOKE_TIMEOUT=2

run_case() {
  local name=$1 expected=$2
  shift 2
  export HOME="$work/$name" XDG_DATA_HOME="$work/$name/data"
  mkdir -p "$HOME"
  local status=0
  local verifier=(--bot "$work/bin/bot")
  [[ ${TEST_NATIVE:-} != 1 ]] || verifier=(--observer "$work/bin/observer")
  bash "$root/scripts/smoke/smoke.sh" --app "$work/bin/app" --seed "$work/bin/seed" \
    "${verifier[@]}" "$@" > "$work/$name.out" 2>&1 || status=$?
  if [[ $status != "$expected" ]]; then
    cat "$work/$name.out"
    echo "FAIL $name: expected exit $expected, got $status" >&2
    exit 1
  fi
  echo "PASS $name"
}

run_case freebsd-connect 0
run_case release-version 0 --version 9.8.7-rc.2
run_case wrong-version 1 --version 9.8.6
run_case version-prefix 1 --version 9.8.7
TEST_VERSION='' run_case missing-version 1 --version 9.8.7-rc.2
TEST_BOT_EXIT=1 run_case bot-failure 1 --version 9.8.7-rc.2
TEST_OS=Linux run_case linux-connect 0
TEST_NATIVE=1 TEST_BOT_EXIT=1 run_case native-observer 0 --version 9.8.7-rc.2
TEST_NATIVE=1 TEST_OBSERVER_EXIT=1 run_case observer-failure 1 --version 9.8.7-rc.2
