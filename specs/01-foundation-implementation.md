# Epic 1 – Foundation · Implementation report

> Specification: `specs/01-foundation-specification.md`
> As of: 2026-10-24 · Mode: team
> Interim report for **stage 1a**. Stage 1b (positional audio, overlay; AC-1.31–1.34) is **not started**.

## Run 1 – 2026-10-24

### Result in one sentence

Stage 1a works end to end against standard Mumble servers (1.4 and 1.5 verified live): wizard and identity, servers with trust-on-first-use, channels, voice with noise suppression, text chat with following and history, registration and profiles, reconnect and TCP fallback, tray, themes, languages, diagnostics, and CI builds for Linux, Windows and macOS; what remains is the manual part of the gate (global push-to-talk on Wayland/Windows/macOS, tray, screen readers, listening checks, real-browser link click) and all of stage 1b.

### Acceptance criteria

Legend: ✅ verified · 🎧 automated check passed, listening check pending · 🖐 manual part pending.

| AC | Result | Evidence / note |
|---|---|---|
| AC-1.1 | ✅ | E2E `e2e/specs/wizard/*`: main window after 3.8–4.9 s; identity only in the OS keyring |
| AC-1.2 | ✅ | E2E: OpenSSL PKCS#12 imported via the real GTK file chooser; same certificate hash and registered name |
| AC-1.3 | ✅ | E2E: level meter moves; about 3.9 s of played-back voice found in the output WAV |
| AC-1.4 | ✅ | E2E `e2e/specs/servers/servers.e2e.ts`: server survives restart; password only in the keyring |
| AC-1.5 | ✅ | E2E: trust dialog shows SHA-256 fingerprint; a changed certificate blocks the connection ("certificate has changed", F15) |
| AC-1.6 | ✅ 🖐 | E2E: link opens, connects and joins Raid (app running and not running); click in a real browser pending |
| AC-1.7 | ✅ | E2E `e2e/specs/voice/voice.e2e.ts`: talking indicator after 74–85 ms (limit 200 ms) |
| AC-1.8 | ✅ | E2E: single click only shows chat; real double-click joins (9/9 runs, after F24) |
| AC-1.9 | ✅ 🖐 | E2E: voice activation ✅; push-to-talk on X11 ✅ (app focused and unfocused). Wayland (KDE portal), Windows, macOS pending |
| AC-1.10 | ✅ | E2E: self mute and deafen seen by Bot, no audio received |
| AC-1.11 | 🎧 | E2E: SNR +21 dB, noise floor −18 dB in pauses; listen to `e2e/artifacts/voice/noise-rec.wav` and live |
| AC-1.12 | ✅ | E2E: 50 % volume and local mute persist across Bot rename and reconnect |
| AC-1.13 | ✅ | E2E `e2e/specs/chat/*`: formatting rendered; `<script>` and external image not loaded |
| AC-1.14 | ✅ | E2E: Markdown and pasted image reach Bot; a 2000×2000 image is compressed to about 102 KB JPEG |
| AC-1.15 | ✅ | E2E: unread badge, ear icon for Bot, followed channel is silent |
| AC-1.16 | ✅ | E2E: clear error when listening is denied or limited; follow hidden for old servers |
| AC-1.17 | ✅ | E2E: history survives restart; clear history and "save history off" work |
| AC-1.18 | ✅ | E2E: private chat opens with sound (−30.5 dBFS), reply arrives; 5/5 runs after F22 |
| AC-1.19 | ✅ | E2E `e2e/specs/profile/profile.e2e.ts`: registration with backup hint, visible to Bot, option disappears |
| AC-1.20 | ✅ 🖐 | E2E: own avatar and comment seen by Bot, Bot's avatar in member list (F14); native file picker is manual |
| AC-1.21 | ✅ | E2E `e2e/specs/network/network-resilience.e2e.ts`: server restart and TCP kill; last channel, voice state and follows restored |
| AC-1.22 | ✅ | E2E: UDP blocked, TCP fallback in 4.6 s, voice both ways, back to UDP in 4.9 s |
| AC-1.23 | ✅ 🎧 | E2E: at 10 % loss longest gap 280–300 ms, UDP kept; bitrate in connection info 50 → 32 kbps, back to 50 in about 13 s. Listening check pending |
| AC-1.24 | 🎧 | E2E: chime 240 ms, −18.4 dBFS peak; judge subtlety in `e2e/artifacts/c7-audio/speaker-out.wav` |
| AC-1.25 | ✅ 🖐 | E2E: ask, minimize + remember, stays connected, no prompt next time; tray icon and menu are manual |
| AC-1.26 | ✅ | E2E: light/dark follows OS, override works, live switch to es and de |
| AC-1.27 | ✅ 🖐 | Keyboard-only flows 20/20; axe: 0 violations in real WebKit (21 tests). Screen reader (Orca, NVDA, VoiceOver) pending |
| AC-1.28 | ✅ | E2E `e2e/specs/misc/diagnostics-about.e2e.ts`: version, OS, devices, connection stats; no chat text, password, certificate or names (2 runs) |
| AC-1.29 | ✅ | E2E: About shows MIT and third-party notices including Mumble's BSD text |
| AC-1.30 | ✅ | GitHub Actions: CI, E2E and "Test builds" green on Linux, Windows, macOS (V9, V10); each build starts and its smoke test connects |
| AC-1.35 | ✅ | E2E `e2e/specs/voice/read-only-join.e2e.ts` 9/9: fresh connect lands in default channel muted and deafened; Join clears both |
| AC-1.36 | ✅ | Live test `crates/engine/tests/live_default_avatar.rs` runs all steps of the criterion |

