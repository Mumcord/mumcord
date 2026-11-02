#!/bin/bash
# Usage: testenv/mumbot/down.sh   (stops the local mumbot; its certificate and registration stay)
set -euo pipefail
cd "$(dirname "$0")/.."
# shellcheck source=testenv/env.sh
. ./env.sh
PUID=$(id -u) PGID=$(id -g) SUPERUSER_PASSWORD="${SUPERUSER_PASSWORD:-x}" docker compose --profile mumbot rm -sf mumbot
