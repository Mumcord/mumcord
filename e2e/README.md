# Mumcord E2E tests

UI end-to-end tests that drive the real app (real engine inside Tauri) through WebDriver, against
the local test servers in `testenv/`. Decision Q4.8; acceptance criteria in spec §11.

## Run

    npm run e2e                                              # whole suite
    npm run e2e -- --spec e2e/specs/smoke/app.e2e.ts         # one spec (args go to `wdio run`)
    MUMCORD_E2E_SHARD=2 npm run e2e                          # one CI shard (see below)

### Several shards in parallel

Build once, then start one test run per shard, each with its own run id (`MUMCORD_E2E_RUN`, 1–9):

    MUMCORD_E2E_STEP=build npm run e2e
    for n in 1 2 3; do
      MUMCORD_E2E_STEP=test MUMCORD_E2E_RUN=$n MUMCORD_E2E_SHARD=$n npm run e2e > e2e-run-$n.log 2>&1 &
    done
    wait

Run n gets its own Xvfb display (`:99+n`), tauri-driver/WebKitWebDriver ports (`4444+2n`,
`4445+2n`), overlay socket, home dir (`<cache>/home-<n>`: keyring, bot certificates), test server
ports (`MUMCORD_PORT_BASE`, default `40000+1000n`) and compose project (`e2e-<n>`). Keep the port
base in the 4xxxx range: the default 64738 plus the netsim offsets overflows above ~65400.

`run.sh` locks `<cache>/run.lock` itself, so no `flock` around `npm run e2e` is needed: a build or
a run without a run id (default servers and ports) takes the lock alone; runs with a run id share
it, so a build (from another checkout, too) waits until they are done instead of replacing the
binaries under them. A run id needs `MUMCORD_E2E_STEP=test`. 2–3 parallel runs work on a 16-core
machine; more invite timing flakes in the voice and packet-loss specs.

Requires only Docker (user in the `docker` group) and `npm ci` on the host. Everything else runs
in the `mumcord-e2e` image, because Arch's webkit2gtk ships without `WebKitWebDriver`.

Runtime (16-core dev machine): cold ~5 min (image build ~3 min, first full debug build of app
and netsim ~1.5 min, smoke spec a few seconds); warm ~15 s without code changes, plus the incremental
rebuild otherwise. In CI expect roughly 3–4x that. Build caches (cargo registry, target dir, bot
certificates) live in `~/.cache/mumcord-build/e2e/` (override with `MUMCORD_E2E_CACHE`), never in `target/` or `/tmp`.

## How it works

- `e2e/run.sh` (host): builds the image from `e2e/Dockerfile`, creates `testenv/.env` with a random
  SuperUser password if missing, and starts the container.
- **Network:** the container uses `--network host`, so the app, bots and netsim reach the test servers
  on `localhost:64738–64742` exactly like on the host. netsim uses 64800 (proxy) and 64801
  (control); all of these move with `MUMCORD_PORT_BASE`.
- **Test servers:** the host's Docker socket is mounted and the repo is mounted at its host path, so
  helpers call `testenv/up.sh` / `reset.sh` / `docker restart` from inside the container.
