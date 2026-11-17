# Feature 15 – Forget local mutes on startup · Implementation report

> Specification: `specs/15-clear-local-mutes-specification.md`
> Task list: `specs/15-clear-local-mutes-tasks.md`
> As of: 2026-11-17 · Mode: team

## Run 1 – 2026-11-17

### Result in one sentence

Settings → Chat & Notifications now has a "Local mutes" group with the switch "Forget local mutes when Mumcord starts"; with it on, Mumcord clears every stored local mute on every server once at app start, before any connect, and leaves volumes, share volumes, blocked users and friends untouched, while off (the default) nothing changes, reconnects never clear and switching it on clears nothing immediately — all eight acceptance criteria are verified end-to-end in the running client.

### Acceptance criteria

| AC | Result | Evidence / note |
|---|---|---|
| AC-15.1 | ✅ | E2E `AC-15.1`: the group sits above Blocked users, the switch is off on a fresh profile, settings search for "mute" and "forget" finds it |
| AC-15.2 | ✅ | E2E `AC-15.2`: with the option off, the muted person's recording is exactly digital silence (600 s, peak 0, −∞ dBFS) both before and after the restart, and the mute icon stays |
| AC-15.3 | ✅ | E2E `AC-15.3`: after the cleared start the recording holds real speech (36 s, peak −18.4 dBFS) and no mute icon is shown |
| AC-15.4 | ✅ | E2E `AC-15.4`: stored row keeps volume 50 and share volume 40, the block and the friend stay, only `localMute` is gone (`crates/store/src/social.rs` test covers the same at store level) |
| AC-15.5 | ✅ | E2E `AC-15.5`: restart connecting only to server B clears server A's stored mute as well |
| AC-15.6 | ✅ | E2E `AC-15.6`: a forced reconnect while the app keeps running keeps the mute set in this session |
| AC-15.7 | ✅ | E2E `AC-15.7`: switching the option on leaves the person at digital silence (peak 0, −∞ dBFS) until the next start |
| AC-15.8 | ✅ | `docs/images/screenshots/15-local-mutes-dark.png` and `-light.png` (1440×900) committed, group above Blocked users |

### Try it yourself

1. In the main checkout: `cp testenv/.env.example testenv/.env`, then `./testenv/up.sh v15` (port 64738); `./testenv/up.sh v15b` (port 64742) for AC-15.5.
2. `npm run tauri dev`, finish the wizard, add `localhost:64738` and connect; a second client (or the test bot) joins so there is a person to mute.
3. Settings → Chat & Notifications: mute that person locally, then switch "Forget local mutes when Mumcord starts" on — nothing changes yet.
4. Quit Mumcord completely, start it again and reconnect: the person is audible and shows no mute icon, and per-person volumes, blocked users and friends are unchanged. Repeat step 3–4 with the switch off to see the mute survive.

### Completed tasks

| Task | Title | Owner |
|---|---|---|
| A1 | Analysis & breakdown | a1-analyst (scout) |
| B1 | Store: settings key + sweep | b1-store (worker) |
| B2 | Engine startup hook + API binding | b2-engine (worker, finished by lead) |
| B3 | UI group + search + en strings | b3-ui (worker) |
| B4a | Locales: bg, cs, da, de, el, es, et, fi | b4a-locales (worker) |
| B4b | Locales: fr, ga, hr, hu, it, lt, lv, mt | lead (agent launch blocked by capacity) |
| B4c | Locales: nl, pl, pt, ro, sk, sl, sv | lead (agent launch blocked by capacity) |
| B5 | E2E spec + screenshots + changelog | b5-e2e (worker) |
| C1 | Independent QA: all ACs in running client | c1-qa (worker) |
| B6 | Settings change tracking for the new key (QA gap 1) | b6-changes (worker) |
| B7 | Screenshot spec into `e2e/walkthrough` (QA gap 2) | b7-walkthrough (worker) |
| D1 | Implementation report | d1-report (worker) |

### Deviations from the specification

None — the feature is built as specified. Two QA findings were closed inside the plan: the new key was missing from the settings change tracking (B6) and the screenshot generator lived in the git-ignored `e2e/artifacts` (B7, now `e2e/walkthrough/screenshots-local-mutes.e2e.ts`). The committed palette screenshots are kept on purpose; the walkthrough spec writes truecolor RGB, which differs from them only in glyph antialiasing (SSIM 98.4 %, layout and text identical).

### Open points & known issues

