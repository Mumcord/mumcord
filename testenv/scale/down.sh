#!/bin/bash
# Stops the load-test server; data is kept in testenv/data/scale.
set -euo pipefail
cd "$(dirname "$0")"
set -a; . ../.env; set +a
docker compose -p mumcord-scale -f docker-compose.yml down
