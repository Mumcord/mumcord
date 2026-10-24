#!/bin/bash
# Usage: ./reset.sh [v15|v14|v13|limit] [--cert A|B]
# Restores a server to its fresh seeded state (Lobby/Raid/AFK, only SuperUser registered).
# Deletes the server's database, so all registrations, ACLs and channels from tests are gone.
# No service argument resets all four.
set -euo pipefail
cd "$(dirname "$0")"
if [ $# -ge 1 ] && [[ "$1" != --* ]]; then svcs=("$1"); shift; else svcs=(v15 v14 v13 limit); fi
for s in "${svcs[@]}"; do
  case "$s" in v15|v14|v13|limit) ;; *) echo "usage: $0 [v15|v14|v13|limit] [--cert A|B]" >&2; exit 2;; esac
done
export PUID="$(id -u)" PGID="$(id -g)" SUPERUSER_PASSWORD="${SUPERUSER_PASSWORD:-x}"
for s in "${svcs[@]}"; do
  docker compose rm -sf "$s" >/dev/null
  rm -f data/"$s"/murmur.sqlite* data/"$s"/mumble-server.sqlite* data/"$s"/.seeded
  ./up.sh "$s" "$@"
done
