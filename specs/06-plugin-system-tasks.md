# Epic 3.1 – Plugin system · Task list

Maintained by the lead only. Status: open / in progress / done / blocked. Task details: see the breakdown below the table.

| ID | Title | Status | Owner |
|---|---|---|---|
| B1 | Bring in PR #297 and all epic dependencies | done | b1-deps |
| B3 | Store: plugin tables and settings keys | done | b3-store |
| B5 | Protocol: low-priority lane for plugin messages | done | b5-lane |
| B6 | E2E: two Mumcord clients in one run | in progress | b6-e2e2 |
| B7 | ⚠ OUTWARD: create the `Mumcord/plugins` repository skeleton | done | b7-pluginsrepo |
| B8 | ⚠ OUTWARD (repo `Mumcord/mumbot`): module `plugins` (mumbot#20) | done (mumbot PR #33, not merged) | b8-mumbot |
| B20 | Deep link `mumcord://plugins/<id>` | done | b20-deeplink |
| B2 | WIT package `mumcord:plugin@1.0.0` | done | b2-wit |
| B4 | Manifest, permission catalogue and package rules to spec | done | b4-manifest |
| B10 | Runtime: thread per plugin, limits, back-off, cache, diagnostics | done | b10-runtime |
| B11a | Host API: storage and secrets | done | b11a-storage |
| B11b | Host API: timers, clock, notify, theme, random, log, permission gate, peer key | done | b11b-hostapi |
| B12 | Messaging logic (pure) | done | b12-msg |
| B13 | Engine bridge for plugins | done | b13-bridge |
| B14 | Signing and verification | done | b14-verify |
| B15 | Registry and repository client | done | b15-registry |
| B16a | Safety fixture plugins | done | b16a-fixtures |
| B17a | Plugin service: lifecycle | in progress | b17a-service |
| B17b | Updates and kill switch | open | |
| B17c | Chat integration logic | done | b17c-chat |
| B18 | App glue (src-tauri) | open | |
| B19 | `net.http` executor (official only) | done | b19-http |
| B21 | UI shell: Settings → Plugins, Developer mode, palette, store | open | |
| B22 | Permission list component and texts | done | b22-permlist |
| B25a | SDK crate `mumcord-plugin-sdk` | done | b25a-sdk |
| B25b | CLI `mumcord-plugin` (author + registry commands) | done | b25b-cli |
| B25c | CLI `test` and `preview` | in progress | b25c-test |
| B16b | Fixture registry setup for E2E | open | |
| B9 | Local mumbot instance (music + plugins) | done | b9-mumbot |
| B26 | UI tree renderer | open | |
| B27 | Plugins strip and panel popover | open | |
| B28 | Member badges, profile-card sections, chat decorations | open | |
| B29 | Chat UI: cards, slash commands, via label, superseded lines | open | |
| B30 | User-menu entry and plugin dialog | open | |
| B31 | Sandboxed web view | open | |
| B32 | Notification centre: category Plugins | open | |
| B34 | Server-suggested plugins logic | open | |
| B23 | Installed tab and plugin details | open | |
| B24 | Browse tab, install prompt, deep-link target | open | |
| B33a | Updates tab | open | |
| B33b | Repositories tab | open | |
| B33c | Developer tab | open | |
| B37 | ⚠ OUTWARD: registry CI and publish workflows | open | |
| B38 | Template and author docs | done (Mumcord/plugins PR #4, not merged) | b38-template |
| B39 | Poll plugin | done (Mumcord/plugins PR #2, not merged) | b39-poll |
| B40 | Dice roller plugin | done (Mumcord/plugins PR #5, not merged) | b40-dice |
| B41a | Echolings: pet core | done (Mumcord/plugins PR #3, not merged) | b41a-pets |
| B41b | Echolings: social + Echo Echo | in progress | b41b-social |
| B41c | Echolings: Snack Race (web view) | open | |
| B42 | Mumbot Music plugin | in progress | b42-music |
| B43 | CI in Mumcord: plugin tests and E2E | open | |
| B44 | PRIVACY.md plugins section | done | b44-privacy |
| K1 | Official plugin registry signing key (generated locally, stored as secret, never committed) | done (public keys: ~/.config/mumcord-signing/plugins/{active,backup}.pub; active id 1CB49831E26E61AD, backup id 7989D354F5AE80C5) | k1-signkey |
| B20b | Deep link: buffer a cold-start `plugins://open-details` until the UI takes it (pending-take command) + Windows runtime registration of `mumcord` in commands/app.rs | done | b20b-deeplink |
| F1 | Fix: ui tsc error in ui/src/api/mock.ts (missing plugin settings fields from B3) + prettier on crates/plugins/schema/manifest.schema.json | done | f1-uifix |
| F2 | Fix: safety fixtures pass `prettier --check .` (format them) and builder `common_manifest()` uses only B4 fields (authors/categories) | done | f2-fixtures |
| F3 | chat.rs GestureGate: a gesture from a typed command (source `command`) allows its one message for up to 8 s (clicks stay 2 s); needed for verified dice rolls (FR-6.44 addendum) | done | f3-gesture |
| F4 | Fixes: docs/plugin-registry-format.md example says `chat.commands` (catalogue id is `commands`); after B25c: add the test-harness section to Mumcord/plugins docs/testing.md | open | |
| M1 | Epic merge follow-up: switch the SDK git dep from branch epic/plugins to main (tools/mumcord-plugin new.rs template, Mumcord/plugins template/Cargo.toml and all official plugins) | open | |
| B25c2 | Runtime test: build the SDK example (wasm32-unknown-unknown + wit-component) and load/enable it in the real runtime (after B11a/B11b serve storage, ui, peers) | open | |

---

# Epic 3.1 – Plugin system · Task breakdown (A1)

Spec: `specs/06-plugin-system-specification.md` · Worktree: `/home/tvorwachs/code/mumble-plugins` · Branch: `epic/plugins`

## 0. Findings that shape the breakdown

- **PR #297 (`origin/feat/plugins-runtime-manifest`)** is three clean commits on top of old main:
  `b96df0a8` (manifest, package, version, i18n, schema, WIT `types`/`log`/`world`), `bee0bfee` (wasmtime runtime, `host.rs`, `health.rs` back-off, WAT fixture + tests), `bf975b6e` (third-party notices). They cherry-pick; only `Cargo.lock`/notices will conflict (regenerate).
  Gaps vs. spec: the manifest uses `author` (not `authors`), `entry`, `icons` map (spec: `icon`), no `commands`/`net`; the permission list is the old 9-id list (spec: 19 ids with risk and official-only); WIT has only `types` + `log`; no per-plugin thread, event queue, cache, store, permissions, messaging.
- **wasmtime 50 needs Rust ≥ 1.97**; local rustc is 1.99, CI uses `stable`. Because `src-tauri` will depend on `crates/plugins`, the app effectively needs 1.97 (workspace stays 1.90 for published crates audio/p2p/protocol/codec/store).
- **Workspace deps already present**: `reqwest` (rustls), `keyring`, `ammonia`, `sha2`, `rand`, `serde_json`, `clap`, `tempfile`. Missing: `wasmtime`, `zip`, `semver`, `minisign-verify`, `hmac`, `wat`, `wit-component`, `wit-parser`, `wit-bindgen` (SDK), a YAML parser (CLI `test`), `toml` (registry source), a minisign *signing* crate for the CLI (`minisign`).
- **Engine**: hello features list `crates/engine/src/mumcord.rs:28` (5 used of 16). All plugin data is dispatched in `crates/engine/src/session/mod.rs:2667–2693`. Registered-sender pinning of spec 04 lives in `crates/engine/src/peer.rs` + `crates/store/src/relay_pin.rs`; inspector "sender not registered" in `crates/engine/src/inspector.rs:195`.
- **Protocol pacing**: `crates/protocol/src/ratelimit.rs` has one leaky bucket for all plugin data (12 tokens / 300 ms). There is **no priority**: FR-6.36 "core features first, plugin messages wait ≤ 10 s then drop" needs a new low-priority lane there.
- **Store**: migrations are a single array `MIGRATIONS` in `crates/store/src/db.rs` (currently 20 entries); settings are one serde record `crates/store/src/settings.rs:522`, mirrored in `src-tauri/src/commands/settings.rs:466`. Keychain helper `crates/store/src/secrets.rs` (service for identities; plugins need service `Mumcord plugins`).
- **Tauri**: commands in `src-tauri/src/commands/*.rs`, bindings generated by specta into the single file `ui/src/bindings/commands.ts` (every command task touches it → regenerate after rebase, never hand-merge). Deep links: `src-tauri/src/deeplink.rs`, scheme list in `src-tauri/tauri.conf.json:64` has **only `mumble`** → `mumcord://` must be registered (all 3 OSes). No Developer mode exists yet.
- **UI**: Settings sections `ui/src/features/settings/sectionIds.ts` + `SettingsPages.tsx`; notification centre `ui/src/features/notifications/centreSources.ts`; person menu `ui/src/features/members/PersonMenu.tsx`; profile card `ui/src/features/members/useProfileCard.tsx`; tree rows `ui/src/features/channels/treeRows.tsx`; composer `ui/src/features/chat/Composer.tsx`.
- **Locales**: namespaces are glob-loaded (`ui/src/i18n/resources.ts`), and `ui/src/i18n/i18n.test.ts` enforces that **all 24 locales have every namespace and key**. So each UI task owns its own namespace file in all 24 locales (no shared locale file, no separate translation task).
- **E2E**: drives **one** app via WebdriverIO; other "users" are the pinned `mumbot` tester CLI (`e2e/helpers/bot.ts`, `scripts/fetch-mumbot.sh`). Earlier epics left two-Mumcord-client criteria manual. Spec §10 requires "E2E with two clients" → a harness task is needed (B6). Rust live tests (`crates/engine/tests/live_*.rs`) already run several engine sessions against the local server → good place for messaging rules.
- **mumbot**: `Mumcord/mumbot` already has the music module with HTTP API (`/play`, `/queue`, `/now-playing`, `/skip`, `/stop`, `/volume`, `/health`, `X-API-Key`, upload max 20 MB). Only module `plugins` (mumbot#20, open) is missing. `GET /health` needs no key → "Test connection" must use `GET /queue` to detect a wrong key.
- **`Mumcord/plugins` does not exist yet.**

## 1. Global contracts (every agent must know)

- **Crate layout** `crates/plugins/src/`: `manifest.rs`, `package.rs`, `version.rs`, `i18n.rs`, `permissions.rs` (catalogue), `runtime.rs`, `host.rs`, `health.rs`, `cache.rs`, `diag.rs`, `api/<interface>.rs` (one file per WIT interface), `messaging/`, `verify/`, `registry/`, `service/` (lifecycle facade), `chat.rs`, `suggestions.rs`. `crates/plugins/src/lib.rs` gets one `mod` line per task (trivial rebase conflicts are OK; resolve by keeping both lines).
- **Layering**: `crates/plugins` depends on `mumcord-store` (kv, grants, repos) but **not** on the engine. The engine only does transport and events (`crates/engine/src/plugins.rs`): raw `mumcord.p.*` / `mumcord.plugins` / `mumcord.srvplugins` bytes in and out, presence/talking/self/chat-activity/mention events, blocked users already filtered. `src-tauri/src/plugins/` glues engine ↔ plugin service ↔ UI. The UI never talks to plugins or the network.
- **WIT** `mumcord:plugin@1.0.0` in `crates/plugins/wit/` (B2) is the contract for runtime, SDK, renderer and official plugins. Changes after B2 are additive only and go through the lead.
- **Tauri API** (spec §8): commands `plugins_list`, `plugins_index`, `plugin_install`, `plugin_set_enabled`, `plugin_set_permissions`, `plugin_set_visibility`, `plugin_remove`, `plugin_export_data`, `plugin_reset_data`, `plugin_dev_load` (+ additional ones a task needs, e.g. `plugin_ui_action`, `plugins_set_all_off`, `plugin_repo_*`, `plugin_update_*`, `plugin_pin`, `plugin_diagnostics`); events `plugins://changed`, `plugins://install-progress`, `plugin://ui`. After any command change: regenerate `ui/src/bindings/commands.ts`.
- **Store**: one migration per task that needs it is forbidden; B3 adds all four tables in one migration at the next free index (re-check index at commit time: "next free version at merge time").
- **Locales**: each UI task creates `ui/src/locales/<all 24>/<its namespace>.json` (English source, then translated). Namespaces: `pluginSettings`, `pluginPermissions`, `pluginBrowse`, `pluginSlots`, `pluginStrip`, `pluginChat`, `pluginDialog`, `pluginNotifications`, `pluginRepos`, `pluginDeveloper`.
- **Test signing key**: never committed. Generated per test run with `mumcord-plugin keygen` into the run's cache dir; the public key goes to `MUMCORD_PLUGIN_REGISTRY_KEY` (dev builds only, FR-6.53).
- **Test hooks** (debug builds only, env vars): fake app version for re-hash (AC-6.21), force the daily revocation/update check (AC-6.20), shorten back-off timers (AC-6.8). Must be compiled out of release builds.
- **Privacy/logging**: never log payloads, kv values, secrets, `net.http` content, chat text (spec §9).
- **Build cache**: `~/.cache/mumcord-build/<agent>`; delete when done (CLAUDE.md).

## 2. Tasks

Legend: deps = must be committed on `epic/plugins` first. ⚠ OUTWARD = touches GitHub outside this repo; needs user approval before start.

### Wave 1 – foundations (no dependencies, run in parallel)

**B1 – Bring in PR #297 and all epic dependencies** · simple
Cherry-pick `b96df0a8`, `bee0bfee`, `bf975b6e` onto `epic/plugins`, regenerate `Cargo.lock` and third-party notices, and add every new workspace dependency of the epic up front (list in §0) plus empty member crates `crates/plugin-sdk` and `tools/mumcord-plugin`, so later tasks never touch root Cargo files. Add `mumcord-store` as dependency of `crates/plugins`.
- Covers: FR-6.9, §8 (wasmtime, minisign-verify, sha2).
- Owns: `Cargo.toml`, `Cargo.lock`, `THIRD_PARTY_NOTICES`, `ui/public/third-party-notices.*`, `crates/plugins/Cargo.toml`, `crates/plugin-sdk/Cargo.toml` + stub `lib.rs`, `tools/mumcord-plugin/Cargo.toml` + stub `main.rs`, `CHANGELOG.md` (one Unreleased entry for the epic).
- Contracts: `crates/plugins` keeps `rust-version = "1.97"`; workspace stays 1.90. Release profile must still build on all three OSes (check with the cross-lint trick for windows/macos).

**B3 – Store: plugin tables and settings keys** · simple
One migration with `plugin`, `plugin_kv`, `plugin_repository`, `server_plugin_suggestion` (spec §7.1, incl. `removed_at`, `installed_at`, `disabled_reason`, `previous_version`, `sender_pin`), store API (CRUD, kv with per-plugin quota accounting, 30-day purge query, max 10 repos), and the settings keys `plugins.updates`, `plugins.all_off`, `plugins.badge_plugin`, `plugins.mirror_url`, `developer_mode`, `plugins.last_official_sequence`, `plugins.revoked_checked_at`.
- Covers: §7.1, FR-6.20 (quota), FR-6.31 (grants stored), FR-6.62 (data kept 30 days), FR-6.74 (≤ 10).
- Owns: `crates/store/src/db.rs` (MIGRATIONS only), `crates/store/src/plugins.rs` (new), `crates/store/src/lib.rs` (mod line), `crates/store/src/settings.rs`, `src-tauri/src/commands/settings.rs` (settings mirror only), `ui/src/bindings/commands.ts` (regen).
- Deps: none.

**B5 – Protocol: low-priority lane for plugin messages** · complex
Add a second, lower-priority queue for plugin data in the client's pacing so core plugin data (watch party, screen share, relay, history sync, …) always goes first; plugin messages wait and are dropped after 10 s, with a drop counter returned to the caller.
- Covers: FR-6.36 (priority, 10 s drop), AC-6.12 (watch party stays responsive).
- Owns: `crates/protocol/src/ratelimit.rs`, `crates/protocol/src/client.rs` (plugin-data send path only).
- Contracts: new `send_plugin_data_low(receivers, data_id, data) -> Result<Ticket, CommandError>` (or equivalent) whose drop is observable; existing `send_plugin_data` keeps priority. Server limit stays the source of truth (≤ 1000 bytes).

**B6 – E2E: two Mumcord clients in one run** · complex
Extend the E2E harness so a spec can drive two app instances A and B (separate home/data/keyring dirs, separate WebDriver sessions, e.g. wdio multiremote) against the same test server, plus helpers to start the app with extra env (`MUMCORD_PLUGIN_REGISTRY_KEY`, registry URL, `--no-plugins`, test hooks) and to restart an instance.
- Covers: spec §10 "E2E with two clients"; enables C2–C4, C6, C9.
- Owns: `e2e/wdio.conf.ts`, `e2e/helpers/app.ts`, new `e2e/helpers/apps.ts`, `e2e/container-run.sh`, `e2e/README.md`.
- Contracts: existing single-app specs must stay green; respect the shared e2e lock (only one agent fixes E2E at a time).

**B7 – ⚠ OUTWARD: create the `Mumcord/plugins` repository skeleton** · simple · needs user approval
Create the public repo with the layout of FR-6.54 (`plugins/<id>/plugin.toml`, `versions/`, `policy/REVIEW.md`, `revoked.toml`, `docs/`, `schemas/`, `template/`, one folder per official plugin), licences (AGPL-3.0-or-later for plugins, Apache-2.0 for template/docs), branch protection (1 approval + green CI), GitHub Pages, and an empty protected environment `publish`. No keys.
- Covers: FR-6.54, FR-6.56 (review rule), §10 prerequisite.
- Owns: the new repo only.
- Owner steps (cannot be done by agents): generate the official repository key pair (active + offline backup), put the private key into the `publish` environment, keep the backup offline, create the `repository_dispatch` token for #281.

**B8 – ⚠ OUTWARD (repo `Mumcord/mumbot`): module `plugins` (mumbot#20)** · simple · needs user approval
In the mumbot repo: module `plugins` that sends `mumcord.srvplugins` `{v, seq, plugins:[{id, min, src, key?, for}]}` (≤ 8 entries, `for` ≤ 40 chars, no `level`) to Mumcord clients announcing hello feature `server-plugins`, ≤ 1 per minute per client; TOML config, example config, CHANGELOG; a release so Mumcord's E2E can pin it.
- Covers: FR-6.77/6.78 (sender side), §10 mumbot, AC-6.26.
- Owns: `Mumcord/mumbot` only; afterwards one line in `scripts/fetch-mumbot.sh` (version + sha) in this repo if the tester binary is used.

**B20 – Deep link `mumcord://plugins/<id>`** · simple
Register the `mumcord` scheme (tauri.conf, Linux desktop entry, macOS/Windows registration), parse `mumcord://plugins/<id>`, ignore anything else, and emit an "open plugin details" event to the UI (Browse handles it in B24). Never installs.
- Covers: FR-6.65, AC-6.30 (backend part).
- Owns: `src-tauri/tauri.conf.json` (schemes), `src-tauri/src/deeplink.rs`, `src-tauri/src/desktop_entry.rs`.
- Contracts: event `plugins://open-details {id}`; official-id check happens in the UI against the loaded index.

### Wave 2 – core crate (after B1)

**B2 – WIT package `mumcord:plugin@1.0.0`** · complex · deps B1
Write all interfaces: `types` (peer {session, name, key?}, errors `not-permitted|too-large|rate-limited|no-recipients|hidden|quota-exceeded`), `log`, `storage` (scope identity/global), `timers`, `clock`, `notify`, `theme`, `random`, `peers` (send channel/direct, kind, game burst, presence snapshot), `ui` (declarative tree: all components of FR-6.41, slots of FR-6.40, set-tree per slot, dialog open/close), `chat` (send, supersedes hash), `secrets` (reference-only), `http` (request with secret header refs and file handles), and the export `on-event` with all events of FR-6.16 plus `start(info)` incl. previous version (FR-6.14). Document every limit in comments.
- Covers: FR-6.1, FR-6.14, FR-6.16, FR-6.18, FR-6.40/41 (shape), §8 WIT split.
- Owns: `crates/plugins/wit/**`.
- Contracts: this is the API for B10–B19, B25a, B26, B39–B42. Freeze after commit; later changes additive only.

**B4 – Manifest, permission catalogue and package rules to spec** · complex · deps B1
Bring `manifest.rs`/`package.rs`/`version.rs` and the JSON schema to FR-6.10–6.13/6.15: `authors`, `icon`, `commands[]`, `net`, `limits.memory_mib`, `storage.quota_kib`, `messaging.*`; the closed 19-id catalogue with risk colour, availability (official-only) and English text in a new `permissions.rs`; errors with the exact user texts; `web/` only with `ui.webview`; README/LICENSE required for official; case-clash check; SVG sanitising at install (ammonia + allow-list); atomic install.
- Covers: FR-6.10–6.13, FR-6.15, FR-6.27, FR-6.28, FR-6.12, FR-6.42 (SVG part), AC-6.9 (load error text), AC-6.16 (official-only refusal text).
- Owns: `crates/plugins/src/{manifest,package,version,i18n,permissions}.rs`, `crates/plugins/schema/**`, `crates/plugins/tests/package.rs`.
- Contracts: `permissions::CATALOGUE: &[Permission { id, risk, official_only, text_en }]`; `Manifest::validate(source: Source)` where `Source = Official | Repo{..} | Dev`.

**B10 – Runtime: thread per plugin, limits, back-off, cache, diagnostics** · complex · deps B2, B4
Evolve `runtime.rs`/`host.rs`: one store and OS thread `mumcord-plugin-<id>` per plugin, event queue 256 (drop oldest non-UI, count), talking coalesced ≤ 10/s, all limits of FR-6.3 (epoch 10 ms, start 500 ms, call 50 ms), any non-`mumcord:plugin@1.x` import refused with the FR-6.1 text, back-off 5 s/30 s/5 min and auto-off after 3 failures in 10 min, compiled cache `<cache>/plugins/<sha256>.cwasm` keyed by package hash + app version, log ring 200×200, diagnostics counters (CPU time, memory, drops, restarts). Global off switch.
- Covers: FR-6.1–6.8, FR-6.26, §7.2 runtime states; AC-6.8, AC-6.9 (engine side).
- Owns: `crates/plugins/src/{runtime,host,health,cache,diag}.rs`, `crates/plugins/tests/runtime.rs`, `crates/plugins/tests/fixtures/*.wat`.
- Contracts: host-call trait `HostApi` implemented by `api/*` modules (B11a/b, B12, B19); failure → `Recovery` event to the service.

**B11a – Host API: storage and secrets** · complex · deps B2, B3, B10
`storage` (identity/global scope, key ≤ 128 B, value ≤ 64 KiB, quota 256 KiB/≤ 1 MiB, `quota-exceeded`), `secrets` in the OS keychain (service `Mumcord plugins`, account `<plugin-id>/<name>`; written only from a UI `password` field, never readable, referenced by name; unavailable keychain → permission unavailable, no plaintext fallback), export/reset/delete helpers used by the service.
- Covers: FR-6.20, FR-6.25, FR-6.62 (export/reset part).
- Owns: `crates/plugins/src/api/{storage,secrets}.rs`.

**B11b – Host API: timers, clock, notify, theme, random, log, permission gate, peer key** · complex · deps B2, B10
Timers (≥ 250 ms, ≤ 32), clock (1 s), notify rate (1/10 min, 3/day, dropped in DND/quiet time, per-plugin switch) emitting a host notification event, theme tokens/mode read, `random.bytes` ≤ 256 from the OS RNG, the single permission gate used by every host call (grants changeable at runtime, revoked optional → `not-permitted` + events stop, revoked required → disable), `peer.key` = HMAC(per-install secret, plugin id, cert hash), absent without certificate.
- Covers: FR-6.18, FR-6.21–6.24, FR-6.29–6.31; AC-6.11, AC-6.15 (logic).
- Owns: `crates/plugins/src/api/{timers,clock,notify,theme,random,log}.rs`, `crates/plugins/src/grants.rs`, `crates/plugins/src/peer_key.rs`.

**B12 – Messaging logic (pure)** · complex · deps B2
Envelope encode/decode (FR-6.34), announce JSON `mumcord.plugins` build/parse with source tags (`@1`, `~<8hex>`, `~dev`, ≤ 32 entries, ≤ 1 per 10 s, Nobody not announced), recipient selection (same plugin id + same source, own channel, direct to one user, visibility Everyone/Friends/Nobody), per-plugin bucket (1 per 2 s, burst 3), game burst (4/s for 10 s, once per min, one plugin at a time), global bucket (1/s burst 5 → 4/s in burst), incoming > 5/s per sender per plugin dropped, all error codes, drop counters.
- Covers: FR-6.33–6.36; AC-6.12, AC-6.14 (logic).
- Owns: `crates/plugins/src/messaging/**`.
- Contracts: pure functions/structs with injected clock; no I/O. Blocked filtering happens before (engine), friends set comes from the caller.

**B13 – Engine bridge for plugins** · complex · deps B5
New `crates/engine/src/plugins.rs`: hello features `plugins` and `server-plugins`; receive/send raw `mumcord.p.*`, `mumcord.plugins`, `mumcord.srvplugins` (size bounded); per-server peer feature tracking; events for presence (join/leave + snapshot), talking, self state, chat activity (no text), mention of me, screen-share started; everything from blocked users dropped and counted; plugin sends through B5's low-priority lane; "server lacks plugin data" state (< 1.4 or rejected); `mumcord.srvplugins` accepted only from a registered sender pinned per server+identity (reuse spec-04 pin, shared if same bot); inspector: `mumcord.p.*` hex only, `mumcord.plugins`/`mumcord.srvplugins` decoded, "sender not registered".
- Covers: FR-6.5, FR-6.16 (sources), FR-6.17, FR-6.19, FR-6.35 (transport), FR-6.38, FR-6.39, FR-6.77 (sender check); AC-6.13, AC-6.17, AC-6.26 (unregistered part).
- Owns: `crates/engine/src/plugins.rs`, `crates/engine/src/mumcord.rs` (FEATURES lines), `crates/engine/src/session/mod.rs` (dispatch + event hook lines), `crates/engine/src/inspector.rs`, `crates/engine/src/lib.rs` (mod line), `crates/engine/tests/live_plugins.rs`.
- Contracts: engine API `Engine::plugin_events() -> Receiver<PluginEngineEvent>` and `Engine::plugin_send(server, receivers, data_id, bytes)`; no dependency on `crates/plugins`.

**B14 – Signing and verification** · complex · deps B4
minisign verify for `index.json`/`revoked.json` with the compiled-in official key + offline backup; `MUMCORD_PLUGIN_REGISTRY_KEY` accepted only in debug builds; install check order size → SHA-256 → author signature → manifest vs index (id, version, permissions) with the exact §5.7 texts; official status (FR-6.51); re-hash of installed packages once per Mumcord version → "Files of this plugin were changed" (with debug-only fake-version hook).
- Covers: FR-6.49–6.53; AC-6.18 (logic), AC-6.21.
- Owns: `crates/plugins/src/verify/**`.
- Contracts: official public key constant is a placeholder until the owner delivers the real key (B7 owner steps) – mark it clearly; release must not ship with the placeholder.

**B15 – Registry and repository client** · complex · deps B3, B14
Fetch official index (ETag, offline cache, `sequence` rollback protection, mirror URL, `User-Agent: Mumcord`, no identifiers), index model incl. website fields of FR-6.80, third-party repositories (HTTPS, http only localhost in Developer mode, ≤ 10, ≤ 2 MiB, ≤ 500 plugins, TOFU key pin, Expected key, `key_rotation` signed by old key, pause on key change, official ids/reserved prefixes ignored with a note), `revoked.json` fetch + bundled copy parsing.
- Covers: FR-6.55, FR-6.57, FR-6.70 (fetch/parse), FR-6.74–6.76, FR-6.80 (model); AC-6.18, AC-6.22 (logic).
- Owns: `crates/plugins/src/registry/**`, `crates/plugins/assets/revoked.json` (bundled copy).

**B16a – Safety fixture plugins** · simple · deps B2
WAT/SDK-free fixture components for: endless loop, > 32 MiB allocation, WASI import, message spammer + game burst, `notify` ×5, `chat.send` without gesture, a second badge plugin, a web view fixture that tries `fetch()`/`window.parent`, a third-party fixture declaring `ui.webview`/`net.http`/`secrets`, a version that fails `start`, a "Dice-like" fixture. Built by a script, not committed as binaries.
- Covers: fixtures for AC-6.6, 6.7, 6.8, 6.9, 6.12, 6.15, 6.16, 6.19.
- Owns: `crates/plugins/tests/fixtures/safety/**`, `scripts/build-plugin-fixtures.sh`.

### Wave 3 – service, app glue, SDK/CLI

**B17a – Plugin service: lifecycle** · complex · deps B10, B11a, B11b, B12, B14, B15
Facade over everything: install flow (prompt result → download → verify → atomic unpack → compile → start → progress events), enable/disable, required-permission revoke → disable, remove with/without data (30-day purge, restore on reinstall), reset, export JSON, turn off all (persisted) and `--no-plugins` (session only), developer folder load/reload/unload (never official, never official-only permissions, unloaded when Developer mode turns off), badge default (first installed with `ui.member-badge`), recipient resolution using announces, state machine §7.2.
- Covers: FR-6.6, FR-6.15, FR-6.29, FR-6.30, FR-6.60 (backend), FR-6.62, FR-6.63 (default), FR-6.73; AC-6.10, 6.11, 6.23, 6.27 (logic).
- Owns: `crates/plugins/src/service/{mod,install,lifecycle,dev}.rs`.

**B17b – Updates and kill switch** · complex · deps B17a
Plugin update setting auto/notify/off, check at start + every 6 h, auto-install only with same-or-fewer permissions and same author key, Pin version, rollback on failed start with reason, update-available notifications, revocation check once a day even when Off, `disable`/`warn` handling, official revocations apply to all sources, data never removed; debug-only "force daily check" hook.
- Covers: FR-6.66–6.70; AC-6.19, AC-6.20 (logic).
- Owns: `crates/plugins/src/service/{updates,revocation}.rs`.

**B17c – Chat integration logic** · simple · deps B11b
Command registry (name rules, built-ins win, first installed wins, "Command taken by <plugin>"), typed command → `ui-action`, `chat.send` gesture window (2 s, one message per gesture, ≤ 500 chars, prefix "[Name] "), supersedes matching (4-byte hash, same session, 10 s).
- Covers: FR-6.43–6.45; AC-6.7 (logic).
- Owns: `crates/plugins/src/chat.rs`.

**B18 – App glue (src-tauri)** · complex · deps B13, B17a
Create the plugin service in app state, connect engine plugin events/sends, `--no-plugins` CLI flag, all Tauri commands and events of §8 as thin wrappers, `plugin://ui` tree pushes and `plugin_ui_action`, text-message posting for `chat.send` (via the existing chat command path), notification hand-off to the UI centre.
- Covers: §8 API; FR-6.6 (flag), FR-6.44 (posting).
- Owns: `src-tauri/src/plugins/**`, `src-tauri/src/commands/plugins.rs`, `src-tauri/src/commands/mod.rs` + `src-tauri/src/lib.rs` (registration lines), `src-tauri/src/main.rs` (flag), `ui/src/bindings/commands.ts` (regen).

**B19 – `net.http` executor (official only)** · complex · deps B11a, B11b
Requests only to hosts the user typed into the plugin's form, exact host+port, no redirects, https for public / http only loopback+private, cert errors with the §5.6 texts, body ≤ 64 KiB, one file-picker upload ≤ 50 MiB, response ≤ 1 MiB, 10 s/120 s, ≤ 5 req/s, `{secret:…}` header substitution by the host, no content logging.
- Covers: FR-6.37; AC-6.25 (host side).
- Owns: `crates/plugins/src/api/http.rs`, `crates/plugins/src/api/files.rs` (file handles).

**B21 – UI shell: Settings → Plugins, Developer mode, palette, store** · simple · deps B3, B18
Section "Plugins" after Privacy with tabs Installed/Browse/Updates/Repositories/Developer (Developer only with Developer mode), "Turn off all plugins" in the header and the command palette, palette entry "Plugins", Developer mode toggle in Advanced, `--no-plugins` banner, a plugin store subscribing to `plugins://changed`; tabs are empty components owned by later tasks.
- Covers: FR-6.58, FR-6.6 (UI), FR-6.73 (toggle), §5.7 banner.
- Owns: `ui/src/features/settings/{sectionIds.ts,SettingsPages.tsx,settingsIndex.ts}` (plugin lines), Advanced section toggle, `ui/src/features/plugins/{index.ts,pluginStore.ts,PluginsSection.tsx}`, `ui/src/locales/*/pluginSettings.json`.

**B22 – Permission list component and texts** · simple · deps B4
Reusable permission list (risk colour + text "High risk" for red, required fixed, optional toggles, `suggested` default on) and the 19 plain-language texts in 24 locales.
- Covers: FR-6.27, FR-6.29, FR-6.32, FR-6.60 (rows), §5.3.
- Owns: `ui/src/features/plugins/permissions/**`, `ui/src/locales/*/pluginPermissions.json`.

**B25a – SDK crate `mumcord-plugin-sdk`** · complex · deps B2
`#[plugin]` macro, wit-bindgen bindings for `wasm32-wasip2` without WASI imports, typed UI tree builders, event enum, helpers for peers/storage; Apache-2.0; docs examples compile.
- Covers: FR-6.72.
- Owns: `crates/plugin-sdk/**`.

**B25b – CLI `mumcord-plugin` (author + registry commands)** · complex · deps B4, B14, B15, B25a
`new` (from the template), `build [--reproducible]`, `check`, `keygen`, `sign`, `serve-registry` (local fixture registry signed with a test key, incl. third-party repo mode and scenario switches: tampered hash, lower sequence, wrong key, extra-permission version, revoke entry, key change), `publish-pr`, `site-data`; Apache-2.0.
- Covers: FR-6.71 (except test/preview), FR-6.80 (site-data), AC-6.28 (most).
- Owns: `tools/mumcord-plugin/src/{main,new,build,check,sign,registry,publish,site}.rs`.

**B25c – CLI `test` and `preview`** · complex · deps B10, B11a, B11b, B12, B25b
Mock host running the same runtime with YAML scenarios (events in, expected messages/trees/notifications out), and `preview` that renders UI trees to a static HTML page.
- Covers: FR-6.71 (`test`, `preview`), AC-6.28 (`test`).
- Owns: `tools/mumcord-plugin/src/{test,preview}.rs`, `tools/mumcord-plugin/scenarios/**` (examples).

**B16b – Fixture registry setup for E2E** · simple · deps B16a, B25b, B6
Script that generates the test key, builds fixture + official plugins (official ones from a pinned `Mumcord/plugins` commit once available), starts `serve-registry` (official + one third-party repo), and exposes URLs/keys to E2E; scenario switching helpers for C8/C9.
- Covers: spec §10/§11 setup.
- Owns: `e2e/helpers/registry.ts`, `scripts/e2e-plugin-registry.sh`.

**B9 – Local mumbot instance (music + plugins)** · simple · deps B8 (for `plugins`), none for music
Optional testenv service (compose profile) running mumbot with the music module (HTTP API on 127.0.0.1, test API key from git-ignored `.env`), the `plugins` module announcing `dice`, registered on `v15`, plus test MP3s; README section. Client-only rule holds: no Mumble server change.
- Covers: AC-6.25, AC-6.26 setup.
- Owns: `testenv/mumbot/**`, `testenv/docker-compose.yml` (new service), `testenv/README.md`, `testenv/.env.example` (key placeholder only).

### Wave 4 – UI features (after B18 + B26 where noted)

**B26 – UI tree renderer** · complex · deps B2, B18
Render plugin trees with Mumcord components: all FR-6.41 components (sprite ≤ 12 fps, list with drag-reorder `{from,to}`, password → secrets, file-picker → handle), tokens-only colours, animation presets stopped by reduced motion; host-side limits (500 nodes, 16 KiB, 20 updates/s, 200 chars, accessible labels → Diagnostics entry) enforced in `crates/plugins/src/api/ui.rs`.
- Covers: FR-6.40–6.42, §5.8.
- Owns: `ui/src/features/plugins/render/**`, `crates/plugins/src/api/ui.rs`, `ui/src/locales/*/pluginSlots.json`.

**B27 – Plugins strip and panel popover** · simple · deps B26
Strip under the channel tree (icons, arrow keys, active panel 260×160, Expand → 420×560 popover, Escape), remembered collapse, hidden without panel plugins, greyed shared parts + tooltip on servers without plugin data.
- Covers: FR-6.64, FR-6.38 (UI), §5.2.
- Owns: `ui/src/features/plugins/strip/**`, mount line in `ui/src/features/channels/ChannelsPanel.tsx`, `ui/src/locales/*/pluginStrip.json`.

**B28 – Member badges, profile-card sections, chat decorations** · simple · deps B26
One badge per user from the chosen badge plugin, 100 badges without lag, profile-card section with 500 ms timeout, chat-decoration.
- Covers: FR-6.40 (badge, profile-card, decoration), FR-6.63; AC-6.6.
- Owns: `ui/src/features/plugins/badges/**`, hook lines in `ui/src/features/channels/treeRows.tsx` and `ui/src/features/members/useProfileCard.tsx`.

**B29 – Chat UI: cards, slash commands, via label, superseded lines** · complex · deps B26, B17c
Chat-card local items, autocomplete with plugin name, typed command never sent as chat, "via <plugin>" label, hidden fallback line with "Show text".
- Covers: FR-6.43–6.45 (UI); AC-6.2 (no duplicate line), AC-6.7.
- Owns: `ui/src/features/plugins/chat/**`, hook lines in `ui/src/features/chat/{Composer.tsx,MessageList.tsx,MessageCard.tsx}`, `ui/src/locales/*/pluginChat.json`.

**B30 – User-menu entry and plugin dialog** · simple · deps B26
One entry per plugin on own-channel users via `user-menu-requested(peer)`, modal dialog with focus trap and `dialog-closed`.
- Covers: FR-6.40 (user-menu, dialog); AC-6.25 (UI shell).
- Owns: `ui/src/features/plugins/dialog/**`, hook line in `ui/src/features/members/PersonMenu.tsx`, `ui/src/locales/*/pluginDialog.json`.

**B31 – Sandboxed web view** · complex · deps B26, B18
iframe with opaque origin (`sandbox="allow-scripts"` only), CSP `default-src 'none'` + package-local blob script/style/img, no IPC/storage/navigation/popups, bridge ≤ 16 KiB / ≤ 60 per s to its own plugin, Tab/Escape focus handling, loaded only when `ui.webview` granted; host bridge in `crates/plugins/src/api/webview.rs`.
- Covers: FR-6.46–6.48; AC-6.16.
- Owns: `ui/src/features/plugins/webview/**`, `crates/plugins/src/api/webview.rs`, CSP adjustments in `src-tauri/tauri.conf.json` only if needed (coordinate with B20 via lead).

**B32 – Notification centre: category Plugins** · simple · deps B18, B17b, B34
Plugin `notify` entries (name + icon), "Turned off after repeated crashes", update available, server suggestions with Install / Details / Not now / Don't ask again; silent in DND, no OS notifications.
- Covers: FR-6.4 (entry), FR-6.22, FR-6.69, FR-6.79; AC-6.8, 6.15, 6.26 (UI).
- Owns: `ui/src/features/plugins/notifications/**`, hook lines in `ui/src/features/notifications/centreSources.ts`, `ui/src/locales/*/pluginNotifications.json`.

**B34 – Server-suggested plugins logic** · simple · deps B13, B15, B17a
Resolve suggestions (official ids via index, revoked never, installed skipped, third-party → Add repository with Expected key), state per server+identity (`new`/`not-now` until seq change or 30 days/`dismissed`/`installed`), nothing sent back.
- Covers: FR-6.77–6.79 (logic); AC-6.26.
- Owns: `crates/plugins/src/suggestions.rs`.

**B23 – Installed tab and plugin details** · complex · deps B21, B22, B26
Rows with enable toggle, labels (Official / Unofficial · repo / Developer plugin / Turned off … / Turned off by Mumcord: reason / Files changed), permissions toggles, visibility, notifications switch, plugin settings form slot, Pin version, Diagnostics, Export data, Reset data, Remove dialog with "Also delete its data", badge picker, empty state.
- Covers: FR-6.8 (UI), FR-6.61, FR-6.62, FR-6.63, §5.4, §5.5, §5.7.
- Owns: `ui/src/features/plugins/installed/**` (shares `pluginSettings` namespace → use its own `pluginInstalled.json` namespace).

**B24 – Browse tab, install prompt, deep-link target** · complex · deps B21, B22, B20
Categories + search, Echolings highlighted, offline/never-fetched messages, details view, install prompt (visibility choice, red rows), first-install-per-repo warning with "I understand", progress + toast, verification error texts, deep-link opens details (official ids only).
- Covers: FR-6.59, FR-6.60, FR-6.61 (prompt), FR-6.65 (UI), §5.3, §5.7; AC-6.1, AC-6.30.
- Owns: `ui/src/features/plugins/browse/**`, `ui/src/features/plugins/prompt/**`, `ui/src/locales/*/pluginBrowse.json`.

**B33a – Updates tab** · simple · deps B21, B17b — FR-6.66–6.69 UI, AC-6.19. Owns `ui/src/features/plugins/updates/**`, `pluginUpdates.json`.
**B33b – Repositories tab** · simple · deps B21, B15 — add dialog (name, homepage, contact, fingerprint, Expected key), toggles, key-change dialog, mirror under Advanced, 10 max; FR-6.57 (mirror), FR-6.74–6.76, AC-6.22. Owns `ui/src/features/plugins/repos/**`, `pluginRepos.json`.
**B33c – Developer tab** · simple · deps B21, B17a — load/reload/unload folder, log view; FR-6.73, AC-6.27. Owns `ui/src/features/plugins/developer/**`, `pluginDeveloper.json`.

### Wave 5 – `Mumcord/plugins` repo work (after B7 approval)

**B37 – ⚠ OUTWARD: registry CI and publish workflows** · complex · deps B7, B25b, B25c
`check-pr` (schema, reproducible rebuild, author signature, permission-diff comment, mock-host load test, licence scan) and `publish` (sign `index.json`/`revoked.json` in the protected environment, monotonic `sequence`, release assets `<id>-v<ver>/<id>-<ver>.mcplugin`, `v1/permissions.json`, Pages, `repository_dispatch plugins-published`).
- Covers: FR-6.54–6.56, FR-6.80.
- Owns: `Mumcord/plugins/.github/**`, `schemas/`, `policy/REVIEW.md`.

**B38 – Template and author docs** · simple · deps B25a, B25b — template used by `mumcord-plugin new`, docs (API, permissions, publishing, third-party repos); Apache-2.0. Owns `Mumcord/plugins/{template,docs}/**`. Covers FR-6.72.

**B39 – Poll plugin** · complex · deps B25a (+ B25c for scenario tests) — FR-6.81. Owns `Mumcord/plugins/poll/**`.
**B40 – Dice roller plugin** · complex · deps B25a, B25c — FR-6.82 (commit–witness–reveal, game burst). Owns `Mumcord/plugins/dice/**`.
**B41a – Echolings: pet core** · complex · deps B25a — species/stages/colours, Cheer/Energy/Bond, Notes caps, Pat/Treat/Play/Nap, streaks, DND sleep, panel, badge, profile card, storage, animation Full/Subtle/Still. Owns `Mumcord/plugins/echolings/src/{pet,ui,storage}/**`. Covers FR-6.83 (core).
**B41b – Echolings: social + Echo Echo** · complex · deps B41a — pet card ≤ 100 B ≤ 1/min, visits (1 pat per friend per day), reactions, Echo Echo with native components. Owns `echolings/src/social/**`, `echolings/src/games/echo_echo/**`.
**B41c – Echolings: Snack Race (web view)** · simple · deps B41a, B31 — `echolings/web/**`, `echolings/src/games/snack_race/**`; hidden without `ui.webview`.
**B42 – Mumbot Music plugin** · complex · deps B25a, B19 — FR-6.84: setup form (Test connection via `GET /queue`), player, polling 2 s only while open, bot leaves → message, saved bots list in settings form. Owns `Mumcord/plugins/mumbot-music/**`.

### Wave 6 – CI, docs

**B43 – CI in Mumcord: plugin tests and E2E** · simple · deps B6, B16b
Add plugin runtime tests, fixture builds (WAT + SDK-built), E2E with two clients and the fixture registry, official plugins built from a pinned `Mumcord/plugins` commit, Rust 1.97+ for the app job.
- Covers: spec §10 CI.
- Owns: `.github/workflows/ci.yml` (plugin jobs), `e2e/shards.json`.

**B44 – PRIVACY.md plugins section** · simple · deps none (text from spec §9) — what plugins and repository hosts can see. Owns `PRIVACY.md`.

## 3. Verification tasks

All against local `testenv` (`v15`; `v13` for AC-6.17) with the fixture registry (B16b), debug build, `MUMCORD_PLUGIN_REGISTRY_KEY` set. Two-client checks use B6; the "official Mumble client C" is the `mumbot` tester CLI (`expect`/text checks), never a public server.

| ID | Covers | How | Deps |
|---|---|---|---|
| C1 | AC-6.1, AC-6.30 | E2E single app: Browse highlight, nothing installed, prompt colours + visibility, toast, strip icon; deep link `mumcord://plugins/dice` and a third-party id | B24, B27, B20, B16b, B39 |
| C2 | AC-6.2, AC-6.3 | E2E A+B (B6) + mumbot C sees "[Poll] …"; rejoin resend, creator leaves → frozen | B39, B29, B6 |
| C3 | AC-6.4, AC-6.7 | E2E A+B Dice verified ≤ 5 s, alone → Unverified, autocomplete, command never in chat, Dice-like fixture `not-permitted` | B40, B29, B16a |
| C4 | AC-6.5, AC-6.6 | E2E A+B friends: badge, profile card, visit/pat refusal, Echo Echo; Snack Race manual on all 3 OSes; badge picker switch | B41a–c, B28, B31 |
| C5 | AC-6.8, AC-6.9, AC-6.10 | Rust tests (limits, back-off with test hook) + E2E labels, notification entry, voice continues (mumbot `expect-audio`), turn off all + restart, `--no-plugins` banner | B10, B17a, B23, B32 |
| C6 | AC-6.11–AC-6.14 | Rust live tests (`crates/engine/tests/live_plugins.rs`, two engines) for rate/burst/blocking/visibility + E2E inspector rows, watch party responsive during burst | B12, B13, B17a, B5 |
| C7 | AC-6.15, AC-6.16, AC-6.17 | notify fixture ×5 + DND; web view fixture with a local proxy log (Linux E2E; Windows/macOS manual); third-party official-only refusal; connect to `v13` → greyed hint | B11b, B31, B27, B4 |
| C8 | AC-6.18–AC-6.21 | serve-registry scenarios: tampered hash, lower sequence, wrong key; Poll 1.1 auto / 1.2 asks / pin / failing start rollback; revoke with updates Off (forced check hook); byte change + fake newer version | B14, B15, B17b, B33a, B25b |
| C9 | AC-6.22, AC-6.23 | third-party fixture repo: fingerprint, label, one-time warning, key change pause, id `poll` ignored, 11th refused; remove/reinstall keeps pet, remove with data deletes data + secrets, export JSON | B15, B33b, B23, B11a |
| C10 | AC-6.24 | 5 plugins, 20 mumbot bots, A talks 2 min; jitter statistic compared automatically, audible dropouts = **manual listening check** | all runtime + plugins |
| C11 | AC-6.25 | local mumbot (B9): red rows, wrong/right key, skip/stop/volume/URL/upload, public http refused, key not in export, bot leaves | B42, B19, B30, B9 |
| C12 | AC-6.26 | registered mumbot `plugins` module announcing `dice`: centre entry, no OS notification, Install, Not now/seq, Don't ask again; unregistered hand-made message → inspector "sender not registered" | B8, B9, B13, B34, B32 |
| C13 | AC-6.27, AC-6.28 | Developer mode tab, folder load, logs, `net.http` refused, off → unloaded; CLI new/build ×2 identical hash/check/test/serve-registry → installs in client | B33c, B25b, B25c |
| C14 | AC-6.29 | axe on strip, prompt, settings, Poll card, Mumbot dialog; keyboard-only walkthrough; both themes; reduced motion freezes Echolings; one non-English locale; **screen reader spot check manual** | all UI |
| D1 | report | Collect results of C1–C14, open points, manual-check list, per-platform web view results, binary size delta | C1–C14 |

Every AC-6.1 … AC-6.30 is covered by a C task above.

## 4. Order, critical path, parallel batches

- **Batch 1 (now, parallel):** B1, B3, B5, B6, B20, B44; plus B7 and B8 once the user approves.
- **Batch 2 (after B1):** B2, B4 (parallel); B9 (music part).
- **Batch 3:** B10, B12, B14, B16a, B25a, B22 (after B2/B4).
- **Batch 4:** B11a, B11b, B13 (after B5), B15, B17c, B25b.
- **Batch 5:** B17a, B19, B25c, B39, B40, B41a, B42 (plugins repo tasks need B7).
- **Batch 6:** B17b, B18, B34, B16b, B37, B38, B41b.
- **Batch 7:** B21, B26 → then B23, B24, B27, B28, B29, B30, B31, B32, B33a–c, B41c, B43.
- **Then:** C1–C14 (each as soon as its deps are in), D1.
- **Critical path:** B1 → B2 → B10 → B11b → B17a → B18 → B26 → B29/B31 → B41c → C4 → D1 (≈ 11 steps). Second long path: B7 (approval) → B25a/B25b → B39–B42 → B16b → C1–C4.

## 5. Blocking open questions for the user (with recommendation)

1. **Create the new public repository `Mumcord/plugins` now?** (repo, GitHub Pages, branch protection, protected "publish" environment.) Recommend: **yes, now**; agents set it up, you only create the signing key and the website token (next question).
2. **Signing keys and token (owner only):** the official repository key (one active + one offline backup) and the `repository_dispatch` token must be created by you. Recommend: **we prepare a step-by-step guide; until then everything is tested with a throw-away test key**, and release is blocked until the real key exists.
3. **Work in `Mumcord/mumbot`:** add module `plugins` (mumbot#20) and publish a new mumbot release so our tests can use it. Recommend: **yes**.
4. **Plugin dialogs (`ui.dialog`): window or pop-up inside Mumcord?** The spec says "Open its own window" but also "focus trapped". Recommend: **a pop-up (modal) inside the main Mumcord window**, keep the permission text as is.
5. **Accept the three defaults of spec §13:** upload limit 50 MiB in Mumcord (the bot itself allows 20 MB and answers with a clear error above), Developer-mode plugins never get network/secrets/web view (Mumbot Music is tested via the local test registry), "Configure bot" appears on every user with a certificate. Recommend: **accept all three**.
6. **Two Mumcord clients in automated tests:** spec §10 asks for it; it costs one harness task (B6) and longer CI runs. Recommend: **yes, build it**; only Snack Race on Windows/macOS, audible dropouts and the screen-reader check stay manual.
7. **Keeping `epic/plugins` up to date:** Recommend: **the lead merges `main` into `epic/plugins` once a week** (and before the final merge) to keep conflicts small.

## 6. Verification approach

- **Unit tests** in `crates/plugins` (manifest, package, permissions, messaging buckets with injected clock, verify, registry parsing, chat rules, suggestions), WAT fixtures for runtime limits; `cargo test -p mumcord-plugins`, clippy for all three targets (cross-lint trick), `npm run lint/typecheck/test` for UI.
- **Rust live tests** (`crates/engine/tests/live_plugins.rs`) with two or more engine sessions against `testenv` `v15`: announce, routing, blocking, visibility, priority lane, unregistered `srvplugins`.
- **CLI tests**: `mumcord-plugin test` YAML scenarios for each official plugin (Poll tally, Dice protocol, Echolings caps, Mumbot Music against a stub HTTP server).
- **E2E** (WebdriverIO in the container): single-app specs for settings/browse/install/updates/safety; two-app specs (B6) for Poll, Dice, Echolings, blocking; mumbot tester CLI as official client C and as 20 bots; `serve-registry` as the fixture registry and third-party repo; local mumbot (B9) for AC-6.25/6.26; `v13` for AC-6.17. Commit after fast checks; CI runs the full E2E; rerun only failing specs locally; only one agent fixes E2E at a time.
- **Manual checks** (listed in D1): audible voice quality (AC-6.24), Snack Race + web view isolation on Windows and macOS (AC-6.16, risk §13), screen reader spot check (AC-6.29), release build ignores `MUMCORD_PLUGIN_REGISTRY_KEY`, binary size delta.

## 7. Commit convention

- Each agent commits **and pushes** its finished, verified task directly to `epic/plugins` (no work PRs, spec §10); before pushing: `git pull --rebase origin epic/plugins`, re-run the fast checks, regenerate `ui/src/bindings/commands.ts` / `Cargo.lock` instead of hand-merging them.
- Conventional Commits, one logical change per commit, scope = area: `feat(plugins): …`, `feat(engine): …`, `feat(ui): …`, `test(e2e): …`, `chore(deps): …`, `docs: …`; English, clear message.
- Stage files explicitly (`git add <paths>`), **never `git add -A`**; no secrets or private keys (test keys are generated at run time).
- No mention of Claude/AI, no `Co-Authored-By` trailers, no "Generated with" lines.
- Outward-facing work (B7, B8, B37 and anything in `Mumcord/plugins` or `Mumcord/mumbot`) only after user approval; mumbot changes follow that repo's own rules.
- Delete own build cache `~/.cache/mumcord-build/<agent>` when done.

---

## Contracts from finished tasks (maintained by the lead)

- **B5 (3315a4cf):** `Handle::send_plugin_data_low(receivers, data_id, data) -> Result<PluginTicket, CommandError>`; `PluginTicket::outcome().await -> LowOutcome::{Sent, Expired, Disconnected}` (dropping the ticket is fine); `Handle::plugin_data_expired() -> u64` (drops per connection). Core `send_plugin_data` unchanged and always goes first; low frames expire after 10 s; flush/disconnect don't wait for them.
- **K1:** registry public keys in `~/.config/mumcord-signing/plugins/{active,backup}.pub` (active id 1CB49831E26E61AD, backup id 7989D354F5AE80C5). Embed only public keys. Signing secrets `PLUGIN_SIGNING_KEY` / `PLUGIN_SIGNING_KEY_PASSWORD` live in the `publish` environment of Mumcord/plugins.
- **B7 (+fix 5dc6e28):** Mumcord/plugins has no repo-wide licence; each official plugin folder (`poll/`, `dice/`, `echolings/`, `mumbot-music/`) has AGPL-3.0, `template/` and `docs/` Apache-2.0. Pages at https://mumcord.github.io/plugins/; environment `publish`; branch protection 1 approval + `check`. Version files live at `plugins/<id>/versions/<ver>.toml`.
- **B20 (bbf7f399):** Tauri event `plugins://open-details` with payload `{id}` (plain string event, not tauri-specta typed); id validated `^[a-z][a-z0-9-]{2,31}$`; other links ignored; never installs. Schemes `mumble` + `mumcord` in tauri.conf.json; Linux desktop entry lists `x-scheme-handler/mumcord`. Open: cold-start links can be lost (no buffer) → B20b; Windows runtime registration in commands/app.rs still only `mumble` → B20b.
- **B1 (4b9bed6b, d15f8ec6, f485644f, 0023f7ba):** PR #297 work is in (`crates/plugins` manifest/package/runtime). New workspace deps: wit-bindgen 0.63, minisign-verify 0.3, minisign 0.10, hmac 0.13, toml 1, serde-saphyr 2 (reqwest, rand, ammonia, tokio, keyring, clap reused). `crates/plugins` (rust 1.97) depends on mumcord-store, minisign-verify, hmac, toml, rand, ammonia, reqwest, tokio. Stub crates `crates/plugin-sdk` (Apache-2.0, rust 1.90) and `tools/mumcord-plugin` (Apache-2.0, rust 1.97; clap, serde-saphyr, toml, minisign, zip, sha2).
- **Lead decisions after B1:** B25a uses wit-bindgen's `export!` instead of a new `#[plugin]` proc-macro crate (no new workspace member). B25b `serve-registry` uses a minimal std::net HTTP server (no new dependency). Docs (B25/B43) note that the CLI source is Apache-2.0 but the built binary links AGPL code.
- **B8 (Mumcord/mumbot PR #33, not merged/released):** data id `mumcord.srvplugins`, sent only to sessions whose `mumcord.hello` lists feature `server-plugins`. JSON `{"v":1,"seq":N,"plugins":[{"id","min","src","key"?,"for"}]}`; `src` = `"official"` or an https index URL; `key` = minisign public key, only for third-party; `for` required (1–40 chars); no `level`; ≤ 8 entries, ≤ 1000 B. ≤ 1 per minute per session. Bot refuses to start the module unless registered and server cert pinned. `seq` persisted, +1 when the list changes between restarts. B9/E2E needs a mumbot release (user's yes) and the fetch-mumbot.sh version + sha line.
- **B20b (b50f149b):** command `plugins_take_pending_link` (returns the newest pending id or null, clears it); live event `plugins://open-details` unchanged. B24 calls the command once on mount, after subscribing to the event. Windows runtime registration covers `mumble` + `mumcord` (`deeplink::SCHEMES`). Touched `src-tauri/src/lib.rs` (registration) and `ui/src/api/mock.ts` (stub). UI typecheck/lint/vitest NOT run for this change: the next UI task runs them first. Docs gap: `docs/engine-api.md` must list `plugins_take_pending_link` (for the docs task).
- **B2 (71067a1f):** WIT `mumcord:plugin@1.0.0` in `crates/plugins/wit/`: types, log, storage, timers, clock, notify, theme, random, peers, ui, chat, secrets, http, webview, events. The world imports all of them and exports start / on-event / stop. start-info is unchanged; right after start the host sends settings-changed, language-changed and theme-changed (plus self-changed if permitted). The UI tree is a flat list of nodes (node 0 = root, children by index). Slow calls answer through events (`http.request` → id → `http-response`; timers → `timer`). There's a shared error `invalid`, and http has its own error variant. 19 events (FR-6.16's 17 + http-response + webview-message). webview: `show(place = panel-popover | dialog, entry under web/)`, `hide()`, `post(text)`; incoming messages arrive as `webview-message`, ≤ 16 KiB each, ≤ 60/s. runtime.rs ALLOWED_IMPORTS lists all 15 interfaces; a call the host can't serve yet fails at load. GOTCHA: no double quotes inside `///` WIT doc comments (breaks bindgen). Shape is pinned by `crates/plugins/tests/wit.rs`; changes must be additive and go through the lead.
- **B4 (b3701f9f):** `permissions::{CATALOGUE, get(id), Permission{id, risk: Risk::{Green,Yellow,Red}, official_only, text_en}}` (19 ids). `manifest::Source::{Official, Repo{repository_id: i64}, Dev}`; `Manifest::parse(bytes)` then `manifest.validate(source)`; text constant `manifest::OFFICIAL_ONLY_TEXT`. `Package::read(bytes, source)`, `read_verified(bytes, source, verifier)`, `install(plugins_dir) -> <dir>/<id>/<version>/`, `file(name)`. Manifest fields: description, authors, categories, icon, `commands: Vec<Command{name,description,usage}>`, `net: Option<Net>`, storage_quota_kib, `messaging: Messaging` (non-optional, max_payload default 997), memory_mib, `has_permission(id)`. Installed SVGs are sanitised, so they differ from the package bytes: B14's per-version re-hash must hash the INSTALLED files, using a fingerprint recorded at install. Old version folders stay for rollback; the lifecycle service (B17a) cleans them up.
- **B3 (3a39b0e9):** migration v21 creates `plugin`, `plugin_kv`, `plugin_repository`, `server_plugin_suggestion` (server_id/identity_id are INTEGER FKs). `crates/store/src/plugins.rs`: PluginRecord, PluginVisibility, PluginRepository, NewPluginRepository, PluginSuggestion; constants MAX_PLUGIN_KEY_BYTES 128, MAX_PLUGIN_VALUE_BYTES 64 KiB, MAX_PLUGIN_REPOSITORIES 10, REMOVED_DATA_RETENTION_MS 30 days. Store methods: plugins(), plugin(id), save_plugin (upsert, kv kept), mark_plugin_removed, delete_plugin, purge_expired_plugins(now); plugin_kv_get/set(plugin, scope, key, value, quota_bytes, now)/delete/keys/all/usage/clear (quota = key + value bytes across scopes); plugin_repositories, add_plugin_repository (max 10, no duplicate URL), set_plugin_repository_enabled, pause_plugin_repository, record_plugin_repository_check, replace_plugin_repository_key, remove_plugin_repository; save_plugin_suggestion, plugin_suggestions(server, identity). Errors: PluginQuotaExceeded, PluginValueTooLarge, InvalidPluginKey, TooManyPluginRepositories, DuplicatePluginRepository. Settings: plugins_updates (PluginUpdates auto/notify/off, default auto), plugins_all_off, plugins_badge_plugin, plugins_mirror_url, developer_mode (mirrored to UiSettings); plugins_last_official_sequence, plugins_revoked_checked_at (store only).
- **Lead decision (B25a, overnight):** plugins target `wasm32-unknown-unknown` (NOT wasm32-wasip2, whose std pulls in 14 `wasi:*` imports the runtime rejects); `mumcord-plugin build` (B25b) turns the module into a component with wit-component. This deviates from FR-6.72's wording "target wasm32-wasip2" and must be recorded in the implementation report and a spec addendum (D1).
- **B16a (f8e7832e):** safety fixtures in `crates/plugins/tests/fixtures/safety/**` (builder + 11 WAT cores, 15 fixtures), built by `scripts/build-plugin-fixtures.sh [out-dir]` (default `$MUMCORD_FIXTURE_OUT` or `~/.cache/mumcord-build/plugin-fixtures`), each as unpacked folder + `.mcplugin`, no binaries committed. Map: AC-6.8 fx-endless-loop; AC-6.9 fx-memory-hog, fx-wasi-import; AC-6.12 fx-spammer, fx-game-burst (logs game-burst-started/-refused); AC-6.15 fx-notify-flood (logs notify-delivered); AC-6.7 fx-chat-send, fx-dice-like (`/fxroll`); AC-6.6 fx-badge-two; AC-6.16 fx-webview-probe (official; proxy on port 18999, `?probe=`; bridge assumed `window.parent.postMessage(string)`), fx-priv-webview, fx-priv-http, fx-priv-secrets; AC-6.19 fx-update-v1 / fx-update-v2-fails (id fx-update). Builder's `common_manifest()` still writes old `author`/`category` next to `authors`/`categories`: align it with B4 (B16b). Not yet wired into `cargo test`; B16b/host tasks load them from the out-dir.
- **B12 (0d633ddd):** `messaging::Messenger::new(now)`. `send(&SendRequest{plugin: PluginRef{id, source: SourceTag, visibility}, permitted, api_major, max_payload, target: Target::{Channel, Direct(session)}, kind, payload}, &[Member{session, is_friend, plugins: &[AnnounceEntry]}], now) -> Result<Outgoing{data_id, receivers, bytes, direct}, SendError>`. Check order: permission → Nobody → size → recipients → rate; only the rate step consumes tokens. Also `start_game_burst(id, permitted, now)`, `receive(&PluginRef, sender_session, Option<&Member>, bytes, now) -> Option<Delivery{sender, kind, payload}>`, `record_expired(id)` (call on B5 LowOutcome::Expired), `counters(id)`, `forget_session`, `forget_plugin`. `announce::build(iter (&AnnounceEntry, Visibility)) -> Vec<u8>`, `announce::parse(bytes)` (lenient), `AnnounceThrottle{mark_changed, poll(now), due_at(now), reset}`. `SourceTag::{Official, Repo([u8;4] key fingerprint prefix), Dev}`; the service maps Source::Repo{repository_id} to the fingerprint prefix. Interpretations: the game burst is a 4/s bucket starting full (cap 4); a match is id + source (not the API major); receiving requires the sender in my channel with the same id + source announced. Wiring into WIT host calls / engine events is B17a's job, with one Messenger per connection.
- **B25a (bfd2a1a0):** `impl mumcord_plugin_sdk::Plugin for X { start / on_event / stop }` plus `mumcord_plugin_sdk::export!(X);`. All 15 interfaces are crate-root modules; `events::Event` is the event enum. `ui::column()/row()/text()/button()/…` with `.child() .animate() .disabled() .color() .kind()`, then `ui::show(slot, tree)` flattens it (node 0 = root, depth-first). `storage::{get_json,set_json}`, `peers::{send_json,decode_json}`, `log::{debug,info,warn,error}`, `JsonError`. Bindings read `../plugins/wit` (no copy). Plugins are a cdylib built with `cargo build --release --target wasm32-unknown-unknown`; B25b `build` componentizes with `wit_component::ComponentEncoder`. Example: `crates/plugin-sdk/examples/hello.rs`. Follow-up B25c2.
- **B22 (59c50b3d):** `<PermissionList requested={{id, required, suggested?}[]} enabled={Set<string>} onToggle={(id,on)=>void} readOnly? />`; `defaultOptionalSelection(requested)` gives the initial `enabled` set (suggested optional ids). Namespace `pluginPermissions`: required, optional, risk.{green,yellow,red}, texts.<camelId> (e.g. texts.uiMemberBadge). Red rows show a "High risk" text, ▲ and a badge. A test fails if the TS risk table drifts from the Rust CATALOGUE. Gotcha: after adding a locale file, run `rm -rf node_modules/.vitest-cache`. Visibility choice and unofficial warning belong to B24.
- **B14 (76099a7e):** `mumcord_plugins::verify`. `RegistryKeys::official()` has the real K1 keys built in (debug builds also accept MUMCORD_PLUGIN_REGISTRY_KEY); `RegistryKeys::pinned(key)` takes a base64 line or a whole .pub file, for third-party repos; `.verify(bytes, minisig_text)` checks index.json / revoked.json; `check_sequence(seq, last_seen)` returns `VerifyFailure::Rollback` when seq is lower (keep the cached list). `verify_package(bytes, &IndexEntry{id, version, digest, author_keys, permissions}, author_sig_text, source) -> Verified{package, author_key, official}` checks size → SHA-256 → author signature → manifest (permissions ⊆ index). Errors `VerifyFailure::{Mismatch, Signature, Permissions, Rollback, Package}` with the §5.7 texts (MISMATCH_TEXT, SIGNATURE_TEXT, PERMISSIONS_TEXT). AUTHOR SIGNATURE FILE: `<id>-<ver>.mcplugin.minisig`, a detached minisign signature over the package bytes, next to the package. Integrity: `<plugins_dir>/integrity.json`. After install call `Integrity::load(dir)` + `record(id, ver, installed_dir)`, and `forget()` when a version dir is deleted. At startup call `recheck(&verify::app_version(env!("CARGO_PKG_VERSION")), &[(id, ver, dir)])`, which returns tampered ids → disable with reason `tampered` and TAMPERED_TEXT. Debug hook MUMCORD_PLUGIN_TEST_APP_VERSION.
- **B13 (b52e2f2d store, b5ec642d engine):** `mumcord_engine::plugins`. `Engine::plugin_events() -> Option<mpsc::Receiver<PluginEngineEvent>>` (take-once, bounded 1024, overflow dropped + counted). `plugin_send(server, &[u32], data_id, Vec<u8>) -> Result<PluginTicket, PluginSendError{NotConnected, Unsupported, TooLarge, InvalidId, NoRecipients}>` goes through the B5 low lane, with receivers filtered to present, non-blocked sessions whose hello has `plugins`. Also `plugin_presence(server)`, `plugin_shared_supported(server)`, `plugin_stats(server)` and `plugin_mentioned(server, sender_session)` (B18 calls it where message_sound gets mention=true). Events (each with server_id): SharedState, Disconnected, PeerCapable, Joined, Left, Talking, ScreenShareStarted, ChatActivity, Mentioned, SelfChanged, PeerData (`mumcord.p.<id>` + `mumcord.plugins`), ServerPlugins. People arrive as `PluginPeer{session, name, cert_hash}`; cert_hash is for computing peer keys only and never goes to plugins. Hello always lists `plugins` + `server-plugins`. Blocking a user emits Left for them; chat activity covers channel and tree messages only; srvplugins from a sender whose cert doesn't match the pin is dropped and shown in the inspector as "sender not pinned"; SelfChanged has no DND (B18 adds it). Store: table `plugin_sender_pin` (inside v21) with `plugin_sender_pin(server, identity)` / `set_plugin_sender_pin`. Docs gap: docs/engine-api.md must list these methods.
- **B10 (c5f35aa7):** `PluginHost::new(app_version, cache_dir: Option<PathBuf>, ctx: Arc<HostContext>, outbox: tokio UnboundedSender<HostEvent>)`; `load`, `enable` (blocks until start returns and resets the failure count; call it via spawn_blocking), `disable`, `unload`, `set_all_off(bool)` / `all_off()`, `send(id, wit::events::Event) -> bool`, `broadcast(&Event)`, `state(id) -> Option<RunState>`, `diagnostics(id) -> Option<Arc<Diagnostics>>` (`.snapshot()` → serde DiagSnapshot; `.message_dropped()` for B12). `RunState::{Disabled, Running, BackingOff{error, retry_at}, AutoOff{error}, AllOff}`. `HostEvent::{Started{id}, Failed{id, error: CallError, recovery: Recovery}}`; `Recovery::StayOff` = auto-off (show "Turned off after repeated crashes" + notification-centre entry). On Started the service sends settings, language and theme. Later tasks add HostEvent variants additively. `pub(crate) trait HostApi { fn add_to_linker(&mut Linker<PluginState>) }` in runtime.rs: each api/* module implements it plus the generated `bindings::mumcord::plugin::<iface>::Host for PluginState`, and adds ONE line to `HOST_APIS`. `PluginState` has pub(crate) id, ctx, outbox, log, diag. `HostContext` is an empty Default struct; tasks add fields additively. `log` lives in runtime.rs (LogApi) and B11b may move it to api/log.rs. `cache::{key, path, prune(cache_dir, keep)}`. WIT types are at `mumcord_plugins::wit`. Import refusal text: "This plugin uses features Mumcord does not provide".
- **mumbot 0.3.0 released** (https://github.com/Mumcord/mumbot/releases/tag/v0.3.0, includes the `plugins` module). For `scripts/mumbot.env`: MUMBOT_VERSION=0.3.0; MUMBOT_SHA256_linux_x86_64=2a294cbe3105e82d00a59a4ded77ce8e9bbe5bdf4deb1fdca8ba6422c1c619df; MUMBOT_SHA256_linux_aarch64=a501a9603a19863696555a6640ea8536c1f4eab16e1a09b5653a42fd82d1c354; MUMBOT_SHA256_windows_x86_64=9963fa8ab6bb7faf282289cc415111d0933f57c0d5a74b672cd88f51fc73a731; MUMBOT_SHA256_macos_universal=419b148c039045cfe58dd075003e99c21172322fa7c38a57e1285c2df35dce68.
- **F2 (568a4fa0):** safety fixtures are prettier-clean; the builder writes only B4 manifest fields (`authors` as plain strings). All 15 parse with B4 rules, BUT they have no README.md/LICENSE: B16b must either add them, for fixtures signed as official (e.g. fx-webview-probe), or load the others as Repo/Dev.
- **B15 (9f528c51 code, 7e4ae169 docs):** wire format in `docs/plugin-registry-format.md` (the repo address is the index.json URL; revoked.json has a `sequence`; key_rotation signs the new key's base64 line). `registry::Client::new(UrlPolicy::new(developer_mode))`. `fetch_official(&client, cache_dir, &official_base(mirror)?, last_seen) -> Loaded{index, freshness: Fresh|NotModified|Offline|Rejected(err)}`; the caller stores index.sequence. `fetch_revoked` merges with the bundled `crates/plugins/assets/revoked.json` and never fails offline. `RevocationList::find(id, ver, sha256_hex, author_key)` returns the strongest match (disable before warn). `IndexPlugin::{latest(), version(v), entry(v)}`, where entry() is a B14 IndexEntry. `client.download(url, size) -> Download{bytes, signature}` (body > size → Mismatch). Third-party: `check_repository_url(text, dev)`, `preview_repository(&client, url, expected_key, &is_official_id) -> RepoPreview{url, info{name, homepage, contact, public_key}, fingerprint, plugin_count, ignored}`, `fetch_repository(&client, cache_dir, &PluginRepository, &is_official_id) -> Loaded | Rotated{new_key, loaded} | KeyChanged{new_key, fingerprint}`; the caller persists last_sequence / replace key / pause("key-changed"). One cache dir per source (official, repo-<id>). UI texts are `registry::UNREACHABLE_TEXT` / `OFFLINE_TEXT`. Not covered: fetching v1/permissions.json.
- **Plugin repo tasks (B39–B42):** each works in a clone of Mumcord/plugins on its own branch `feat/<plugin>` and opens a PR there (don't merge; the lead merges). Plugins depend on `mumcord-plugin-sdk` via a git dependency on Mumcord/mumcord branch `epic/plugins` (path `crates/plugin-sdk`), build with `cargo build --release --target wasm32-unknown-unknown`, and get AGPL-3.0 (LICENSE already in each folder).
- **B11b (1bb23d4a):** permission gate `ctx.grants.check(&id, "<perm>") -> Err(Error::NotPermitted)`; the service calls `ctx.grants.set(id, granted)` / `forget(id)`. `PluginHost::send/broadcast` drop events the plugin lacks permission for (`grants::event_permission`). `grants::revokes_required(&manifest.permissions.required, &granted)` → if true, the service disables the plugin. HostContext fields: grants, timers, notify, theme, `peer_keys: Option<PeerKeys>`; build with `..HostContext::default()`. notify: `HostEvent::Notify{id, title, body}`; the service sets `ctx.notify.set_quiet(dnd || quiet_time)`, `set_enabled(id, bool)`, `forget(id)`; limits 1/10 min + 3/24 h, in memory. theme: service calls `ctx.theme.set(mode, reduced_motion)` and broadcasts `ctx.theme.info()`. THEME TOKENS (contract for B26): `api::theme::TOKENS` = text, text-muted, accent, danger, warning, success, surface, surface-raised, border. Peer key: `peer_key::PeerKeys::load_or_create(plugins_dir)` (secret `<plugins_dir>/peer-key.secret`), `.key(plugin_id, Option<cert_hash>) -> Option<String>` (64 hex); B17a loads it into `ctx.peer_keys`. Timers use one shared scheduler thread and are cancelled on stop/restart. log is in `api/log.rs`. `Event::DialogClosed` requires `ui.dialog`. If the OS RNG fails, random.bytes returns not-permitted.
- **B9 (8d99218d, 2d17f1bb):** mumbot is pinned to 0.3.0 everywhere (scripts/mumbot.env, and the engine's mumbot lib dep with `features = ["tester"]`). Local bot: `testenv/mumbot/up.sh` / `down.sh` (compose profile `mumbot`, image ghcr.io/mumcord/mumbot:0.3.0); music API on `127.0.0.1:${MUMBOT_PORT:-8081}`; registers bot `MumbotLocal` and pins the server cert; renders the git-ignored `bot.toml`; the API key comes from `MUMBOT_API_KEY` in `testenv/.env`; the `plugins` module announces `dice`; test MP3s live in `testenv/mumbot/media/`. Order for E2E: `testenv/up.sh v15`, then `testenv/mumbot/up.sh` (CERT_SET=B after `--cert B`). URL tracks only from host.docker.internal.
- **B11a (79f5d8f1, 4588fe13, 62922065):** build `HostContext{ storage: Storage::new(Arc<Mutex<Store>>), secrets: Some(Arc::new(PluginSecrets::os())), ..HostContext::default() }`. The service calls `ctx.storage.set_identity(focused server's identity, else the default)` whenever it changes; with none, identity-scope calls fail with `invalid`. HostContext helpers `export_plugin_data(id) -> JSON` ({format:1, plugin, entries:[{scope:"global"|"identity:<n>", key, text|base64}]}, never secrets), `reset_plugin_data`, `delete_plugin_data`, `purge_removed_plugins(now)`, all of which also delete keychain secrets; run them on a blocking thread. `PluginSecrets::available()` false → UNAVAILABLE_TEXT and the permission is unavailable. `set(plugin, name, value)` is ONLY for the settings password field; `pub(crate) get(plugin, name)` is for the net.http executor (B19). Secret names are 1–128 bytes with no control characters. A DB failure reaches the plugin as quota-exceeded (logged without keys/values).
- **B17c (05c51be7):** `mumcord_plugins::chat`. `CommandRegistry::new(builtins)`; `register(plugin, name, commands)` in install order; `unregister(plugin)` hands a name over to the next plugin that declares it. `resolve(line) -> Resolution::{Builtin, Plugin{plugin, action: UiAction{action, text}}, Unknown}`: for Plugin, send a `ui-action` with source `command` and don't post the text. `taken_by(plugin, name)`, `lost_commands(plugin)` ("Command taken by <plugin>"), `suggest(prefix)`. `GestureGate{gesture(plugin, now), send(plugin, name, text, now) -> Result<"[Name] text", SendError::{NotPermitted, TooLarge, Invalid}>}`: 2 s window, 1 message per gesture, ≤ 500 chars. `hash_line(line)` = first 4 bytes of SHA-256 as big-endian u32; `line_hash(plugin_name, text)` includes the "[Name] " prefix. `SupersedeTracker{card(...), text(message_id, sender, line, now)} -> Supersede::{None, Replace{card, message}}` (either order, same session, 10 s), `forget_plugin`, `forget_session`. B17a: grant checks (commands, chat.send, chat.supersedes) and gesture() on a UI click / command; Invalid → WIT `invalid`. B18: pass the plain, HTML-stripped text of incoming lines to `text()`, and post the returned prefixed line through the existing chat path.
- **B39 (Mumcord/plugins PR #2, b430449):** Poll in `poll/**`, with logic behind a `Host` trait (natively tested, 33 tests). Binary wire: kind 1 create {id u32, line_hash u32, question, options}; 2 vote {id, option} direct to the creator; 3 tally {id, closed, close_hash, counts u16[]}; 4 voters {id, part, parts, entries (option, name ≤ 16 B)}, ≤ 100 entries, split per 997 B. Componentized: only mumcord:plugin/*@1.0.0 imports. Open: Manifest::parse/CLI check (after B25b), scenario tests (B25c), registry entry `plugins/poll/*.toml` (publish), runtime load test (B25c2). Spec addendum: the panel question is 64 B (text-input limit) while the wire accepts 200 B (FR-6.81 vs FR-6.41). Plugin repo clones need `git config user.name/email` set to the user's identity per clone.
- **B41a (Mumcord/plugins PR #3, 238abf0 art + 290a036 core):** `echolings/`, with 33 host tests and a 236 KB wasm that imports clock, log, storage, theme, timers, ui. `src/pet::{Pet, Session, Now, Reaction, Refusal}`; balance in `src/pet/balance.rs` + README; storage keys `pet` / `settings` (identity scope, versioned JSON). B41b: others' pets (Talking of others, ScreenShareStarted, PeerMessage, others' ProfileCardOpened are currently ignored in lib.rs) and Echo Echo hooked into `ui::action::PLAY`. API gaps: (1) plugins can't read their own i18n files, so the strings are compiled in (`src/ui/i18n.rs`; en only); (2) there's no host call to open the panel popover. ASSUMPTION for B18: `chat-activity` also fires for the user's OWN channel messages (Echolings Notes rely on it).
- **F3 (b2cc0e37):** `GestureGate::command_gesture(plugin, now)` opens an 8 s window for typed commands (`COMMAND_GESTURE_WINDOW`); `gesture(plugin, now)` stays 2 s for clicks; still one message per gesture, and a later gesture replaces the earlier one.
- **B19 (1f5ae524):** `api/http.rs` + `api/files.rs`. HostContext `http: Http::new(&plugins_dir)` (persists `<plugins_dir>/net-hosts.json`) and `files: Files`; on reset/remove call `ctx.http.forget(id)` + `ctx.files.forget(id)`. UI glue (ui_action owner): when the user confirms a `server-address` text-input, call `ctx.http.allow(id, &text) -> Result<(), AddressError::Invalid>` ('host', 'host:port' or an http(s) URL; without port or scheme 80/443; ≤ 16 per plugin, oldest dropped). After a file-picker pick, call `ctx.files.register(id, &path) -> Result<FileHandle, FileError{NotAFile, TooLarge, Unreadable}>` (≤ 50 MiB, 16 per plugin, memory only) and pass the handle in ui-action `file`. http-error `host-not-allowed` also covers plain http to a public address; B42 shows "Plain http is only allowed for local addresses". TLS reasons are short texts. One upload in flight per plugin. Forbidden headers: Host, Content-Length, Transfer-Encoding, Connection.
- **B25b (354e9ae9, f9467dc5, a30d9ace):** CLI `mumcord-plugin`: new (embedded template, `--sdk-path`), build [--reproducible] (needs Cargo.lock; prints sha256), check [--official] [--key] (package only, not a folder), keygen [--unencrypted], sign, serve-registry, publish-pr [--open], site-data. Key password from env MUMCORD_PLUGIN_KEY_PASSWORD, else a prompt. serve-registry: `--packages <dir> --key <test.key> [--author-key] [--port] (127.0.0.1 only) [--repository NAME] [--sequence] [--scenario …]`; re-signs packages with the author key (default: the registry key); official base `http://127.0.0.1:PORT/v1/`, packages at `/packages/<id>-<ver>.mcplugin(+.minisig)`; runtime control `POST /_ctl/scenario?set=a,b` (empty clears), `POST /_ctl/reload` (rescan + raise sequence); scenarios tampered-hash:<id>, lower-sequence, wrong-key, extra-permission:<id>, revoke:<id>[@ver][:warn], key-change[:rotate]. Docs: permissions.json, "Registry source" (plugin.toml, versions/<ver>.toml, revoked.toml), "Local test registry" in docs/plugin-registry-format.md. B43 runs `cargo test -p mumcord-plugin -- --ignored`. The template's SDK git dep points at epic/plugins and must switch to main after the epic merge (final task).
- **B38 (Mumcord/plugins PR #4):** template/ is the real `mumcord-plugin new` output (check passes); docs/ has README, getting-started, manifest, api, permissions, building, testing, publishing, third-party-repositories. Follow-ups F4 and M1.
- **B25c seam (4e73d1de):** `mumcord_plugins::PluginIo` (crates/plugins/src/api/io.rs), `trait PluginIo: Send + Sync + Debug` with set_tree(plugin, Slot, Vec<Node>), clear, open_dialog(plugin, title), close_dialog, set_user_menu_entry(plugin, Option<label>), peer_send(plugin, Target, kind, payload), start_game_burst, presence -> Vec<Peer>, chat_send(plugin, text) (impl applies the gesture gate + "[Name] " prefix), line_hash(plugin, text) -> u32, supersede(plugin, card_id, sender, hash). Set via `HostContext{ io: Some(Arc<dyn PluginIo>), .. }`. Grant checks happen in api/{ui,peers,chat}.rs before the call (`api::ui::slot_permission`); io None → not-permitted. Calls run on the plugin thread within the 50 ms budget, so they must not block. webview returns not-permitted until B31. B17a implements PluginIo for the client; the CLI has a mock.
- **B40 (Mumcord/plugins PR #5, 117fb71):** Dice in `dice/**`, 32 native tests, `mumcord-plugin check --official` ok. Wire: kind 1 commit {id u32, C 32 B, canonical notation ≤ 32 B}, 2 witness {roller session u32, id u32, W 16 B}, 3 reveal {id, R 16 B, count u8 ≤ 2, W…}. "Not revealed" after 10 s from commit, or 5 s after the 2nd witness. The slash command arrives as `ui-action` with source Command, action "roll", value Text(args): B17a must deliver it that way. Needs F3 (8 s command gesture). MVP cut: large channels may send a few extra witnesses (only 2 count).
