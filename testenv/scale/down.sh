#!/bin/bash
# Stops the load-test server; data is kept in testenv/data/scale.
set -euo pipefail
cd "$(dirname "$0")"
set -a
# shellcheck disable=SC1091 # testenv/.env is a git-ignored local file, not in the repo
. ../.env
set +a
docker compose -p mumcord-scale -f docker-compose.yml down
