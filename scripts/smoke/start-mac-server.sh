#!/bin/bash
# Starts the Mumble server in the background and waits until it listens on port 64738 (macOS smoke
# job). Run it in the SAME workflow step as the smoke test, so the server lives as long as it.
# Mumble 1.6 parses long options with two dashes; the old "-ini"/"-fg" spelling is rejected.
#   scripts/smoke/start-mac-server.sh <mumble-server> <dir>
set -euo pipefail
server=${1:?server missing}
dir=${2:?dir missing}
mkdir -p "$dir"
printf 'database=%s\nport=64738\nautobanAttempts=0\n' "$dir/mumble-server.sqlite" > "$dir/mumble-server.ini"
"$server" --ini "$dir/mumble-server.ini" --foreground > "$dir/server.log" 2>&1 &
pid=$!
for _ in $(seq 60); do
  if nc -z 127.0.0.1 64738 2>/dev/null; then
    echo "server pid $pid listening"
    exit 0
  fi
  kill -0 "$pid" 2>/dev/null || break
  sleep 1
done
echo "server did not open port 64738" >&2
cat "$dir/server.log" >&2
exit 1
