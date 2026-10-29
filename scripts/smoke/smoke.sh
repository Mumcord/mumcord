#!/usr/bin/env bash
# Packaging smoke test (AC-1.30): starts an installed/unpacked Mumcord build and checks that it
# connects to a local Mumble test server.
#
#   scripts/smoke/smoke.sh --app <app> --seed <mumcord-smoke-seed> --bot <mumbot> [--port N]
#
#   --app   Linux: the AppImage (or binary); Windows: Mumcord.exe; macOS: Mumcord.app
#   --seed  built scripts/smoke/seed binary
#   --bot   mumbot binary (scripts/fetch-mumbot.sh)
#   --port  test server port (default 64738)
#   --start-only  no server available: only check that the app starts and keeps running
#                 (no --seed/--bot needed; the connect part then belongs on the manual checklist)
#
# Steps: seed a profile (identity in the OS secure storage, wizard done, saved server with its
# certificate trusted) → start the app with a mumble:// link to that server → pass when the app
# log shows "connected" with a server_id AND mumbot sees the app's user on the server.
#
# The seed step replaces the identity in the OS secure storage, so the script only runs when
# CI is set or --allow-identity-overwrite is given (use a throwaway keyring/profile locally).
# Runs in bash on Linux, macOS and Windows (Git Bash).
set -euo pipefail

port=64738
app="" seed="" bot="" allow_overwrite="" start_only=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --app) app=$2; shift 2 ;;
    --seed) seed=$2; shift 2 ;;
    --bot) bot=$2; shift 2 ;;
    --port) port=$2; shift 2 ;;
    --allow-identity-overwrite) allow_overwrite=1; shift ;;
    --start-only) start_only=1; shift ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
[[ -n $app ]] || { echo "--app is required" >&2; exit 2; }
[[ -n $start_only || (-n $seed && -n $bot) ]] || { echo "--seed and --bot are required" >&2; exit 2; }
if [[ -z $start_only && -z ${CI:-} && -z $allow_overwrite ]]; then
  echo "refusing to overwrite the stored identity outside CI (pass --allow-identity-overwrite)" >&2
  exit 2
fi

readonly identifier=app.mumcord.client
readonly username=SmokeTester
readonly url="mumble://127.0.0.1:${port}/"
readonly timeout_secs=${SMOKE_TIMEOUT:-120}

# Tauri's app_data_dir / app_log_dir per OS.
case "$(uname -s)" in
  Linux)
    os=linux
    data_dir="${XDG_DATA_HOME:-$HOME/.local/share}/$identifier"
    log_dir="$data_dir/logs"
    ;;
  Darwin)
    os=macos
    data_dir="$HOME/Library/Application Support/$identifier"
    log_dir="$HOME/Library/Logs/$identifier"
    ;;
  MINGW* | MSYS* | CYGWIN*)
    os=windows
    data_dir="$(cygpath -u "$APPDATA")/$identifier"
    log_dir="$(cygpath -u "$LOCALAPPDATA")/$identifier/logs"
    ;;
  *) echo "unsupported OS: $(uname -s)" >&2; exit 2 ;;
esac

native_path() {
  if [[ $os == windows ]]; then cygpath -m "$1"; else printf '%s' "$1"; fi
}

if [[ -e "$data_dir/mumcord.sqlite3" ]]; then
  echo "a Mumcord profile already exists in $data_dir; use a fresh profile" >&2
  exit 2
fi

work=$(mktemp -d)
export MUMCORD_BOT_DATA="$work/bot"
app_pid=""

# shellcheck disable=SC2329 # only reached through cleanup, which the EXIT trap invokes
stop_app() {
  case $os in
    linux) [[ -n $app_pid ]] && kill "$app_pid" 2>/dev/null || true ;;
    macos) pkill -x Mumcord 2>/dev/null || pkill -x mumcord 2>/dev/null || true ;;
    windows) MSYS2_ARG_CONV_EXCL="*" taskkill /F /IM "$(basename "$app")" >/dev/null 2>&1 || true ;;
  esac
}

show_logs() {
  echo "== app log (no chat contents or secrets by design, see crates/engine/src/diag.rs)"
  cat "$log_dir"/* 2>/dev/null | tail -n 80 || echo "(no log in $log_dir)"
  [[ -s $work/app.out ]] && { echo "== app stdout/stderr"; tail -n 40 "$work/app.out"; }
  return 0
}

# shellcheck disable=SC2329 # only invoked through the EXIT trap
cleanup() {
  stop_app
  rm -rf "$work"
}
trap cleanup EXIT

start_app() {
  echo "== starting $app $*"
  case $os in
    macos)
      # A link reaches the app as an Apple "open URL" event, as with a browser click.
      open -a "$app" "$@" >"$work/app.out" 2>&1
      ;;
    *)
      "$app" "$@" >"$work/app.out" 2>&1 &
      app_pid=$!
      ;;
  esac
}

app_running() {
  case $os in
    macos) pgrep -x Mumcord >/dev/null || pgrep -x mumcord >/dev/null ;;
    *) kill -0 "$app_pid" 2>/dev/null ;;
  esac
}

if [[ -n $start_only ]]; then
  start_app
  sleep 20
  if app_running && compgen -G "$log_dir/*" >/dev/null; then
    echo "== PASS (start only): app is running and writes its log; connect is a manual check"
    exit 0
  fi
  echo "== FAIL: app not running or no log after 20 s" >&2
  show_logs
  exit 1
fi

echo "== seeding profile in $data_dir"
"$seed" "$(native_path "$data_dir")" "$port" "$username"

# The bot waits (up to the timeout) for the app's user to appear; start it first.
echo "== waiting for $username with mumbot"
"$bot" --port "$port" --name SmokeBot --timeout "$timeout_secs" expect user "$username" \
  >"$work/bot.out" 2>&1 &
bot_pid=$!

start_app "$url"

log_ok=""
for ((i = 0; i < timeout_secs; i++)); do
  if grep -qsE '(^|[^a-z])connected([^a-z]|$).*server_id' "$log_dir"/*; then
    log_ok=1
    break
  fi
  if ((i > 10)) && ! app_running; then
    echo "app exited early" >&2
    break
  fi
  sleep 1
done

bot_ok=""
if wait "$bot_pid"; then bot_ok=1; fi
cat "$work/bot.out"

if [[ -n $log_ok && -n $bot_ok ]]; then
  echo "== PASS: app started and connected (log + bot)"
  exit 0
fi
echo "== FAIL: app log connected=${log_ok:-no}, bot saw user=${bot_ok:-no}" >&2
show_logs
exit 1
