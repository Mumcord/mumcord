#!/bin/bash
# Usage: ./reset.sh [v15|v14|v13|limit|v15b] [--cert A|B]
# Restores a server to its fresh seeded state (Lobby/Raid/AFK, only SuperUser registered).
# Deletes the server's database, so all registrations, ACLs and channels from tests are gone.
# No service argument resets all five.
set -euo pipefail
cd "$(dirname "$0")"
. ./env.sh
if [ $# -ge 1 ] && [[ "$1" != --* ]]; then svcs=("$1"); shift; else svcs=(v15 v14 v13 limit v15b); fi
for s in "${svcs[@]}"; do
  case "$s" in v15|v14|v13|limit|v15b) ;; *) echo "usage: $0 [v15|v14|v13|limit|v15b] [--cert A|B]" >&2; exit 2;; esac
done
PUID=$(id -u)
PGID=$(id -g)
export PUID PGID SUPERUSER_PASSWORD="${SUPERUSER_PASSWORD:-x}"
for s in "${svcs[@]}"; do
  docker compose rm -sf "$s" >/dev/null
  rm -f "$DATA_ROOT/$s"/murmur.sqlite* "$DATA_ROOT/$s"/mumble-server.sqlite* "$DATA_ROOT/$s"/.seeded
  ./up.sh "$s" "$@"
done
