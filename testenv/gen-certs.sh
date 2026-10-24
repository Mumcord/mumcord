#!/bin/bash
# Generates self-signed certificate sets A and B (once) into testenv/certs/.
set -euo pipefail
cd "$(dirname "$0")"
for set in A B; do
  dir="certs/$set"
  [ -f "$dir/cert.pem" ] && continue
  mkdir -p "$dir"
  openssl req -x509 -newkey rsa:2048 -nodes -days 3650 -subj "/CN=Mumcord Test Server $set" \
    -keyout "$dir/key.pem" -out "$dir/cert.pem" 2>/dev/null
  echo "generated certificate set $set"
done
