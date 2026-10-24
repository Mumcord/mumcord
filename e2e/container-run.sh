#!/bin/bash
# Runs inside the E2E container: session D-Bus with an unlocked Secret Service (gnome-keyring),
# Xvfb, then builds the app (debug, feature test-backend), bot and netsim, then runs WebdriverIO.
set -euo pipefail

# The container runs as the host uid (e.g. 1001 on GitHub runners), which has no /etc/passwd entry
# here; dbus-daemon refuses to start without one. Fake the entry with nss_wrapper.
if ! getent passwd "$(id -u)" >/dev/null; then
  nss_dir="$(mktemp -d)"
  cat /etc/passwd > "$nss_dir/passwd"
  echo "e2e:x:$(id -u):$(id -g):e2e:${HOME:-/cache/home}:/bin/bash" >> "$nss_dir/passwd"
  cat /etc/group > "$nss_dir/group"
  getent group "$(id -g)" >/dev/null || echo "e2e:x:$(id -g):" >> "$nss_dir/group"
  export NSS_WRAPPER_PASSWD="$nss_dir/passwd" NSS_WRAPPER_GROUP="$nss_dir/group"
  export LD_PRELOAD="$(ls /usr/lib/*/libnss_wrapper.so | head -n1)${LD_PRELOAD:+:$LD_PRELOAD}"
fi

if [ -z "${DBUS_SESSION_BUS_ADDRESS:-}" ]; then
  exec dbus-run-session -- "$0" "$@"
fi

# A fixed non-empty password unlocks (an empty one creates no login collection) the login keyring non-interactively; the keyring crate talks to it via
# the Secret Service D-Bus API, exactly like on a desktop.
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-$(mktemp -d)}"
# Fresh keyring per run so state cannot leak between runs.
rm -f "$HOME"/.local/share/keyrings/*.keyring "$HOME"/.local/share/keyrings/default
printf 'e2e' | gnome-keyring-daemon --unlock --components=secrets >/dev/null
gnome-keyring-daemon --start --components=secrets >/dev/null

export DISPLAY=:99
Xvfb "$DISPLAY" -screen 0 1600x1000x24 -nolisten tcp >/dev/null 2>&1 &
xvfb_pid=$!
trap 'kill "$xvfb_pid" 2>/dev/null || true' EXIT

# WebKit's bubblewrap sandbox and DMA-BUF renderer do not work in an unprivileged container.
export WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1 WEBKIT_DISABLE_DMABUF_RENDERER=1
export LIBGL_ALWAYS_SOFTWARE=1 NO_AT_BRIDGE=1

[ -d node_modules ] || npm ci --no-audit --no-fund
echo "building app (debug, test-backend), mumcord-bot, netsim..."
npx tauri build --debug --no-bundle --features test-backend -- --locked
cargo build --locked --quiet -p mumcord-bot -p netsim

exec npx wdio run e2e/wdio.conf.ts "$@"
