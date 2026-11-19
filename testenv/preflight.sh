#!/bin/bash
# Usage: ./preflight.sh
#
# Checks that this machine can run the 0.9.0 acceptance run
# (specs/07-08-bovnar-media-streams-0.9.0-acceptance-inventory.md). Fails fast instead of after
# hours: a missing piece here is what stops a whole family of ACs.
set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

failed=0
ok() { printf '  ok    %s\n' "$1"; }
bad() { printf '  FAIL  %s\n' "$1"; failed=$((failed + 1)); }
warn() { printf '  warn  %s\n' "$1"; }

# True when $1 is 4.3.0 or newer, the first SVT-AV1 release that runs on CPUs without AVX2.
newer_than_4_3() {
  local major minor
  IFS=. read -r major minor _ <<<"$1"
  (( ${major:-0} > 4 || (${major:-0} == 4 && ${minor:-0} >= 3) ))
}

echo "Hardware"
# The CPU is no longer a gate: SVT-AV1 4.3.0 fixes the SSSE3 load that killed the encoder on x86
# CPUs without AVX2. What must be current is the build (see "SVT-AV1 4.3+" below); 4.2.0 still
# crashes there. A CPU without AVX2 only means 1080p30 will not keep up (about 28 ms a frame on a
# 2011 i7), so it is a warning, not a failure.
case "$(uname -m)" in
  x86_64 | amd64)
    if grep -qw avx2 /proc/cpuinfo; then ok "CPU has AVX2"; else warn "CPU has no AVX2: SVT-AV1 4.3.0 runs, but 1080p30 will not keep up"; fi
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

echo "SVT-AV1 4.3+"
static="${SVTAV1_STATIC_DIR:-$HOME/.cache/mumcord-build/svt-av1}"
# The archive alone does not say which version it is: a prefix built from 4.2.0 looks exactly like
# a current one and crashes on CPUs without AVX2. The installed pkg-config file carries the
# version, so read it there.
static_version=$(sed -n 's/^Version: *//p' "$static/lib/pkgconfig/SvtAv1Enc.pc" 2>/dev/null)
if [[ -n $static_version && -f "$static/lib/libSvtAv1Enc.a" ]]; then
  if newer_than_4_3 "$static_version"; then
    ok "static build $static_version in $static (export SVTAV1_STATIC_DIR=$static)"
  else
    bad "static build $static_version in $static is older than 4.3.0 and crashes on CPUs without AVX2: rerun scripts/build-svt-av1.sh $static"
  fi
elif pkg-config --atleast-version=4.3 SvtAv1Enc 2>/dev/null; then
  ok "system SvtAv1Enc $(pkg-config --modversion SvtAv1Enc)"
else
  bad "none at 4.3.0 or newer: run scripts/build-svt-av1.sh ~/.cache/mumcord-build/svt-av1 and export SVTAV1_STATIC_DIR"
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