### Try it yourself

1. Start test servers (Docker, `openssl`, `sqlite3` needed): `cp testenv/.env.example testenv/.env` (set passwords), then `testenv/up.sh v14`. Use `v15` (port 64738) for the newest server. Details: `testenv/README.md`. Reset to a clean state with `testenv/reset.sh v14`.
2. Install dependencies and start the client: `npm ci && npm run tauri dev` (prerequisites in `README.md`).
3. Complete the first-start wizard (name, microphone, finish).
4. Click "+" and add a server: address `localhost`, port `64739`, any username; accept the certificate fingerprint.
5. You land read-only in the default channel, muted and deafened. Double-click "Raid" to join: voice turns on.
6. Optional: run the bot (`cargo run -p mumcord-bot -- --port 64739 --name Bot --hold 600 join Raid`; the bot leaves after the command unless `--hold` keeps it connected) to have a second participant; try chat, follow, private messages, registration, avatar.
7. Open the signal icon in the user panel for transport, latency, loss and bitrate. Test links with `xdg-open mumble://localhost:64739/Raid` after enabling the handler in the wizard.
8. Network tests: `cargo run -p netsim -- run --listen 127.0.0.1:64800 --target 127.0.0.1:64739 --control 127.0.0.1:64801`, connect to port 64800, then `cargo run -p netsim -- ctl --control 127.0.0.1:64801 loss both 10`.
9. Automated suite: `npm run e2e` (Docker; about 5 min cold, 15 s warm).

### Completed tasks

| Task | Title | Owner |
|---|---|---|
| B1, V1–V10 | Scaffold, CI skeleton and CI checks | b1-scaffold, ci*-watch |
| B2, B2b | Test server kit, autoban off | b2-testenv, b2b-autoban2 |
| B3 | Engine API contract and UI mock | b3-contract |
| B4, B5 | Protocol: control channel, voice transport | b4-protocol, b5-voice |
| B6 | Store, identity, secrets | b6-store |
| B7, B7b, B8, B9 | Audio I/O and Opus, device filter, DSP chain, jitter/mixer/bitrate | b7-audio-io, b7b-devices, b8-dsp, b9-mixer |
| B10, B11, B12 | Bot harness, bot voice, netsim proxy | b10-bot, b11-botvoice, b12-netsim |
| B13, B13b | Engine session and connection, mic-test event | b13r-session, b13b-mictest |
| B14 | Engine chat, follow, history, profile | b14-chat |
| B15 | Push-to-talk and global shortcuts | b15-ptt |
| B16 | Logging and diagnostics | b16-diag |
| B17–B24 | UI foundation, wizard, servers, channels, chat, follow, user panel, settings | b17…b24r |
| B25, B26 | Tray, close behaviour, deep links; notification sounds | b25-tray, b26-sounds |
| B27 | E2E harness | b27r-e2e |
| B28, B28b, B28c | Licensing, README, third-party notices | b28-license, b28b-c-notices, b28c-notices |
| B29 | Packaging and cross-OS smoke test | b29-packaging |
| C1–C8, C3b, C4b, C6b, C7b, C8b | QA verification of AC-1.1–1.30 | c1-qa … c8b-qa |
| F1–F35 (excl. F8 listed below) | Fixes found by CI and QA, e.g. Windows manifest and test crash (F1), lock file (F3), Version before Authenticate (F5), connection info (F19), bitrate tuning (F20), TCP fallback hysteresis (F21), double-click join (F24), accessibility of tree and badges (F25, F27, F29), read-only connect (F30), user panel layout (F31, F34), default avatar (F32), listener talking ring (F33), avatar fallback (F35); F28 not reproduced, regression tests added | various f*-agents |
| F8 | E2E included in root typecheck | f8-tsconfig |
| S1, S2 | Spec addenda QA.2, QA.3 | s1-spec-addendum, s2-spec-default-avatar |
| Q1 | Final E2E suite (18/18), AC-1.29 and AC-1.35 specs | q1-e2e-final |
| D1a | This report | d1a-report |

