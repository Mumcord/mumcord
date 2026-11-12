#!/bin/bash
# Runs inside the E2E container: session D-Bus with an unlocked Secret Service (gnome-keyring),
# Xvfb, then builds the app (debug, feature test-backend), netsim, the test game plugin and the
# in-game overlay check, fetches the pinned mumbot test bot, then runs WebdriverIO.
# MUMCORD_E2E_SHARD=<n> limits the run to one shard of e2e/shards.json (see wdio.conf.ts).
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
  nss_wrapper_lib=$(find /usr/lib -name libnss_wrapper.so -print -quit)
  export LD_PRELOAD="$nss_wrapper_lib${LD_PRELOAD:+:$LD_PRELOAD}"
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

# The in-game overlay listens on an abstract Unix socket, and abstract sockets live in the network
# namespace the container shares with the host (--network host). A Mumcord running on the host for
# the same uid holds the default name, so the app under test and the layer use a name of their own.
# Parallel runs (MUMCORD_E2E_RUN) share that namespace too, and every container has its own PIDs.
run="${MUMCORD_E2E_RUN:-}"
export MUMCORD_OVERLAY_SOCKET="mumcord-overlay-e2e-${run:-$$}"

# The X socket is abstract as well, so parallel runs need a display of their own.
export DISPLAY=":$((99 + ${run:-0}))"
Xvfb "$DISPLAY" -screen 0 1600x1000x24 -nolisten tcp -noreset >/dev/null 2>&1 &
xvfb_pid=$!
pids=("$xvfb_pid")
trap 'kill "${pids[@]}" 2>/dev/null || true' EXIT

# Ubuntu 26.04's virtual-terminal function-key mappings make xdotool add Alt to plain F7/F9.
# No virtual terminals exist in Xvfb; use ordinary function keys and retain the map across clients.
for _ in $(seq 50); do
  xdotool getdisplaygeometry >/dev/null 2>&1 && break
  sleep 0.1
done
setxkbmap -layout us -option srvrkeys:none

# Sound daemon for the share-sound specs. MUMCORD_E2E_AUDIO=pipewire (default) starts PipeWire and
# WirePlumber with a null sink as the default output, so test apps (helpers/pwapps.ts) have
# somewhere to play; =pulse starts only PulseAudio (no PipeWire: AC-7.66); =none starts nothing.
# The sockets live in this run's own XDG_RUNTIME_DIR, so parallel runs never meet.
audio="${MUMCORD_E2E_AUDIO:-pipewire}"
case "$audio" in
  pipewire)
    pipewire >/dev/null 2>&1 &
    pids+=($!)
    for _ in $(seq 50); do pw-cli info 0 >/dev/null 2>&1 && break; sleep 0.1; done
    # No Bluetooth: its logind module has no system bus here and ends WirePlumber.
    sed '/bluetooth.lua/d' /usr/share/wireplumber/wireplumber.conf > "$XDG_RUNTIME_DIR/wireplumber.conf"
    wireplumber -c "$XDG_RUNTIME_DIR/wireplumber.conf" >/dev/null 2>&1 &
    pids+=($!)
    pw-cli create-node adapter \
      '{ factory.name=support.null-audio-sink node.name=e2e-sink media.class=Audio/Sink object.linger=true audio.position=[FL FR] }' >/dev/null
    ;;
  pulse)
    pulseaudio --daemonize=no --exit-idle-time=-1 --load="module-null-sink sink_name=e2e-sink" >/dev/null 2>&1 &
    pids+=($!)
    ;;
  none) ;;
  *) echo "MUMCORD_E2E_AUDIO must be pipewire, pulse or none, got '$audio'" >&2; exit 2 ;;
esac

# WebKit's bubblewrap sandbox and DMA-BUF renderer do not work in an unprivileged container.
export WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1 WEBKIT_DISABLE_DMABUF_RENDERER=1
export LIBGL_ALWAYS_SOFTWARE=1 NO_AT_BRIDGE=1

[ -d node_modules ] || npm ci --no-audit --no-fund
# MUMCORD_E2E_STEP=build|test splits the run for CI: one job builds, shard jobs test the binaries
# it shared. Unset = both.
step="${MUMCORD_E2E_STEP:-}"
if [ "$step" != test ]; then
  # Screen sharing links static SVT-AV1 and dav1d; Ubuntu's packages are too old. The scripts
  # return at once when the library is already in the native cache.
  if [ -z "${SVTAV1_STATIC_DIR:-}" ]; then
    scripts/build-svt-av1.sh /cache/native/svt-av1
    export SVTAV1_STATIC_DIR=/cache/native/svt-av1
  fi
  if [ -z "${DAV1D_STATIC_DIR:-}" ]; then
    scripts/build-dav1d.sh /cache/native/dav1d
    export DAV1D_STATIC_DIR=/cache/native/dav1d
  fi
  echo "building app (debug, test-backend), netsim, test game plugin..."
  npx tauri build --debug --no-bundle --features test-backend -- --locked
  cargo build --locked --quiet -p netsim
  cargo build --locked --quiet -p mumcord-positional --example test_plugin
  cargo build --locked --quiet -p mumcord-overlay-layer --example headless_check
  # Next to the debug binaries, so CI ships it to the shard jobs with them.
  scripts/fetch-mumbot.sh linux-x86_64 "$CARGO_TARGET_DIR/debug"
fi
[ "$step" = build ] && exit 0

# The bot is a release binary, not built from this repository: point it at this checkout's test
# servers (SuperUser password) and at a fixed place for its certificates (AC-1.2 imports one).
export MUMCORD_TESTENV_DIR="$PWD/testenv" MUMCORD_BOT_DATA="$HOME/.local/share/mumbot"

exec npx wdio run e2e/wdio.conf.ts "$@"
