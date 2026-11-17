# Feature 15 – Forget local mutes on startup · Tasks

> Spec: `specs/15-clear-local-mutes-specification.md` · Mode: team (lead coordinates, one fresh agent per task)
> Status values: open / in progress / done / blocked. Only the lead edits this file.

| ID | Title | Status | Owner | Depends on | Complexity | Covers / files |
|---|---|---|---|---|---|---|
| A1 | Analysis & breakdown | done | a1-analyst (scout) | – | complex | breakdown below, no blocking questions |
| B1 | Store: settings key + sweep | done | b1-store (worker) | A1 | simple | FR-15.1/2/8 · crates/store/src/settings.rs, crates/store/src/social.rs |
| B2 | Engine startup hook + API binding | done | b2-engine (worker, finished by lead) | B1 | simple | FR-15.1–7 · crates/engine/src/session/mod.rs, src-tauri/src/commands/settings.rs, ui/src/bindings/commands.ts (regenerated) |
| B3 | UI group + search + en strings | done | b3-ui (worker) | B2 | simple | FR-15.1/5, AC-15.1/7 · new ui/src/features/settings/sections/LocalMutesGroup.tsx, SettingsPages.tsx, settingsIndex.ts (+test), ui/src/locales/en/settings.json, ui/src/api/mock.ts |
| B4a | Locales: bg, cs, da, de, el, es, et, fi | done | b4a-locales (worker) | B3 | simple | keys localMutes.* + searchTerms.localMutes in those 8 settings.json |
| B4b | Locales: fr, ga, hr, hu, it, lt, lv, mt | done | lead (agent launch blocked by capacity) | B3 | simple | keys localMutes.* + searchTerms.localMutes in those 8 settings.json |
| B4c | Locales: nl, pl, pt, ro, sk, sl, sv | done | lead (agent launch blocked by capacity) | B3 | simple | keys localMutes.* + searchTerms.localMutes in those 7 settings.json |
| B5 | E2E spec + screenshots + changelog | done (agent timed out after its runs finished) | b5-e2e (worker) | B2, B3 | complex | AC-15.1–8 · new e2e/specs/chat/forget-local-mutes.e2e.ts, docs/images/screenshots/15-*.png, CHANGELOG.md |
| C1 | Independent QA: all ACs in running client | done | c1-qa (worker) | B4a, B4b, B4c, B5 | complex | AC-15.1–8 · verification only, no code edits except test fixes reported to lead |
| B6 | Settings change tracking for the new key | done | b6-changes (worker) | B3, C1 | simple | ui/src/features/settings/changes.ts (+test): QA gap 1 |
| B7 | Screenshot spec into e2e/walkthrough | done | b7-walkthrough (worker) | B5, C1 | simple | e2e/walkthrough/screenshots-local-mutes.e2e.ts: QA gap 2 |
| D1 | Implementation report | done | d1-report (worker) | B6, B7 | simple | specs/15-clear-local-mutes-implementation.md |

## Contracts (from A1)

- B1 result: `Store::clear_local_mutes(&self) -> Result<usize>` (rows updated, for debug count log); settings field `forget_local_mutes_on_start: bool`, default false, serde default covers old records. Store tests/clippy/fmt pass.
- Settings key: store `forget_local_mutes_on_start: bool` (`#[serde(default)]`, Default = false) → API camelCase `forgetLocalMutesOnStart` via commands/settings.rs Settings struct + from_store + to_store. No new command; `settings_update` unchanged.
- B6 result: `top("chatNotifications", "localMutes.switch", ["forgetLocalMutesOnStart"])` added in changes.ts:201 + 2 tests (change detected, reset restores) → 296 settings tests green.
- B7 result: `e2e/walkthrough/screenshots-local-mutes.e2e.ts` reproduces both screenshots (SSIM 98.4 %, layout/text identical; committed palette PNGs kept on purpose). Walkthrough specs need no shards.json entry.
- Lead re-ran after B6/B7: `npm run typecheck` ok, `npm run lint` ok, `npx vitest run` 325 files / 3577 tests green.
- C1 QA result: AC-15.1 ✅, 15.2 🎧 (muted −∞ dBFS, survives restart), 15.3 🎧 (−15.7 dBFS after the cleared start), 15.4 ✅, 15.5 ✅, 15.6 ✅, 15.7 🎧, 15.8 ✅; cargo fmt/clippy/test (2986 tests), typecheck, lint, vitest (3575) green. Gaps: (1) new key missing from TRACKED_SETTINGS in changes.ts, (2) screenshot capture spec lives in git-ignored e2e/artifacts.
- B4 result: all 24 locales carry localMutes.title|switch|hint + searchTerms.localMutes; `npx vitest run src/i18n/i18n.test.ts` green (126 tests). B4a was delegated, B4b/B4c written by the lead because the subagent capacity limit (maxActiveAsyncRunsPerSession=1) blocked parallel launches.
- B3 result: LocalMutesGroup.tsx + SettingsPages + settingsIndex(+test) + en strings + mock default; typecheck/lint/full vitest green except the expected i18n parity test for the 23 locales (B4a–c).
- i18n keys: `localMutes.title|switch|hint`, `searchTerms.localMutes`.
- B2 result (verified by lead): hook at top of `connect_at_start` clears before identity check and connects, warns on error, debug count only; Settings API field mapped; `ui/src/bindings/commands.ts` regenerated (`forgetLocalMutesOnStart: boolean`); 4 engine tests + store tests + clippy/fmt green.
- Sweep: `Store::clear_local_mutes()` in social.rs, `unchecked_transaction()` (pattern channel_audio.rs:87), `UPDATE person_setting SET local_mute = 0` (all rows/all servers). volumes/share_volume untouched.
- Hook: top of `Engine::connect_at_start` (session/mod.rs:~1127), BEFORE identity early-return and connect loop; called once from src-tauri/src/lib.rs:912–917. Reconnects use `connect()` → never clear. Debug log count only, no names/hashes.
- UI: `<LocalMutesGroup {...props} />` above `<BlockedUsersGroup />` in chatNotifications case (SettingsPages.tsx:107–117). Keys `localMutes.title/switch/hint`, `searchTerms.localMutes`; switch via `update({ forgetLocalMutesOnStart: v })`; helper via aria-describedby. Index entry: section chatNotifications, group localMutes.title, control localMutes.switch.
- Verification: cargo fmt/clippy/test --workspace --locked; npm run typecheck / lint / test; E2E `npm run e2e -- --spec e2e/specs/chat/forget-local-mutes.e2e.ts` (Docker, mumbot second client, app.restart()); test servers testenv/up.sh v15 (64738) + v15b (64742), needs testenv/.env from main checkout.