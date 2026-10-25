# Mumcord E2E tests

UI end-to-end tests that drive the real app (real engine inside Tauri) through WebDriver, against
the local test servers in `testenv/`. Decision Q4.8; acceptance criteria in spec §11.

## Run

    npm run e2e                                              # whole suite
    npm run e2e -- --spec e2e/specs/smoke/app.e2e.ts         # one spec (args go to `wdio run`)
    MUMCORD_E2E_SHARD=2 npm run e2e                          # one CI shard (see below)

Requires only Docker (user in the `docker` group) and `npm ci` on the host. Everything else runs
in the `mumcord-e2e` image, because Arch's webkit2gtk ships without `WebKitWebDriver`.

Runtime (16-core dev machine): cold ~5 min (image build ~3 min, first full debug build of app, bot
and netsim ~1.5 min, smoke spec a few seconds); warm ~15 s without code changes, plus the incremental
rebuild otherwise. In CI expect roughly 3–4x that. Build caches (cargo registry, target dir, bot
certificates) live in `~/.cache/mumcord-build/e2e/` (override with `MUMCORD_E2E_CACHE`), never in `target/` or `/tmp`.

## How it works

- `e2e/run.sh` (host): builds the image from `e2e/Dockerfile`, creates `testenv/.env` with a random
  SuperUser password if missing, and starts the container.
- **Network:** the container uses `--network host`, so the app, bots and netsim reach the test servers
  on `localhost:64738–64741` exactly like on the host. netsim always uses 64800 (proxy) and 64801
  (control).
- **Test servers:** the host's Docker socket is mounted and the repo is mounted at its host path, so
  helpers call `testenv/up.sh` / `reset.sh` / `docker restart` from inside the container.
- `e2e/container-run.sh` (inside): session D-Bus, unlocked gnome-keyring (Secret Service, which the
  app uses for the identity and server passwords), Xvfb on `:99`, then builds the app
  (`tauri build --debug --no-bundle --features test-backend`), `mumcord-bot` and `netsim`, then runs
  `wdio run e2e/wdio.conf.ts`.
- **Audio:** no sound server. The app is built with the `test-backend` feature and the env
  `MUMCORD_AUDIO_BACKEND` picks `null` (default: silence in, discard out) or
  `file:<in.wav>:<out.wav>` (mic plays `in.wav`, speakers record to `out.wav`, 48 kHz mono 16-bit).
- **Fresh install per spec file:** each spec file gets a new profile (`XDG_DATA_HOME`/`CONFIG`/`CACHE`
  in a temp dir) and an empty keyring. Spec files run one at a time (shared servers, keyring, ports).
- **CI shards:** `.github/workflows/e2e.yml` builds the image and the binaries once
  (`MUMCORD_E2E_STEP=build`), shares them as an artifact, and runs the shards of `e2e/shards.json`
  in parallel jobs (`MUMCORD_E2E_STEP=test MUMCORD_E2E_SHARD=<n>`, each runner with its own test
  servers). The "Linux E2E" job fails if the build or any shard failed. The shards are balanced by
  measured spec durations; the longest spec (network resilience, ~5 min in CI) sits alone in shard 1
  and sets the floor. A new spec file must be added to a shard (the shortest one); a run with
  `MUMCORD_E2E_SHARD` set fails while `shards.json` and the spec files disagree. When changing
  the number of shards, also change the job matrix.
- **Failures:** screenshot + page source of the app go to `e2e/artifacts/` (git-ignored); CI uploads
  them. axe reports land there too.

## Writing specs

Put specs in `e2e/specs/<area>/<name>.e2e.ts` (mocha `describe`/`it`, WebdriverIO `browser`/`$`/`expect`
from `@wdio/globals`). Import helpers from `e2e/helpers/index.ts`:

| Helper                                                        | What it does                                                                                |
| ------------------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| `testenv.up(svc, {cert})`, `testenv.reset(svc, {cert})`       | start / reset a server (`v15`, `v14`, `v13`, `limit`; cert `A`/`B`). `reset` drops clients. |
| `testenv.stop/start/restart(svc)`, `testenv.ports`            | container control for reconnect tests; port map                                             |
| `bot([...cmd], {name, port, cert, superuser, timeout, hold})` | runs one `mumcord-bot` subcommand; throws unless exit 0                                     |
| `botInBackground([...cmd], opts)`                             | long-running bot (`--hold`, `script` via `.send(line)`); `.done`, `.stop()`                 |
| `netsim.start(svc)`, `netsim.ctl(...args)`, `netsim.stop()`   | proxy `127.0.0.1:64800` → server; e.g. `ctl("loss","both","10")`, `ctl("block-udp","on")`   |
| `app.restart({freshProfile, audio})`                          | quit and restart the app (keeps the profile unless `freshProfile`)                          |
| `app.profile()`                                               | current profile root (check files in `data/app.mumcord.client/` etc.)                       |
| `axe.scan(label, options)`, `axe.summarize(violations)`       | axe-core WCAG 2.2 AA scan of the current screen; returns violations                         |
| `bin`, `repoRoot`, `artifactsDir`                             | paths to binaries and folders                                                               |

The test servers are not started automatically: call `testenv.up("v15")` (or `reset`) in a
`before` hook of specs that need them. Use `bot(["expect", "user", "Tester"])`-style checks to
verify what other users see.
