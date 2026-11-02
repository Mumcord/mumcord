#!/bin/bash
# Host entry point for `npm run e2e [-- <wdio args>]`: builds the E2E image and runs the suite in it.
# Extra arguments go to `wdio run`, e.g. `npm run e2e -- --spec e2e/specs/smoke/app.e2e.ts`.
# MUMCORD_E2E_SHARD=<n> runs only shard n of e2e/shards.json; MUMCORD_E2E_STEP=build|test runs
# only that half (CI splits build and test into separate jobs).
# MUMCORD_E2E_RUN=<n> (1-9) runs the tests next to other runs on the same machine: run n gets its
# own display, driver ports, overlay socket, home dir, test server ports and compose project.
set -euo pipefail
repo="$(cd "$(dirname "$0")/.." && pwd)"
cache="${MUMCORD_E2E_CACHE:-$HOME/.cache/mumcord-build/e2e}"
image="${MUMCORD_E2E_IMAGE:-mumcord-e2e}"
run="${MUMCORD_E2E_RUN:-}"
home="$cache/home${run:+-$run}"
mkdir -p "$cache"/{target,cargo} "$home"

if [ -n "$run" ]; then
  if ! [[ "$run" =~ ^[1-9]$ ]]; then
    echo "MUMCORD_E2E_RUN must be 1-9, got '$run'" >&2
    exit 2
  fi
  # A build would rewrite the binaries the other runs are testing: build once, then test in parallel.
  if [ "${MUMCORD_E2E_STEP:-}" != test ]; then
    echo "MUMCORD_E2E_RUN needs MUMCORD_E2E_STEP=test; build first with MUMCORD_E2E_STEP=build" >&2
    exit 2
  fi
  # 4xxxx: the default base 64738 plus the netsim offsets would overflow above ~65400.
  export MUMCORD_PORT_BASE="${MUMCORD_PORT_BASE:-$((40000 + 1000 * run))}"
  export MUMCORD_PROJECT="${MUMCORD_PROJECT:-e2e-$run}"
fi

# Builds and default runs (default test servers and ports) take the lock alone; parallel test runs
# share it, so a build waits until they are done and never swaps the binaries under them.
lock_mode=-x
[ -n "$run" ] && lock_mode=-s
exec {lock_fd}>"$cache/run.lock"
flock "$lock_mode" "$lock_fd"

# Test servers need a .env; generate throwaway passwords when there is none (CI, fresh clones).
# A git worktree reuses the main checkout's .env so all checkouts agree on the shared servers' password.
primary_env="$(dirname "$(git -C "$repo" rev-parse --path-format=absolute --git-common-dir 2>/dev/null)")/testenv/.env"
if [ ! -f "$repo/testenv/.env" ] && [ -f "$primary_env" ]; then
  cp "$primary_env" "$repo/testenv/.env"
  echo "copied testenv/.env from the main checkout"
fi
if [ ! -f "$repo/testenv/.env" ]; then
  printf 'SUPERUSER_PASSWORD=%s\nSERVER_PASSWORD=\n' "$(openssl rand -hex 16)" > "$repo/testenv/.env"
  echo "created testenv/.env with a random SuperUser password"
fi

# MUMCORD_E2E_STEP=test (CI shard jobs) uses the image the build job shared via `docker load`;
# MUMCORD_E2E_IMAGE_BUILT=1 (CI build job) one that is already built with a layer cache.
if [ "${MUMCORD_E2E_STEP:-}" != test ] && [ -z "${MUMCORD_E2E_IMAGE_BUILT:-}" ]; then
  docker build -q -t "$image" "$repo/e2e" >/dev/null
fi

tty=(); [ -t 1 ] && tty=(-t)
# Extra `docker run` flags, e.g. MUMCORD_E2E_DOCKER_ARGS="--cpus=2" to mimic a 2-core CI runner.
read -r -a extra <<< "${MUMCORD_E2E_DOCKER_ARGS:-}"
# Plugin CLI and packages built on the host (scripts/e2e-plugin-registry.sh --build-only): the
# image has no wasm32 target, so the plugin specs take them from there instead of building.
if [ -n "${MUMCORD_E2E_PLUGIN_PACKAGES:-}" ]; then
  extra+=(-v "$MUMCORD_E2E_PLUGIN_PACKAGES:$MUMCORD_E2E_PLUGIN_PACKAGES:ro" -e MUMCORD_E2E_PLUGIN_PACKAGES)
fi
# Host network: the app, bots and netsim reach the testenv servers on localhost:64738-64741 and
# netsim on 64800/64801. The repo is mounted at its host path so docker compose (via the host
# socket) resolves testenv's relative volume paths to real host paths.
exec docker run --rm -i --init "${tty[@]}" \
  --network host \
  --user "$(id -u):$(id -g)" \
  --group-add "$(stat -c %g /var/run/docker.sock)" \
  --security-opt seccomp=unconfined \
  --shm-size 1g \
  -v /var/run/docker.sock:/var/run/docker.sock \
  -v "$repo:$repo" -w "$repo" \
  -v "$cache/target:/cache/target" \
  -v "$cache/cargo:/cache/cargo" \
  -v "$home:/cache/home" \
  -e HOME=/cache/home -e CARGO_HOME=/cache/cargo -e CARGO_TARGET_DIR=/cache/target \
  -e MUMCORD_SKIP_BINDINGS_EXPORT=1 \
  -e CI -e MUMCORD_E2E_SHARD -e MUMCORD_E2E_STEP -e MUMCORD_E2E_RUN -e MUMCORD_VERSION \
  -e MUMCORD_PORT_BASE -e MUMCORD_PROJECT \
  -e MUMCORD_E2E_RELAY -e MUMCORD_E2E_RELAY_PORT -e MUMCORD_E2E_RELAY_HOST \
  -e CARGO_PROFILE_DEV_DEBUG -e CARGO_INCREMENTAL \
  "${extra[@]}" \
  "$image" "$@"