- Nothing audio-related is left open: "muted" and "audible" are proven from the recorded caller audio (digital silence with peak 0 vs. speech with peak −18.4 dBFS), and the voice path itself is unchanged by this feature. A manual listening pass adds no information here and remains optional.
- The 24 translations come from the normal machine pass; the human review is tracked in issue #291.
- Pre-existing and unrelated: `mumcord-engine` test `full_image_switches` occasionally aborts with SIGABRT at process exit (green on rerun), and `npm run lint` reports one warning in `app/UpdateCheckPrompt.tsx`.
- Committed on the feature branch `degenhard/lhf` as four commits: engine and store, UI, E2E, docs. Nothing is pushed; the base branch is still to be decided (a milestone branch does not exist right now, since 0.9.0 shipped).

### Technical notes

**Behaviour**
- Store: `Settings.forget_local_mutes_on_start: bool` (default `false`; the container-level `#[serde(default)]` on `Settings` fills it in for records written before the key existed) and `Store::clear_local_mutes() -> Result<usize>` — one `unchecked_transaction()` with `UPDATE person_setting SET local_mute = 0` over all rows of all servers (FR-15.2, FR-15.8). `volume`, `share_volume`, blocks and friends are never touched. The returned row count is only used for a `debug!` log.
- Engine: the sweep runs at the top of `Engine::connect_at_start`, before the identity check and the connect loop, so sessions start from the cleared state (FR-15.2, FR-15.7). A failed sweep is logged at `warn!` and never stops the startup connects. Reconnects and server switches go through `Engine::connect` and never clear (FR-15.4); changing the setting takes effect from the next start (FR-15.5).
- Logs carry no names and no certificate hashes — only the count.

**Changed files**
- New: `ui/src/features/settings/sections/LocalMutesGroup.tsx`, `e2e/specs/chat/forget-local-mutes.e2e.ts` (shard 5 in `e2e/shards.json`), `e2e/walkthrough/screenshots-local-mutes.e2e.ts`, `docs/images/screenshots/15-local-mutes-dark.png`, `docs/images/screenshots/15-local-mutes-light.png`, `specs/15-clear-local-mutes-tasks.md`, `specs/15-clear-local-mutes-implementation.md` (this report).
- Modified: `crates/store/src/settings.rs`, `crates/store/src/social.rs`, `crates/engine/src/session/mod.rs`, `src-tauri/src/commands/settings.rs`, `ui/src/bindings/commands.ts` (regenerated), `ui/src/api/mock.ts`, `ui/src/features/settings/SettingsPages.tsx`, `ui/src/features/settings/settingsIndex.ts` + test, `ui/src/features/settings/changes.ts` + test, all 24 `ui/src/locales/*/settings.json`, `e2e/shards.json`, `CHANGELOG.md` (Added, issue #872).

**Tests**
- Store: `forget_local_mutes_on_start_defaults_off_and_round_trips`, `clear_local_mutes_sweeps_all_servers_and_keeps_volumes`, `clear_local_mutes_on_empty_table_is_fine` (+3).
- Engine: new module `forget_local_mutes_tests` — `start_with_the_flag_on_clears_mutes_before_connecting`, `start_with_the_flag_off_keeps_mutes`, `connect_never_clears_even_with_the_flag_on`, `turning_the_flag_on_clears_nothing_immediately` (+4).
- UI: `settingsIndex.test.ts` "finds the forget-local-mutes switch" (+1), `changes.test.ts` change detected and section reset (+2).
- E2E: `e2e/specs/chat/forget-local-mutes.e2e.ts`, 7 tests (AC-15.1 – AC-15.7), against `v15` plus `v15b` with scripted bots as the second clients; audio is graded from the app's recorded speaker output (peak in dBFS while a bot talks) and the stored `person_setting` row is read with `sqlite3` from the app profile, because share volume has no UI without a running share.

**Gates (2026-11-17)**
- `cargo fmt` clean · `cargo clippy --workspace --all-targets --locked -- -D warnings` clean · `cargo test --workspace --locked` 2986 passed, 27 ignored (only the pre-existing flaky `full_image_switches` SIGABRT at exit, green on rerun).
- `npm run typecheck` clean · `npm run lint` clean except the pre-existing warning in `app/UpdateCheckPrompt.tsx` · `npx vitest run` 325 files, 3577 tests green · `npx vitest run src/i18n/i18n.test.ts` green, 126 tests (all 24 locales complete).
- `npm run e2e -- --spec e2e/specs/chat/forget-local-mutes.e2e.ts` green twice (7/7, ~2m43 per run).

**Data, dependencies, platforms**
- No schema migration: one settings key inside the existing settings record; the `person_setting` table is written, not altered.
- No new crates, npm packages, permissions, CI steps or signing changes.
- No platform-specific code; E2E and QA ran on Linux with the local Docker test servers `v15` (64738) and `v15b` (64742). Standard Mumble server, no protocol change.
- Test setup needs `testenv/.env` (from `testenv/.env.example`) — no secrets in the repo.