- `e2e/container-run.sh` (inside): session D-Bus, unlocked gnome-keyring (Secret Service, which the
  app uses for the identity and server passwords), Xvfb on `:99` (`:99+n` for run id n), then builds the app
  (`tauri build --debug --no-bundle --features test-backend`), `netsim`, the test game plugin
  (`crates/positional/examples/test_plugin.rs`) and `headless_check`, fetches the test bot (the
  [mumbot](https://github.com/Mumcord/mumbot) release pinned in `scripts/mumbot.env`, via
  `scripts/fetch-mumbot.sh`), then runs `wdio run e2e/wdio.conf.ts`.
- **Audio:** no sound server. The app is built with the `test-backend` feature and the env
  `MUMCORD_AUDIO_BACKEND` picks `null` (default: silence in, discard out) or
  `file:<in.wav>:<out.wav>` (mic plays `in.wav`, speakers record to `out.wav`, 48 kHz mono 16-bit);
  `file-stereo:<in.wav>:<out.wav>` records the speakers in stereo (positional audio checks).
- **Window manager, Vulkan:** no window manager runs by default; specs that need one start
  `openbox` themselves (overlay on-top check). lavapipe (software Vulkan) lets
  `crates/overlay-layer/examples/headless_check` render through the in-game overlay layer.
- **Fresh install per spec file:** each spec file gets a new profile (`XDG_DATA_HOME`/`CONFIG`/`CACHE`
  in a temp dir) and an empty keyring. Spec files run one at a time (shared servers, keyring, ports).
- **CI shards:** `.github/workflows/e2e.yml` builds the image and the binaries once
  (`MUMCORD_E2E_STEP=build`), shares them as an artifact, and runs the shards of `e2e/shards.json`
  in parallel jobs (`MUMCORD_E2E_STEP=test MUMCORD_E2E_SHARD=<n>`, each runner with its own test
  servers). The "Linux E2E" job fails if the build or any shard failed. A push to main runs only
  the build job, to keep its caches warm for pull requests (which only restore caches).
- **Shard balance:** eight shards, balanced by the measured CI duration of each spec file
  (`e2e/spec-durations.json`). The longest spec (network resilience, ~5 min in CI) is pinned alone
  to shard 1 and sets the floor; the others are spread longest-first onto the emptiest shard.
  `scripts/e2e-shards.js` rewrites `shards.json`:

      node scripts/e2e-shards.js                                 # rebalance (e.g. after adding a spec)
      node scripts/e2e-shards.js --measure <run-id> [<run-id>…]  # re-measure from passed CI runs first
      node scripts/e2e-shards.js --shards 12                     # change the number of shards

  `--measure` reads the shard logs with the `gh` CLI (take ~10 recent passed E2E runs from
  `gh run list --workflow e2e.yml --status success`). A spec without a measured duration counts with
  the median. A run with `MUMCORD_E2E_SHARD` set fails while `shards.json` and the spec files
  disagree. When changing the number of shards, also change the job matrix in `e2e.yml` (the script
  warns).

- **Failures:** screenshot + page source of the app go to `e2e/artifacts/` (git-ignored); CI uploads
  them (instance B: same name with `-B`). axe reports land there too.

## Writing specs

Put specs in `e2e/specs/<area>/<name>.e2e.ts` (mocha `describe`/`it`, WebdriverIO `browser`/`$`/`expect`
from `@wdio/globals`). Import helpers from `e2e/helpers/index.ts`:

| Helper                                                                                     | What it does                                                                                                     |
| ------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------- |
| `testenv.up(svc, {cert})`, `testenv.reset(svc, {cert})`                                    | start / reset a server (`v15`, `v15b`, `v14`, `v13`, `limit`; cert `A`/`B`). `reset` drops clients.              |
| `testenv.stop/start/restart(svc)`, `testenv.ports`                                         | container control for reconnect tests; port map                                                                  |
| `bot([...cmd], {name, port, cert, superuser, timeout, hold})`                              | runs one `mumbot` subcommand; throws unless exit 0                                                               |
| `botInBackground([...cmd], opts)`                                                          | long-running bot (`--hold`, `script` via `.send(line)`); `.done`, `.stop()`                                      |
| `botOn(svc, cmd, opts)`, `botOnAll(svcs, cmd, opts)`, `botInBackgroundOn(svc, cmd, opts)`  | bots by service name (`v15`, `v15b` …); `botOnAll` connects to several servers at once                           |
| `netsim.start(svc)`, `netsim.ctl(...args)`, `netsim.stop()`                                | proxy `127.0.0.1:64800` → server; e.g. `ctl("loss","both","10")`, `ctl("block-udp","on")`                        |
| `netsim.startProxy(svc, {listen, control})`                                                | extra proxy (own `ctl`, `stop`, `listenPort`), e.g. a bot behind its own proxy: `bot(cmd, {port: p.listenPort})` |
| `testenv.address(svc)`, `testenv.project`                                                  | `localhost:<port>` for the add-server form; compose project (`MUMCORD_PROJECT`)                                  |
| `app.restart({freshProfile, audio, env, args})`                                            | quit and restart app A (keeps the profile unless `freshProfile`; `env`/`args`: extra app env / CLI arguments)    |
| `app.profile(id)`                                                                          | profile root of instance `"a"` (default) or `"b"` (check files in `data/app.mumcord.client/` etc.)               |
| `apps.startB(opts)`, `apps.restartB(opts)`, `apps.stopB()`                                 | second client B (see "Two clients"); `startB` uses a fresh profile unless `freshProfile: false`                  |
| `apps.onB(async () => …)`, `apps.browserB()`                                               | run helpers against B (globals `browser`/`$`/`$$` point at B inside); B's raw WebDriver session                  |
| `apps.pluginEnv({registryKey, registryUrl, fakeAppVersion, forceDailyCheck, fastBackoff})` | plugin test env for the `env` option (see "Plugin test environment")                                             |
| `app.invoke(command, args)`                                                                | calls a Tauri command of the app and returns its result (state the UI does not show)                             |
| `axe.scan(label, options)`, `axe.summarize(violations)`                                    | axe-core WCAG 2.2 AA scan of the current screen; returns violations                                              |
| `bin`, `repoRoot`, `artifactsDir`                                                          | paths to binaries (incl. the test game plugin and `headless_check`) and folders                                  |

The test servers are not started automatically: call `testenv.up("v15")` (or `reset`) in a
`before` hook of specs that need them. Use `bot(["expect", "user", "Tester"])`-style checks to
verify what other users see.

## Two clients

Some criteria need two Mumcord users (plugin messaging, blocking). Every spec has instance **A**
(the normal wdio session, `browser`/`$`). `apps.startB()` starts instance **B** next to it:

```ts
await completeWizard("Alice");
await addAndConnect();
await apps.startB();
await apps.onB(async () => {
  await completeWizard("Bob");
  await addAndConnect();
});
await personRow("Bob").waitForDisplayed();
```

- **Isolation:** B has its own profile dir, its own tauri-driver/WebKitWebDriver ports
  (`4544+2n`/`4545+2n` for run id n), its own overlay socket name, and its own session D-Bus with
  its own unlocked gnome-keyring (keyring file inside B's profile). The private bus is what lets a
  second app start at all: the app's single-instance lock is a D-Bus name, and on a shared bus B
  would hand over to A and exit (and share A's identity).
- **Processes:** everything an instance starts carries `MUMCORD_E2E_INSTANCE=a|b` in its
  environment, so `app.restart()` never kills B and `apps.restartB()` never kills A. B is stopped
  after each spec file and at the start of the next one.
- **`onB`:** points the `@wdio/globals` exports at B for the duration of the callback, so every
  existing UI helper works for B. Keep A and B steps sequential: never mix an `onB` call and A
  commands in one `Promise.all`. Elements fetched inside `onB` stay bound to B.
- **Cost:** two WebKit apps need about twice the memory and CPU; start B only in specs that need it.

## Plugin test environment

Pass `env: apps.pluginEnv({...})` to `app.restart`, `apps.startB` or `apps.restartB`, and
`args: ["--no-plugins"]` to start without plugins. The variables are read by debug builds only;
release builds ignore them.

| Variable                                | `pluginEnv` field | Purpose                                                            |
| --------------------------------------- | ----------------- | ------------------------------------------------------------------ |
| `MUMCORD_PLUGIN_REGISTRY_KEY`           | `registryKey`     | public minisign key of the run's fixture registry (FR-6.53)        |
| `MUMCORD_PLUGIN_REGISTRY_URL`           | `registryUrl`     | base URL of the fixture registry (`mumcord-plugin serve-registry`) |
| `MUMCORD_PLUGIN_TEST_APP_VERSION`       | `fakeAppVersion`  | fake app version, triggers the per-version re-hash (AC-6.21)       |
| `MUMCORD_PLUGIN_TEST_FORCE_DAILY_CHECK` | `forceDailyCheck` | `1`: run the daily revocation/update check at start (AC-6.20)      |
| `MUMCORD_PLUGIN_TEST_FAST_BACKOFF`      | `fastBackoff`     | `1`: shorten the crash back-off timers (AC-6.8)                    |

The signing key pair is generated per run with `mumcord-plugin keygen` into the run's cache dir and
is never committed.

`registry.start()` runs `scripts/e2e-plugin-registry.sh`, which builds the CLI, the safety fixtures
and the official plugins. The image has no wasm32 target, so CI builds them on the runner with
`scripts/e2e-plugin-registry.sh --build-only <dir>` and sets `MUMCORD_E2E_PLUGIN_PACKAGES=<dir>`:
`e2e/run.sh` mounts the folder into the container and the script takes the CLI and packages from
it. The same works locally.