Open: G1 (stage 1a gate, user), B30–B33, C9, C10, D1 (stage 1b).

### Deviations from the specification

- **Echo/noise processing:** the `sonora` crate (pure Rust WebRTC audio processing port) is used instead of `webrtc-audio-processing`, to avoid a native build on Windows and macOS. Sonora allocates per frame on the send thread (not the device callback); accepted.
- **Identity:** the `rsa` crate is used only to generate the RSA-2048 key (CN = name, 20 years); everything else uses rustls/ring and OpenSSL-compatible PKCS#12.
- **Bitrate:** ceiling 96 kbps and always 10 % below the server's limit, because servers drop bursts above their 7.2 s average. Floors: at least 32 kbps below 25 % loss, 20 kbps at 25–40 %, 12 kbps only above 40 %; recovery +25 %/s after 3 s of clean traffic.
- **Read-only connect (user decision QA.2, 2026-10-24):** every fresh connect lands in the default channel, muted and deafened; voice starts only after an explicit Join (also via `mumble://` link). Reconnect after a drop restores channel and voice state. Added as AC-1.35.
- **Default avatar (user decision QA.3):** a default avatar can be set in Settings and is sent once per server (opt-out). Added as FR-1.47 and AC-1.36.
- **Multi-server read-along** moved to its own epic after Epic 1 (user decision).
- **User panel:** names longer than about 8 characters are ellipsized at the default panel width (15 rem); the full name stays available as tooltip and accessible name.
- Mic test is record-then-play-back (up to 5 s), not a live loopback.

### Open points & known issues

- Text sends do not wait for server confirmation; a refused follow can show up twice (command error and server-denied notice); comment and avatar changes are reported as accepted after 3 s of silence from the server; server messages have an empty sender name.
- Comment editor is plain text: rich formatting of an existing comment is lost on save. The avatar size limit is not simulated in the mock.
- Linux tray tooltips are not shown.
- Mumble 1.3 server is untested live; listen refusal verified with the `limit` server only through the bot and E2E.
- No macOS UI automation available: macOS is covered by CI build, unit tests and the smoke test only.
- The E2E suite is flaky when a human is connected to the shared test servers (v15); all runs use `flock ~/.cache/mumcord-build/e2e.lock`. v15 refuses TLS for a few seconds after a reset.
- The F28 report (everyone appears to be talking) could not be reproduced; probably QA bots talking on v15. Regression tests exist.
- Device polling every 2 s costs about 70 ms on the control thread: listen for glitches (checklist).
- Test builds are unsigned; Windows and macOS show security warnings.
- Pending manual items are listed in the checklist below (G1).

### Technical notes

- **New dependencies:** `sonora`, `opus` (static libopus), `rustls` with ring, `rsa` (key generation only), `image` (chat and avatar resizing), `rusqlite` (bundled SQLite), `keyring`, WebdriverIO devDependencies for E2E. Third-party notices regenerated with `scripts/generate-notices.sh` (cargo-about 0.9.2); `rust-ini` is pinned to 0.21.1 in `Cargo.lock`.
- **Local data:** SQLite database `mumcord.sqlite3` in the app data directory, schema migrated v1 → v2 via `PRAGMA user_version` (v2 adds the per-server "default avatar sent" flag); `default_avatar.png` (128 px) next to it; new setting `sendDefaultAvatar` (default on) and wizard/log level settings. Certificate and server passwords live only in the OS keyring (service `app.mumcord.client`).
- **Environment variables:** `MUMCORD_AUDIO_BACKEND=null|file:<in.wav>:<out.wav>` (test builds with feature `test-backend`; release builds reject it), `MUMCORD_SKIP_BINDINGS_EXPORT` (skips regenerating `ui/src/bindings/commands.ts`), `MUMCORD_LIVE=1` (enables live tests: `cargo test -p mumcord-protocol -- --ignored --test-threads=1`), `MUMCORD_BOT_DATA` (bot certificates), `VITE_MUMCORD_MOCK=1` (UI against the mock engine).
- **Test infrastructure:** `testenv/` Docker servers (v15 64738, v14 64739, v13 64740, limit 64741); `crates/bot` scripted Mumble clients with voice; `tools/netsim` packet-loss proxy; `e2e/` Docker image with WebdriverIO and tauri-driver (`npm run e2e`); `scripts/smoke` for cross-OS smoke tests.
- **CI:** `ci.yml` (fmt, clippy, tests, lint, typecheck, UI tests on three OS), `e2e.yml` (Linux), `release.yml` named "Test builds" (AppImage, NSIS installer, DMG as artifacts `mumcord-<os>-unsigned-test-build`, plus a smoke test per OS).
- **Scale:** 46 commands and 17 events in the engine contract (`docs/engine-api.md`).
- **Platforms tested:** Linux (CachyOS/KDE, Ubuntu 24.04 in Docker/CI), Windows and macOS via CI only. Servers: Mumble 1.5.735 and 1.4.230 live; 1.3 untested.

