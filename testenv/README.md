# Mumcord test servers

Local Mumble servers for manual and automated tests. Server configuration only; nothing here changes Mumble itself.
Requires: Docker (with compose), `openssl`, `sqlite3` on the host.

## Setup (once)

    cp testenv/.env.example testenv/.env   # then edit the passwords; .env is git-ignored

## Start / stop

    testenv/up.sh <v15|v15b|v14|v13|limit> [--cert A|B]
    testenv/down.sh [service]              # no argument stops everything; data is kept

| Service | Server | Host port (TCP+UDP) |
|---|---|---|
| `v15` | Mumble 1.5.735 | 64738 |
| `v14` | Mumble 1.4.230 | 64739 |
| `v13` | Mumble 1.3.x (Debian bullseye `mumble-server` package, built locally on first start) | 64740 |
| `v15b` | second 1.5.735 server (own data, name "Mumcord Test Server B") for multi-server tests | 64742 |
| `limit` | 1.5.735 with `listenersperchannel=0` and `listenersperuser=0` (Listen refused) | 64741 |

Superuser name is `SuperUser`, password is `SUPERUSER_PASSWORD` from `.env`. `SERVER_PASSWORD` (optional) is the connect password; leave it empty for an open server.

### One password for the shared servers

The SuperUser password is applied on every container start (`up.sh` recreates the container). The default servers (project `testenv`, ports 64738-64742) are shared by all git worktrees, so every worktree needs the same `testenv/.env` as the main checkout. `up.sh` refuses to run when it differs, and `e2e/run.sh` copies the main checkout's `.env` into a new worktree. Live tests read the password from their own checkout's `.env`, so a mismatch shows up as a failed SuperUser login.

If SuperUser logins fail against a running shared server, apply the main checkout's password again:

    cp <main-checkout>/testenv/.env testenv/.env
    testenv/up.sh v15        # recreates the container with that password; data is kept

For a private server with its own password use a parallel instance (below).

## Parallel instances

Two optional env vars (read by `up.sh`, `down.sh`, `reset.sh` and the E2E helpers) let several people or agents run their own servers side by side:

    MUMCORD_PORT_BASE=64900 MUMCORD_PROJECT=qa1 testenv/up.sh v15

- `MUMCORD_PORT_BASE` (default 64738): port of `v15`; `v14`, `v13`, `limit`, `v15b` follow at +1 to +4. E2E netsim ports are +62/+63 (64800/64801 by default). Give each instance a base at least 100 apart.
- `MUMCORD_PROJECT` (default `testenv`): compose project name. Containers are `<project>-<service>-1`; a non-default project keeps its data in `testenv/data/_<project>/`.
- Stop with the same variables set: `MUMCORD_PROJECT=qa1 testenv/down.sh`.
- Defaults reproduce the original ports and names. Certificates are shared.

## Channels

On the first start of each service, `up.sh` writes the channels Lobby, Raid and AFK (under Root) directly into the server's SQLite database while the server is stopped, then restarts it. No client is needed. State lives in `testenv/data/<service>/` (git-ignored). Autoban is disabled on all servers (`autobanAttempts=0`), so tests can reconnect as often as they like.

## Reset to the fresh state

    testenv/reset.sh [v15|v15b|v14|v13|limit] [--cert A|B]   # no service resets all five

Live tests leave self-registered users behind. `reset.sh` stops the service, deletes its database and restarts it via `up.sh`: only SuperUser is registered, channels are Lobby, Raid and AFK. It interrupts connected clients, so do not run it during another test. Cert set defaults to A; pass `--cert B` to keep B.

## Certificates

`up.sh` generates two self-signed sets on first use into `testenv/certs/A` and `testenv/certs/B` (git-ignored, never committed). Default is A. For the "certificate changed" test:

    testenv/up.sh v15            # certificate A
    testenv/up.sh v15 --cert B   # recreates the container with certificate B, same data

Check: `openssl s_client -connect localhost:64738 </dev/null 2>/dev/null | openssl x509 -noout -fingerprint -sha256`

## Restart a container (reconnect tests)

    docker restart testenv-v15-1     # also: testenv-v15b-1, testenv-v14-1, testenv-v13-1, testenv-limit-1
    docker stop testenv-v15-1; docker start testenv-v15-1

Restart keeps the certificate set and data. Logs: `docker logs -f testenv-v15-1`.

## Local mumbot (music and plugins)

An optional [mumbot](https://github.com/Mumcord/mumbot) next to `v15` for the plugin tests: the `music` module (HTTP API on `127.0.0.1:8081`) and the `plugins` module announcing `dice`. It is an ordinary client on the server; nothing on the server changes. Needs Docker, the `v15` server running and `MUMBOT_API_KEY` in `.env` (placeholder in `.env.example`; set your own).

    testenv/up.sh v15
    testenv/mumbot/up.sh      # renders the config, registers the bot once, starts the container
    testenv/mumbot/down.sh    # stops it; certificate and registration stay

- The bot is `MumbotLocal`, registered on `v15`, pinned to the test certificate (set A). It sits in `Lobby` and plays into `Lobby`, `Raid` and `AFK`.
- Play a test track: `curl -X POST http://127.0.0.1:8081/play -H "X-API-Key: $MUMBOT_API_KEY" -d '{"file": "tone-a.mp3"}'`. Test MP3s: `mumbot/media/tone-a.mp3`, `tone-b.mp3` (6 s sine tones).
- URL sources are allowed from `host.docker.internal` (a throwaway HTTP server on the host).
- Version: the image tag in `docker-compose.yml` and `scripts/mumbot.env` move together. The bot needs the `v15` certificate set `A` (the default); after `up.sh v15 --cert B` rerun `mumbot/up.sh` with `CERT_SET=B`.
- Port: `MUMBOT_PORT` (default 8081).
- `up.sh` fetches the tester build of mumbot only to register the bot (`MUMBOT_BIN` overrides).
