#!/bin/bash
# Usage: ./up.sh <v15|v14|v13|limit|v15b> [--cert A|B]
set -euo pipefail
cd "$(dirname "$0")"
. ./env.sh
svc="${1:-}"; shift || true
case "$svc" in v15|v14|v13|limit|v15b) ;; *) echo "usage: $0 <v15|v14|v13|limit|v15b> [--cert A|B]" >&2; exit 2;; esac
CERT_SET=A
if [ "${1:-}" = "--cert" ]; then CERT_SET="${2:-}"; fi
case "$CERT_SET" in A|B) ;; *) echo "cert set must be A or B" >&2; exit 2;; esac
[ -f .env ] || { echo "missing testenv/.env - run: cp testenv/.env.example testenv/.env" >&2; exit 1; }
# The shared default servers (project "testenv") are recreated by whichever checkout runs up.sh, and
# the SuperUser password is applied on every start. A git worktree with its own, different .env would
# silently change the password the other checkouts' live tests expect, so refuse that.
if [ "$MUMCORD_PROJECT" = testenv ]; then
  primary="$(dirname "$(git rev-parse --path-format=absolute --git-common-dir 2>/dev/null)")/testenv/.env"
  if [ -f "$primary" ] && ! cmp -s .env "$primary"; then
    echo "testenv/.env differs from the main checkout's ($primary)." >&2
    echo "The shared servers must use one password: cp '$primary' testenv/.env" >&2
    echo "(or use a private instance: MUMCORD_PROJECT=<name> MUMCORD_PORT_BASE=<port>)" >&2
    exit 1
  fi
fi
command -v sqlite3 >/dev/null || { echo "sqlite3 is required on the host to seed channels" >&2; exit 1; }
PUID=$(id -u)
PGID=$(id -g)
export CERT_SET PUID PGID
./gen-certs.sh
mkdir -p "$DATA_ROOT/$svc"

# Recreating the container applies the selected certificate set; data (users, channels) survives.
docker compose up -d --build --force-recreate "$svc"

if [ ! -f "$DATA_ROOT/$svc/.seeded" ]; then
  db="$DATA_ROOT/$svc/murmur.sqlite"; [ "$svc" = v13 ] || db="$DATA_ROOT/$svc/mumble-server.sqlite"
  # The schema appears early but is rolled back if the container is stopped mid-initialisation
  # (slow runner). The root channel row is written once the server has booted its virtual server.
  ready=0
  for _ in $(seq 120); do
    if [ -f "$db" ] && [ "$(sqlite3 "$db" "select count(*) from channels where channel_id = 0" 2>/dev/null)" = 1 ]; then
      ready=1; break
    fi
    sleep 1
  done
  [ "$ready" = 1 ] || { echo "$svc did not initialise its database within 120s" >&2; docker compose logs --tail 20 "$svc" >&2; exit 1; }
  # The server only reads channels at startup, so seed while it is stopped.
  docker compose stop "$svc" >/dev/null
  sqlite3 "$db" "insert or ignore into channels (server_id, channel_id, parent_id, name, inheritacl) values
    (1,1,0,'Lobby',1),(1,2,0,'Raid',1),(1,3,0,'AFK',1);"
  touch "$DATA_ROOT/$svc/.seeded"
  docker compose up -d "$svc"
fi
echo "$svc up with certificate set $CERT_SET"