## Manual checklist for the stage 1a gate (user)

Tick each box when it behaves as described under "Pass". If something fails, note what you did and what you saw; a screenshot helps. Total effort: about 20 min on Linux, 20 min each on Windows and macOS, 10 min listening.

### Preparation

- **Test servers:** on the Linux PC run `testenv/up.sh v14` (port 64739). Linux tests use `localhost`.
- **Windows and macOS test builds:** on GitHub open the repository, go to Actions, choose the workflow "Test builds", open the latest green run for branch `epic-1` and download the artifact at the bottom: `mumcord-windows-unsigned-test-build` or `mumcord-macos-unsigned-test-build`. Unzip it. The builds are unsigned, so a security warning is expected.
- **Server reachability for Windows/macOS:** the test server must be reachable from those machines. Two options. (a) Run the Docker test servers on the Linux PC and connect over the LAN using the PC's IP address (find it with `ip addr`), port 64739; allow ports 64739 TCP and UDP in the firewall. (b) Use a public server of your own, only if you explicitly agree to that. Do not use other people's public servers.
- Make sure the Bot can join: `cargo run -p mumcord-bot -- --port 64739 --name Bot --hold 600 join Raid` (optional but helpful for hearing and seeing a second participant).

### Linux / KDE (about 20 min)

- [ ] **Push-to-talk via KDE portal.** Settings → Voice → switch to push-to-talk and choose a key. KDE shows a dialog asking for permission. Pass: after allowing, holding the key while another window (for example a browser) is focused makes the transmit indicator light and the Bot hears you (let the Bot run `expect-audio`); releasing stops it.
- [ ] **Does the consent dialog repeat?** Quit Mumcord completely and start it again. Pass: push-to-talk still works. Write down whether KDE asked for permission again on this second launch (either answer is fine; we only need to know).
- [ ] **Tray icon and menu.** Connect to the server. Pass: a Mumcord icon is in the system tray; its colour reflects the state (green when connected); the right-click menu shows a status line, Show, Mute, Deafen, Quit, and each entry works.
- [ ] **Close dialog with the mouse.** Click the window close button while connected. Pass: a dialog asks whether to minimize to the tray or quit. Choose "Minimize to tray" with "Remember my choice" ticked: the window hides and stays connected (the Bot still sees you). Open it from the tray and close again: no dialog, it just hides.
- [ ] **Close dialog with Esc.** In Settings → Appearance, set "When you close the window while connected" back to "Ask every time", close the window, and press Esc in the dialog. Pass: the dialog closes and nothing happens: the window stays open and connected.
- [ ] **Avatar file picker.** User menu (click your name in the user panel) → Edit profile → "Choose picture…". (Settings → Profile only sets the default avatar.) Pass: the normal KDE file dialog opens, picking a PNG or JPG shows it in the user panel, and the Bot sees it. Also try a very large image: a clear notice is shown instead of a silent failure.
- [ ] **mumble:// link in a real browser.** In the last wizard step enable "Open mumble:// links with Mumcord" (Settings has no switch for it; to test later, reset the app data). In Firefox or Chromium open `mumble://localhost:64739/Raid` (create a link on any local web page or type it into the address bar). Pass: the browser asks to open Mumcord (confirm); Mumcord comes to the front, connects and you are in "Raid" with the microphone active (the Bot sees you there, not deafened).
- [ ] **Orca screen reader basics.** Start Orca (Super+Alt+S), then use only Tab, arrow keys and Enter in Mumcord. Pass: buttons and fields are read with sensible names (for example "Mute", not "button"); the channel list is read as a list with channel names; when the Bot talks, a talking announcement is heard; you can finish the wizard, connect, join a channel and send a message without the mouse.

