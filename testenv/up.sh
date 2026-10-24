#!/bin/bash
# Usage: ./up.sh <v15|v14|v13|limit> [--cert A|B]
set -euo pipefail
cd "$(dirname "$0")"
svc="${1:-}"; shift || true
case "$svc" in v15|v14|v13|limit) ;; *) echo "usage: $0 <v15|v14|v13|limit> [--cert A|B]" >&2; exit 2;; esac
CERT_SET=A
if [ "${1:-}" = "--cert" ]; then CERT_SET="${2:-}"; fi
case "$CERT_SET" in A|B) ;; *) echo "cert set must be A or B" >&2; exit 2;; esac
[ -f .env ] || { echo "missing testenv/.env - run: cp testenv/.env.example testenv/.env" >&2; exit 1; }
command -v sqlite3 >/dev/null || { echo "sqlite3 is required on the host to seed channels" >&2; exit 1; }
export CERT_SET PUID="$(id -u)" PGID="$(id -g)"
./gen-certs.sh
mkdir -p "data/$svc"

# Recreating the container applies the selected certificate set; data (users, channels) survives.
docker compose up -d --build --force-recreate "$svc"

if [ ! -f "data/$svc/.seeded" ]; then
  db="data/$svc/murmur.sqlite"; [ "$svc" = v13 ] || db="data/$svc/mumble-server.sqlite"
  for _ in $(seq 60); do
    [ -f "$db" ] && sqlite3 "$db" "select 1 from channels limit 1" >/dev/null 2>&1 && break
    sleep 1
  done
  # The server only reads channels at startup, so seed while it is stopped.
  docker compose stop "$svc" >/dev/null
  sqlite3 "$db" "insert or ignore into channels (server_id, channel_id, parent_id, name, inheritacl) values
    (1,1,0,'Lobby',1),(1,2,0,'Raid',1),(1,3,0,'AFK',1);"
  touch "data/$svc/.seeded"
  docker compose up -d "$svc"
fi
echo "$svc up with certificate set $CERT_SET"
