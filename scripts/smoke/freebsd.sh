#!/usr/bin/env bash
# Installs and smoke-tests a release package in the build VM. Run as root on FreeBSD:
#   bash scripts/smoke/freebsd.sh <package.pkg> <version> <seed> <observer> [port]
# Requires mumble-server, xorg-vfbserver, xdpyinfo and dbus from the package collection.
# All server data, display state and client identity/profile are disposable and local.
set -euo pipefail
package=${1:?package missing}
version=${2:?release version missing}
seed=${3:?seeder missing}
observer=${4:?observer missing}
port=${5:-64738}
root=$(cd "$(dirname "$0")/../.." && pwd)

[[ $(uname -s) == FreeBSD ]] || { echo "FreeBSD only" >&2; exit 2; }
[[ $(id -u) == 0 ]] || { echo "run as root to install the package" >&2; exit 2; }

# Never ignore dependencies (-M), force the install (-f), or let `pkg check` repair a bad build.
pkg add "$package"
pkg check -dn mumcord
[[ $(pkg query '%v' mumcord) == "$version" ]] || { echo "wrong package version" >&2; exit 1; }

work=$(mktemp -d)
server_pid="" display_pid=""
# shellcheck disable=SC2329 # invoked by the EXIT trap
cleanup() {
  status=$?
  trap - EXIT
  if ((status != 0)); then
    for log in "$work/server.log" "$work/display.log"; do
      [[ ! -f $log ]] || { echo "== $log"; tail -n 80 "$log"; }
    done
  fi
  [[ -z $server_pid ]] || { kill "$server_pid" 2>/dev/null || true; wait "$server_pid" 2>/dev/null || true; }
  [[ -z $display_pid ]] || { kill "$display_pid" 2>/dev/null || true; wait "$display_pid" 2>/dev/null || true; }
  rm -rf "$work"
  exit "$status"
}
trap cleanup EXIT

# The VM still has build dependencies installed. Detect omissions from the package manifest
# even when the undeclared library happens to be available on this machine.
pkg query '%do' mumcord > "$work/deps"
linked=$(ldd /usr/local/bin/mumcord)
if grep -q 'not found' <<< "$linked"; then
  echo "$linked" >&2; exit 1
fi
while read -r lib; do
  owner=$(pkg which -q -o "$lib")
  grep -Fxq "$owner" "$work/deps" || { echo "undeclared runtime dependency: $owner ($lib)" >&2; exit 1; }
done < <(awk '/=> \/usr\/local\// {print $3}' <<< "$linked" | sort -u)
# These are loaded by WebKit, not linked into the executable (build-freebsd-pkg.sh).
pkg query '%dn' mumcord > "$work/dep-names"
for dep in gstreamer1-libav gstreamer1-plugins-vpx gstreamer1-plugins-opus; do
  grep -Fxq "$dep" "$work/dep-names" || { echo "undeclared runtime dependency: $dep" >&2; exit 1; }
done

# No user profile, keyring, desktop session or already-running server is reused.
export HOME="$work/home" XDG_DATA_HOME="$work/home/data" XDG_CONFIG_HOME="$work/home/config"
export XDG_CACHE_HOME="$work/home/cache" XDG_RUNTIME_DIR="$work/runtime"
mkdir -p "$HOME" "$XDG_RUNTIME_DIR"
chmod 700 "$HOME" "$XDG_RUNTIME_DIR"
# The FreeBSD server drops privileges to its package account. Let it traverse the workspace
# and write only its own database directory; the client profile/identity stays root-private.
chmod 711 "$work"
install -d -m 700 -o mumble-server -g mumble-server "$work/server"
cat > "$work/server.ini" <<EOF
host=127.0.0.1
database=$work/server/server.sqlite
logfile=
port=$port
autobanAttempts=0
EOF
mumble-server -ini "$work/server.ini" -fg > "$work/server.log" 2>&1 &
server_pid=$!
ready=""
for ((i = 0; i < 60; i++)); do
  kill -0 "$server_pid" 2>/dev/null || break
  if nc -z 127.0.0.1 "$port"; then ready=1; break; fi
  sleep 1
done
[[ -n $ready ]] || { echo "local Mumble server did not start" >&2; exit 1; }

# -displayfd selects a free X display, without xvfb-run (not shipped by FreeBSD).
Xvfb -displayfd 3 -screen 0 1280x720x24 -nolisten tcp 3> "$work/display" > "$work/display.log" 2>&1 &
display_pid=$!
ready=""
for ((i = 0; i < 30; i++)); do
  kill -0 "$display_pid" 2>/dev/null || break
  if [[ -s $work/display ]]; then
    DISPLAY=":$(< "$work/display")"
    export DISPLAY
    if xdpyinfo >/dev/null 2>&1; then ready=1; break; fi
  fi
  sleep 1
done
[[ -n $ready ]] || { echo "headless display did not start" >&2; exit 1; }

# Unlike Linux, FreeBSD's SecretStore uses the encrypted file in the isolated profile, so no
# gnome-keyring is needed. D-Bus is still provided for the GTK/WebKit desktop integration.
CI=1 dbus-run-session -- bash "$root/scripts/smoke/smoke.sh" --app /usr/local/bin/mumcord \
  --seed "$seed" --observer "$observer" --port "$port" --version "$version"
echo "== PASS FreeBSD package: installed, runtime dependencies declared, connected, version=$version"
