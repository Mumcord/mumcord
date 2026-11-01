#!/bin/bash
# Usage: testenv/mumbot/up.sh
# Starts the local mumbot (music + plugins modules) on the v15 test server. Needs v15 running
# (testenv/up.sh v15) and MUMBOT_API_KEY in testenv/.env. Music API: http://127.0.0.1:${MUMBOT_PORT:-8081}.
set -euo pipefail
cd "$(dirname "$0")/.."
. ./env.sh
[ -f .env ] || { echo "missing testenv/.env - run: cp testenv/.env.example testenv/.env" >&2; exit 1; }
set -a; . ./.env; set +a
[ -n "${MUMBOT_API_KEY:-}" ] || { echo "set MUMBOT_API_KEY in testenv/.env (any random string, e.g. openssl rand -hex 16)" >&2; exit 1; }
[ -f certs/A/cert.pem ] || ./gen-certs.sh
cert_set=${CERT_SET:-A}
sha=$(openssl x509 -in "certs/$cert_set/cert.pem" -noout -fingerprint -sha256 | cut -d= -f2 | tr -d : | tr 'A-F' 'a-f')

dir="$DATA_ROOT/mumbot"
mkdir -p "$dir/data"
sed -e "s/@CERT_SHA256@/$sha/" -e "s/@API_KEY@/$MUMBOT_API_KEY/" mumbot/bot.toml.template > "$dir/bot.toml"

# The plugins module only runs for a registered bot user, so register once with the same
# certificate the container will use. The tester build of mumbot (scripts/fetch-mumbot.sh) does it.
bot="${MUMBOT_BIN:-}"
if [ -z "$bot" ]; then
  fetch_dir="${MUMBOT_FETCH_DIR:-$HOME/.cache/mumbot/bin}"
  case "$(uname -s)-$(uname -m)" in
    Linux-x86_64) target=linux-x86_64 ;; Linux-aarch64) target=linux-aarch64 ;;
    Darwin-*) target=macos-universal ;; *) echo "set MUMBOT_BIN to a mumbot tester binary" >&2; exit 1 ;;
  esac
  ../scripts/fetch-mumbot.sh "$target" "$fetch_dir" >/dev/null
  bot="$fetch_dir/mumbot"
fi
if [ ! -f "$dir/.registered" ]; then
  MUMBOT_DATA="$PWD/$dir/data" "$bot" --port "$PORT_V15" --name MumbotLocal --cert mumbotlocal register
  touch "$dir/.registered"
fi

export PUID=$(id -u) PGID=$(id -g) SUPERUSER_PASSWORD="${SUPERUSER_PASSWORD:-x}"
docker compose --profile mumbot up -d --force-recreate mumbot
echo "mumbot up: music API http://127.0.0.1:${MUMBOT_PORT:-8081} (key MUMBOT_API_KEY from testenv/.env)"
