#!/bin/bash
# Usage: ./down.sh [service]   (no argument stops everything; data is kept)
set -euo pipefail
cd "$(dirname "$0")"
export PUID="$(id -u)" PGID="$(id -g)" SUPERUSER_PASSWORD="${SUPERUSER_PASSWORD:-x}"
if [ $# -ge 1 ]; then docker compose rm -sf "$1"; else docker compose down --remove-orphans; fi
