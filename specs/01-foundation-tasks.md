# Epic 1 – Foundation · Task list

> Specification: `specs/01-foundation-specification.md` · Mode: team · Maintained by the lead only.
> Status values: `open` / `in progress` / `done` / `blocked`. Work branch: `epic-1` (pushes allowed, never `main`). The lead commits each finished task (owned paths only).
> Layout: `crates/{protocol,audio,store,engine,bot,positional}`, `tools/netsim`, `src-tauri/`, `ui/`, `testenv/`, `e2e/`. Translations per feature in `ui/src/locales/{en,es,de}/<feature>.json`, owned by that feature's task. Shared manifests (Cargo.toml, package.json, tauri config) are owned by B1 only.

## User decisions from phase A (2026-10-24)
- User runs a manual Windows + macOS checklist (~20 min each) at the end of stage 1a.
- Agents may push to branch `epic-1` so GitHub Actions runs; never `main`.
- User installs host tools: `sudo pacman -S github-cli orca xdotool`. tauri-driver via `cargo install`; WebKitWebDriver/xvfb only inside the Ubuntu E2E Docker image.
- User does a ~10 min headphone listening check at the end of each stage.

## Tasks

| ID | Title | Status | Owner | Depends on | Cx | Covers | Owned files/areas |
|---|---|---|---|---|---|---|---|
| A1 | Analysis & breakdown | done | a1-analyst | – | complex | whole spec | – |
| B1 | Scaffold & CI skeleton | done | b1-scaffold | – | complex | §8, §10 | root Cargo.toml, package.json, tsconfig, eslint, src-tauri/Cargo.toml, tauri.conf.json, capabilities, .github/, .gitignore, LICENSE, proto/ |
| B2 | Test server kit | done | b2-testenv | – | simple | §10, AC-1.5/1.16/1.21 infra | testenv/ |
| B3 | Engine API contract + UI mock | open | | B1 | complex | §8 | src-tauri/src/lib.rs, src-tauri/src/commands/*, ui/src/api/, ui/src/bindings/ |
| B4 | Protocol: control channel | open | | B1 | complex | FR-1.7–1.12, 1.14, 1.15, 1.22 | crates/protocol (except voice/) |
| B5 | Protocol: voice transport | open | | B4 | complex | FR-1.26, 1.24 | crates/protocol/src/voice/ |
| B6 | Store, identity, secrets | open | | B1 | complex | FR-1.2, 1.3, 1.8, 1.10, 1.19, 1.32, 1.33 | crates/store |
| B7 | Audio I/O, Opus, test backend | open | | B1 | complex | FR-1.4, §6 | crates/audio/src/{io,codec}/ |
| B8 | Audio DSP chain | open | | B7 | complex | FR-1.23, 1.21, 1.4 | crates/audio/src/dsp/ |
| B9 | Jitter buffer, mixer, bitrate | open | | B7 | complex | FR-1.24, 1.25, 1.18/1.19, 1.31 | crates/audio/src/{jitter,mixer,bitrate}/ |
| B10 | Bot harness (control) | open | | B2, B4 | complex | test infra | crates/bot (except voice) |
| B11 | Bot voice | open | | B5, B10 | simple | test infra | crates/bot/src/voice/, testenv/audio/ |
| B12 | Netsim proxy | open | | B1 | simple | AC-1.21–1.23 infra | tools/netsim |
| B13 | Engine: session & connection | open | | B3, B5, B6, B8, B9 | complex | FR-1.9, 1.10, 1.13, 1.14, 1.19, 1.22, 1.24–1.26 | crates/engine/src/{session,voice,people}.rs + matching commands |
| B14 | Engine: chat, follow, history, profile | open | | B13 | complex | FR-1.20, 1.28–1.34 | crates/engine/src/{chat,follow,profile}.rs + matching commands |
| B15 | Push-to-talk & global shortcuts | open | | B3, B8 | complex | FR-1.5, 1.21 | src-tauri/src/ptt.rs |
| B16 | Logging & diagnostics | open | | B3 | simple | FR-1.40, §9 | crates/engine/src/diag.rs |
| B17 | UI foundation | open | | B3 | complex | FR-1.35, 1.36, §5.5 | ui/src/{app,theme,i18n,a11y}/ |
| B18 | UI wizard | open | | B17 | complex | FR-1.1–1.6 | ui/src/features/wizard/ |
| B19 | UI servers & connection | open | | B17 | complex | FR-1.7, 1.9–1.11, 1.13, §5.4 | ui/src/features/servers/ |
| B20 | UI channels, members, profile card | open | | B17 | complex | FR-1.15–1.19 | ui/src/features/{channels,members}/ |
| B21 | UI chat core | open | | B17 | complex | FR-1.28–1.30 | ui/src/features/chat/ (except history/) |
| B22 | UI follow, PMs, history | open | | B21 | simple | FR-1.31–1.33 | ui/src/features/follow/, ui/src/features/chat/history/ |
| B23 | UI user panel & registration | open | | B17 | simple | FR-1.20, 1.22, 1.34 | ui/src/features/userpanel/ |
| B24 | UI settings | open | | B17 | complex | FR-1.3, 1.23, 1.27, 1.33, 1.35, 1.36, 1.39, 1.40 | ui/src/features/settings/ |
| B25 | Tray, close behaviour, deep links | open | | B3, B13 | complex | FR-1.6, 1.11, 1.37, 1.38 | src-tauri/src/{tray,window,deeplink}.rs |
| B26 | Notification sounds | open | | B9, B13 | simple | FR-1.27 | crates/engine/src/sounds.rs, assets/sounds/ |
| B27 | E2E harness | open | | B1, B2, B10 | complex | Q4.8, all ACs infra | e2e/ |
| B28 | Licensing & README | open | | B1 | simple | AC-1.29, §10 | THIRD_PARTY_NOTICES, README.md, about.toml |
| B29 | Packaging & cross-OS smoke | open | | B13, B27 | complex | AC-1.30, §10 | .github/workflows/release.yml, smoke script |
| C1 | Verify wizard & identity | open | | B18, B24, B27 | simple | AC-1.1–1.3 | e2e/specs/wizard |
| C2 | Verify servers, trust, links | open | | B19, B25, B27 | simple | AC-1.4–1.6 | e2e/specs/servers |
| C3 | Verify channels & voice | open | | B11, B13, B15, B20, B23, B27 | complex | AC-1.7–1.12 | e2e/specs/voice |
| C4 | Verify chat & follow | open | | B14, B21, B22, B26, B27 | complex | AC-1.13–1.18 | e2e/specs/chat |
| C5 | Verify registration & profiles | open | | B14, B20, B23, B27 | simple | AC-1.19, 1.20 | e2e/specs/profile |
| C6 | Verify network resilience | open | | B12, B13, B19, B27 | simple | AC-1.21–1.23 | e2e/specs/network |
| C7 | Verify app behaviour, theme, language, a11y | open | | B17, B24, B25, B26, B27 | complex | AC-1.24–1.27 | e2e/specs/app |
| C8 | Verify diagnostics, About, CI builds | open | | B16, B28, B29 | simple | AC-1.28–1.30 | e2e/specs/misc |
| G1 | **Stage 1a gate**: user manual checklist (Win/macOS, listening, KDE PTT/tray/Orca) + sign-off | open | user | C1–C8 | – | – | – |
| B30 | Mumble Link + 3D audio | open | | G1 | complex | FR-1.41, 1.43 | crates/positional/src/link/, crates/audio/src/spatial/ |
| B31 | Game plugin host | open | | B30 | complex | FR-1.42, 1.43 | crates/positional/src/plugins/ |
| B32 | Overlay window | open | | G1 | complex | FR-1.44–1.46 | src-tauri/src/overlay.rs, ui/src/overlay/ |
| B33 | 1b settings UI | open | | B30, B32 | simple | FR-1.41, 1.43, 1.45, 1.46 | ui/src/features/settings/positional/ |
| C9 | Verify positional audio | open | | B31, B33 | complex | AC-1.31, 1.32 | e2e/specs/positional |
| C10 | Verify overlay | open | | B32, B33 | simple | AC-1.33, 1.34 | e2e/specs/overlay |
| D1 | Implementation report | open | | C1–C10 | simple | all | specs/01-foundation-implementation.md |

## Contracts from finished tasks
(Filled by the lead from final reports.)
- **B2:** `testenv/up.sh <v15|v14|v13|limit> [--cert A|B]`, `testenv/down.sh [svc]`. Ports v15 64738, v14 64739, v13 64740, limit 64741 (TCP+UDP). Containers `testenv-<svc>-1` (`docker restart` for reconnect tests). SuperUser password `SUPERUSER_PASSWORD` in git-ignored `testenv/.env`. Certs generated on first up in `testenv/certs/{A,B}`. Channels Lobby/Raid/AFK seeded via sqlite (host needs `sqlite3`). Limit server: listener limits 0 (Listen refusal untested with a real client).
- **B1:** Crates `mumcord-{protocol,audio,store,engine,bot,positional}`, bin `netsim`, app `mumcord` (lib `mumcord_lib`, identifier `app.mumcord.client`). All dep versions in root `[workspace.dependencies]`. Proto code: `mumcord_protocol::proto` / `::udp_proto` (protoc vendored). Commands registered in `specta_builder()` in `src-tauri/src/lib.rs`; debug runs and `cargo test -p mumcord` regenerate `ui/src/bindings/commands.ts`. npm scripts: dev, build, tauri, lint, format, typecheck, test; app via `npm run tauri dev` (Vite :1420, root `ui/`). vitest+jsdom, setup `ui/src/test/setup.ts`, axe helper `ui/src/test/axe.ts`. Deps: sonora (instead of webrtc-audio-processing), `opus` crate (static), rustls with ring, TS ~6.0, ESLint 9, `rust-ini` pinned 0.21.1 in Cargo.lock (keep on cargo update). Placeholder icons.
