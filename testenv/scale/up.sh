#!/bin/bash
# Starts the load-test server on port 64850 and, on first start, seeds ~300 nested channels.
set -euo pipefail
cd "$(dirname "$0")"
[ -f ../.env ] || { echo "missing testenv/.env" >&2; exit 1; }
set -a
# shellcheck disable=SC1091 # testenv/.env is a git-ignored local file, not in the repo
. ../.env
set +a
PUID=$(id -u)
PGID=$(id -g)
export PUID PGID
../gen-certs.sh
mkdir -p ../data/scale
compose=(docker compose -p mumcord-scale -f docker-compose.yml)
"${compose[@]}" up -d
db=../data/scale/mumble-server.sqlite
if [ ! -f ../data/scale/.seeded ]; then
  for _ in $(seq 120); do
    [ -f "$db" ] && [ "$(sqlite3 "$db" "select count(*) from channels where channel_id = 0" 2>/dev/null)" = 1 ] && break
    sleep 1
  done
  "${compose[@]}" stop >/dev/null
  python3 seed_channels.py | sqlite3 "$db"
  touch ../data/scale/.seeded
  "${compose[@]}" up -d
fi
echo "scale server up on port 64850 ($(sqlite3 "$db" 'select count(*) from channels') channels)"
