#!/bin/sh
# Starts the test stack inside the jail: Mumble server, mumpeer relay, mumbot peer bot.
# Idempotent: running it again restarts the services and re-pins the certificates.
#
# Failures are loud on purpose: a silent half-started stack looks like a Mumcord bug from the
# client side (the client then says "this server has no relay").
set -eu

# Both come from the relay config, so this script carries no placeholders of its own (a template
# that rewrites placeholders would otherwise rewrite the sed expressions below too).
JAIL_IP=$(awk -F'"' '/^public_host/ { print $2; exit }' /usr/local/etc/mumpeer/mumpeer.toml)
[ -n "$JAIL_IP" ] || { echo "setup-stack: no public_host in the relay config" >&2; exit 1; }
RELAY_HOST=$JAIL_IP
RELAY_CONFIG=/usr/local/etc/mumpeer/mumpeer.toml
RELAY_SECRET=/usr/local/etc/mumpeer/api_secret
RELAY_LOG=/var/log/mumpeer/mumpeer.log
BOT_CONFIG=/root/bot.toml
BOT_LOG=/var/log/mumpeer/bot.log

log() { echo "setup-stack: $*"; }

fingerprint_of() {
    openssl s_client -connect "$1" </dev/null 2>/dev/null |
        openssl x509 -noout -fingerprint -sha256 |
        cut -d= -f2 | tr -d ':\r\n' | tr 'A-F' 'a-f'
}

# 1. The API secret the bot uses; never in a config file, never in a Git repository.
if [ ! -s "$RELAY_SECRET" ]; then
    umask 077
    openssl rand -hex 32 > "$RELAY_SECRET"
    log "generated $RELAY_SECRET"
fi

# 2. Mumble server. A server started by hand would keep the port, so clear those first.
pkill -f 'mumble-server -ini' 2>/dev/null || true
sleep 1
sysrc mumble_server_enable=YES >/dev/null
service mumble_server onestart >/dev/null 2>&1 || service mumble_server restart >/dev/null 2>&1
i=0
until sockstat -4 -l 2>/dev/null | grep -q ':64738'; do
    i=$((i + 1))
    [ "$i" -gt 30 ] && { echo "setup-stack: the Mumble server did not start" >&2; exit 1; }
    sleep 1
done
SERVER_PIN=$(fingerprint_of "$JAIL_IP:64738")
log "Mumble server on $JAIL_IP:64738, certificate $SERVER_PIN"

# 3. Relay.
pkill -f 'mumpeer --config' 2>/dev/null || true
: > "$RELAY_LOG"
daemon -f -o "$RELAY_LOG" /root/bin/mumpeer --config "$RELAY_CONFIG"
i=0
RELAY_FP=$(grep -o 'relay fingerprint (SHA-256): [0-9a-f]*' "$RELAY_LOG" | tail -1 | awk '{print $4}')
until [ -n "$RELAY_FP" ]; do
    i=$((i + 1))
    [ "$i" -gt 30 ] && { echo "setup-stack: the relay did not report a fingerprint, see $RELAY_LOG" >&2; exit 1; }
    sleep 1
    RELAY_FP=$(grep -o 'relay fingerprint (SHA-256): [0-9a-f]*' "$RELAY_LOG" | tail -1 | awk '{print $4}')
done
log "relay on $RELAY_HOST (443/udp and 443/tcp), certificate $RELAY_FP"

# 4. Bot config: the pins are only known now, so bot.toml is filled in here.
sed -e "s|@JAIL_IP@|$JAIL_IP|g" \
    -e "s|@SERVER_PIN@|$SERVER_PIN|g" \
    -e "s|@RELAY_FP@|$RELAY_FP|g" \
    /root/bot.toml.in > "$BOT_CONFIG"
chmod 600 "$BOT_CONFIG"

# 5. Register the bot once: Mumcord only trusts a relay announced by a registered user.
pkill -f 'mumbot peer --config' 2>/dev/null || true
if ! grep -q "^name = \"RelayBot\"" /root/.mumbot-registered 2>/dev/null; then
    log "registering RelayBot on the server"
    /root/bin/mumbot --host "$JAIL_IP" --port 64738 --name RelayBot --hold 5 register
    echo 'name = "RelayBot"' > /root/.mumbot-registered
fi

# 6. Peer bot.
: > "$BOT_LOG"
daemon -f -o "$BOT_LOG" /root/bin/mumbot peer --config "$BOT_CONFIG"
sleep 3
if ! pgrep -f 'mumbot peer' >/dev/null; then
    echo "setup-stack: the peer bot stopped, see $BOT_LOG" >&2
    exit 1
fi
log "peer bot running as RelayBot"
log "ready: point Mumcord at mumble://$JAIL_IP:64738"
