# Mumcord test servers

Local Mumble servers for manual and automated tests. Server configuration only; nothing here changes Mumble itself.
Requires: Docker (with compose), `openssl`, `sqlite3` on the host.

## Setup (once)

    cp testenv/.env.example testenv/.env   # then edit the passwords; .env is git-ignored

## Start / stop

    testenv/up.sh <v15|v14|v13|limit> [--cert A|B]
    testenv/down.sh [service]              # no argument stops everything; data is kept

| Service | Server | Host port (TCP+UDP) |
|---|---|---|
| `v15` | Mumble 1.5.735 | 64738 |
| `v14` | Mumble 1.4.230 | 64739 |
| `v13` | Mumble 1.3.x (Debian bullseye `mumble-server` package, built locally on first start) | 64740 |
| `limit` | 1.5.735 with `listenersperchannel=0` and `listenersperuser=0` (Listen refused) | 64741 |

Superuser name is `SuperUser`, password is `SUPERUSER_PASSWORD` from `.env`. `SERVER_PASSWORD` (optional) is the connect password; leave it empty for an open server.

## Channels

On the first start of each service, `up.sh` writes the channels Lobby, Raid and AFK (under Root) directly into the server's SQLite database while the server is stopped, then restarts it. No client is needed. State lives in `testenv/data/<service>/` (git-ignored). To reset a server: `down.sh <service>`, then `rm -rf testenv/data/<service>`.

## Certificates

`up.sh` generates two self-signed sets on first use into `testenv/certs/A` and `testenv/certs/B` (git-ignored, never committed). Default is A. For the "certificate changed" test:

    testenv/up.sh v15            # certificate A
    testenv/up.sh v15 --cert B   # recreates the container with certificate B, same data

Check: `openssl s_client -connect localhost:64738 </dev/null 2>/dev/null | openssl x509 -noout -fingerprint -sha256`

## Restart a container (reconnect tests)

    docker restart testenv-v15-1     # also: testenv-v14-1, testenv-v13-1, testenv-limit-1
    docker stop testenv-v15-1; docker start testenv-v15-1

Restart keeps the certificate set and data. Logs: `docker logs -f testenv-v15-1`.
