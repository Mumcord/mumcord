#!/bin/bash
# Host entry point for `npm run e2e [-- <wdio args>]`: builds the E2E image and runs the suite in it.
# Extra arguments go to `wdio run`, e.g. `npm run e2e -- --spec e2e/specs/smoke/app.e2e.ts`.
# MUMCORD_E2E_SHARD=<n> runs only shard n of e2e/shards.json; MUMCORD_E2E_STEP=build|test runs
# only that half (CI splits build and test into separate jobs).
set -euo pipefail
repo="$(cd "$(dirname "$0")/.." && pwd)"
cache="${MUMCORD_E2E_CACHE:-$HOME/.cache/mumcord-build/e2e}"
image="${MUMCORD_E2E_IMAGE:-mumcord-e2e}"
mkdir -p "$cache"/{target,cargo,home}

# Test servers need a .env; generate throwaway passwords when there is none (CI, fresh clones).
if [ ! -f "$repo/testenv/.env" ]; then
  printf 'SUPERUSER_PASSWORD=%s\nSERVER_PASSWORD=\n' "$(openssl rand -hex 16)" > "$repo/testenv/.env"
  echo "created testenv/.env with a random SuperUser password"
fi

# MUMCORD_E2E_STEP=test (CI shard jobs) uses the image the build job shared via `docker load`.
if [ "${MUMCORD_E2E_STEP:-}" != test ]; then
  docker build -q -t "$image" "$repo/e2e" >/dev/null
fi

tty=(); [ -t 1 ] && tty=(-t)
# Extra `docker run` flags, e.g. MUMCORD_E2E_DOCKER_ARGS="--cpus=2" to mimic a 2-core CI runner.
read -r -a extra <<< "${MUMCORD_E2E_DOCKER_ARGS:-}"
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
  -v "$cache/home:/cache/home" \
  -e HOME=/cache/home -e CARGO_HOME=/cache/cargo -e CARGO_TARGET_DIR=/cache/target \
  -e MUMCORD_SKIP_BINDINGS_EXPORT=1 \
  -e CI -e MUMCORD_E2E_SHARD -e MUMCORD_E2E_STEP \
  -e MUMCORD_PORT_BASE -e MUMCORD_PROJECT \
  "${extra[@]}" \
  "$image" "$@"
