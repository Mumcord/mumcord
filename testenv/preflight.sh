#!/bin/bash
# Usage: ./preflight.sh
#
# Checks that this machine can run the 0.9.0 acceptance run
# (specs/07-08-bovnar-media-streams-0.9.0-acceptance-inventory.md). Fails fast instead of after
# hours: a missing piece here is what stops a whole family of ACs.
set -uo pipefail
cd "$(dirname "$0")/.."

failed=0
ok() { printf '  ok    %s\n' "$1"; }
bad() { printf '  FAIL  %s\n' "$1"; failed=$((failed + 1)); }
warn() { printf '  warn  %s\n' "$1"; }

echo "Hardware"
# SVT-AV1 refuses to open on a CPU without AVX2 (crates/screenshare/src/av1_svt.rs), so no share
# and no camera can start there.
case "$(uname -m)" in
  x86_64 | amd64)
    if grep -qw avx2 /proc/cpuinfo; then ok "CPU has AVX2"; else bad "CPU has no AVX2: SVT-AV1 cannot encode, no share or camera starts"; fi
    ;;
  *) warn "not x86: check AV1 encoding by hand" ;;
esac

echo "Containers"
if command -v docker >/dev/null && docker compose version >/dev/null 2>&1; then
  ok "docker with compose"
else
  # Rootless podman is not a substitute, even through a `docker` shim: it maps the host user to
  # container root, so the server cannot read the 0600 testenv/certs/A/key.pem and exits.
  if command -v podman >/dev/null; then
    warn "podman is present but the testenv needs docker: rootless podman cannot read the certificate key"
  fi
  bad "docker with compose missing (testenv/up.sh and the mumpeer testenv call 'docker compose')"
fi

echo "Privileges"
if sudo -n true 2>/dev/null; then ok "passwordless sudo"; else bad "sudo needs a password: the netns cells (AC-7.77 c, AC-7.86, AC-8.38) cannot run"; fi

echo "Tools"
for tool in sqlite3 node npm cargo; do
  if command -v "$tool" >/dev/null; then ok "$tool"; else bad "$tool missing"; fi
done

echo "SVT-AV1 4.0+"
static="${SVTAV1_STATIC_DIR:-$HOME/.cache/mumcord-build/svt-av1}"
if [[ -f "$static/lib/libSvtAv1Enc.a" ]]; then
  ok "static build in $static (export SVTAV1_STATIC_DIR=$static)"
elif pkg-config --atleast-version=4.0 SvtAv1Enc 2>/dev/null; then
  ok "system SvtAv1Enc $(pkg-config --modversion SvtAv1Enc)"
else
  bad "none: run scripts/build-svt-av1.sh ~/.cache/mumcord-build/svt-av1 and export SVTAV1_STATIC_DIR"
fi

echo "Mumble test server"
if [[ -f testenv/.env ]]; then
  ok "testenv/.env"
else
  bad "testenv/.env missing: cp testenv/.env.example testenv/.env and set SUPERUSER_PASSWORD"
fi

echo "Relay"
mumpeer="${MUMPEER_DIR:-../mumpeer}"
if [[ -f "$mumpeer/src/broadcast/media.rs" ]]; then
  ok "mumpeer at $mumpeer carries the media profile"
else
  bad "mumpeer at $mumpeer has no media profile (src/broadcast/media.rs): check out the 0.3.0 work, the new-relay cells need it"
fi

echo "Old client"
if [[ -n "${MUMCORD_V080_BINARY:-}" && -x "${MUMCORD_V080_BINARY}" ]]; then
  ok "0.8.0 build at $MUMCORD_V080_BINARY"
else
  bad "no 0.8.0 build: tag v0.8.0 must be built, set MUMCORD_V080_BINARY (the old-client ACs need it)"
fi

echo "Screen"
if [[ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]]; then
  ok "DISPLAY or WAYLAND_DISPLAY set"
else
  bad "no X11 or Wayland session: the E2E run and the screenshots need one"
fi

echo
if ((failed)); then
  echo "$failed check(s) failed. The acceptance run cannot pass on this machine."
  exit 1
fi
echo "All checks passed."