### Windows (about 20 min)

- [ ] **Install the unsigned test build.** Run the `-setup.exe` from the artifact. Pass: Windows SmartScreen shows "Windows protected your PC"; click "More info", then "Run anyway"; the installer completes and Mumcord starts.
- [ ] **Wizard and connect.** Create an identity, pick microphone and speakers, speak (the level meter moves, "Test" plays your voice back), add the server (LAN IP, port 64739), accept the certificate. Pass: you land muted and deafened in the default channel; after double-clicking "Raid" you can talk and hear the Bot.
- [ ] **Push-to-talk with another app focused.** Switch to push-to-talk with a key such as F9 or Ctrl+Space. Open a game or any other app (for example Notepad) and keep it in front. Pass: holding the key transmits (the Bot hears you), releasing stops it, and nothing is typed into the other app unexpectedly.
- [ ] **Tray.** Pass: the Mumcord icon is in the notification area (maybe under the "^" overflow); the menu works (Show, Mute, Deafen, Quit); closing the window with "Minimize to tray" keeps you connected.
- [ ] **NVDA basics.** Start NVDA (Ctrl+Alt+N) and navigate with Tab and arrows. Pass: controls are announced with sensible names and the channel list is readable; talking events are announced.
- [ ] **mumble:// link.** Enable "Open mumble:// links with Mumcord" in the wizard, then paste `mumble://<LAN-IP>:64739/Raid` into a browser address bar. Pass: the browser asks to open Mumcord, which connects and joins "Raid".

### macOS (about 20 min)

- [ ] **Open the unsigned build.** Open the `.dmg`, drag Mumcord to Applications and start it. Pass: Gatekeeper says it cannot be opened because the developer cannot be verified; open System Settings → Privacy & Security, click "Open Anyway", confirm; Mumcord starts. macOS also asks for microphone permission: allow it.
- [ ] **Wizard and connect.** Same as on Windows. Pass: microphone meter moves, server connects, read-only start, voice after joining "Raid".
- [ ] **Push-to-talk with another app focused.** macOS may ask to allow Mumcord in System Settings → Privacy & Security → Accessibility or Input Monitoring; allow it and restart if asked. Pass: holding the key transmits while another app is in front. If macOS never asks and the key does nothing, note it as a failure.
- [ ] **Tray (menu bar).** Pass: the menu bar icon appears and its menu works; closing the window with "Minimize to tray" keeps you connected.
- [ ] **VoiceOver basics.** Start VoiceOver (Cmd+F5), navigate with VO keys or Tab. Pass: controls are read with sensible names, the channel list is readable, talking events are announced.
- [ ] **mumble:// link.** Enable "Open mumble:// links with Mumcord" in the wizard, then open `mumble://<LAN-IP>:64739/Raid` in Safari or Chrome. Pass: Mumcord opens, connects and joins "Raid".

### Listening with headphones (about 10 min)

Wear headphones to avoid feedback.

- [ ] **Noise suppression, recording.** Play `e2e/artifacts/voice/noise-rec.wav` (what the Bot received from Mumcord while the input contained keyboard and fan noise). Pass: speech is clear; the noise between words is quiet; speech is not cut off or underwater-sounding.
- [ ] **Noise suppression, live.** Join a channel with the Bot listening or recording and type loudly on the keyboard while talking. Pass: typing is strongly reduced and your voice stays natural.
- [ ] **Packet loss.** Start netsim as in "Try it yourself" step 8, connect via port 64800, let the Bot talk, then set 10 % loss. Pass: audio continues with at most brief crackles or short dropouts (under about a third of a second); the bitrate in the connection popover drops and returns to normal a few seconds after `reset`.
- [ ] **Notification sounds.** Play `e2e/artifacts/c7-audio/speaker-out.wav`, then have the Bot join and leave your channel and send a private message. Pass: the sounds are short and subtle, not startling, not annoying when repeated, and nothing is read out loud by a synthetic voice.
- [ ] **Device switching.** While the Bot talks, change the output device (and then the input device) in Settings, and plug or unplug headphones if you can. Mumcord checks devices every 2 seconds. Pass: audio moves to the new device within a few seconds without lasting breaks; also listen during normal operation for small regular clicks or gaps every 2 seconds (that would be a failure).

### Sign-off

- [ ] All boxes above are ticked or have a note on the failure. Tell the lead the result so that the gate G1 can be closed and stage 1b can start.
