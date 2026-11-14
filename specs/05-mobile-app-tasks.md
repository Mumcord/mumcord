# Epic 5 – Android and iOS app · Task list

> Spec: `specs/05-mobile-app-specification.md` · As of: 2026-11-13 · Only the team lead edits this file.
> Status values: `open` / `in progress` / `done` / `blocked`. Complexity follows the rule in `.claude/skills/dev/SKILL.md`.

## Intro

The epic spans two repositories:

- **Mumcord** (this repository): engine preparation and the desktop "Send to your phone" dialog. Work happens on one train branch `feat/mobile-engine`, created from `origin/milestone/0.9.0`; its pull request targets `milestone/0.9.0`.
- **mumcord-mobile** (`https://github.com/Mumcord/mumcord-mobile`, today only README and LICENSE): bridge crate, Android app, iOS app, CI, releases. Issues #1–#28 there mirror the spec areas and are referenced in the "covers" column as `m#N`.

Android leads: every feature is built and verified on Android first; the iOS task of the same area follows and is verified only in CI on macOS runners, because this machine cannot build iOS. Phase 1 is broken down to single-agent tasks. Phases 2–10 are coarse on purpose and get refined into single-agent tasks (bridge / Android / iOS / verification) when the phase is reached.

State of the analysis: based on reading the code and resolving the dependency tree for `aarch64-linux-android` and `aarch64-apple-ios` (`cargo tree --target …`). Nothing was compiled for those targets, because no Android or iOS toolchain is installed here; the first compile happens in tasks 1.1–1.8.

### What blocks a phone build of the engine today

| # | Finding | Where | Handled by |
|---|---|---|---|
| 1 | `mumcord-screenshare` always binds and links the C libraries libvpx, SVT-AV1 and dav1d (bindgen + pkg-config) and contains the desktop hardware encoders (AMF, NVENC, QSV, VAAPI). None of these libraries exist for the phone targets yet. | `crates/screenshare/build.rs`, `src/hw/` | 1.3 now, 7.1 later (cross-built libvpx and dav1d) |
| 2 | Secure storage is tied to the `keyring` crate. On Android it resolves to no store at all; on iOS it resolves to a generic Keychain store that does not set "this device only, not in backups". The backend interface (`SecretBackend`) exists but is private. | `crates/store/src/secrets.rs` | 1.1, 1.13 |
| 3 | `mumcord-positional` uses POSIX shared memory (`shm_open`), which Android does not have, and `mumcord-effects` pulls in the ONNX runtime. Both are unconditional dependencies of the engine. | `crates/engine/Cargo.toml`, `src/positional.rs`, `src/camera/` | 1.5 |
| 4 | Audio goes through cpal. cpal has Android and iOS backends, but offers no way to ask for the system's voice-communication mode, which FR-5.25 (system echo cancellation) and Bluetooth call routing need. Phones therefore need audio fed by platform code through an engine interface (the fallback the spec already names in section 13). | `crates/audio/src/io/` | 1.2 (build only), 3.1 |
| 5 | Certificate checking against the system store (`rustls-platform-verifier`) needs a one-time start-up call with the Android app context plus a small Kotlin companion library. | bridge | 1.10, 1.13 |

What already helps: the FreeBSD port left "no capture on this platform" fallbacks in the engine (camera, screen capture, share sound); the camera crate already has the public `CameraBackend` interface and the engine has `Engine::camera_set_backend`; `Engine::new` already takes the secret store as a parameter; `crash.rs` already knows Android and iOS; PipeWire and D-Bus are not pulled in on the phone targets.

## Contracts

Decisions every task agent must follow. A task that finds a reason to deviate reports it to the lead instead of deviating.

### Branches and pinning

- Mumcord: one train branch `feat/mobile-engine` from `origin/milestone/0.9.0`. Agents commit nothing and push nothing unless the lead says so; no agent opens its own pull request.
- mumcord-mobile: default branch `main`; feature branches `feat/<area>`; same commit rules as Mumcord (Conventional Commits, no AI attribution, never `git add -A`).
- The bridge depends on the engine as a git dependency with an exact reference, never a branch name. During development: `rev = "<full commit hash of feat/mobile-engine or milestone/0.9.0>"`. For a mobile release: `tag = "vX.Y.Z"` of a Mumcord release (pending product manager confirmation, see final report). The same reference is written once into `engine.lock.toml` at the mobile repository root; CI and scripts check out the Mumcord repository at that reference into `.engine/` to get `testenv/`, the desktop translation files and the theme tokens.
- Build caches: `CARGO_TARGET_DIR=~/.cache/mumcord-build/<agent>`, deleted by the agent when done. Gradle caches of an agent go to `~/.cache/mumcord-build/<agent>/gradle`.

### Cargo features (Mumcord repository)

Rule: desktop builds use default features and must behave exactly as before. Mobile uses `default-features = false` and switches on what it needs.

| Crate | Feature | Default | Content |
|---|---|---|---|
| `mumcord-engine` | `desktop` | yes | Umbrella: `positional`, `camera-effects`, `screen-capture`, `mumble-import`, `desktop-codecs`, `vp9`, `av1-decode` |
| `mumcord-engine` | `positional` | via `desktop` | Dependency `mumcord-positional`, module `positional`, Mumble Link handling |
| `mumcord-engine` | `camera-effects` | via `desktop` | Dependency `mumcord-effects` (ONNX runtime), background blur and effects |
| `mumcord-engine` | `screen-capture` | via `desktop` | Screen capture backends and share-sound capture (sending a screen share). Watching stays always available |
| `mumcord-engine` | `mumble-import` | via `desktop` | Import from the official Mumble client (`plist`, registry) |
| `mumcord-engine` | `desktop-codecs` | via `desktop` | Forwards `mumcord-screenshare/av1-encode` and `/hw-encode` |
| `mumcord-engine` | `vp9`, `av1-decode` | via `desktop` | Forward to `mumcord-screenshare`; mobile switches them on in phase 7 |
| `mumcord-screenshare` | `vp9`, `av1-decode`, `av1-encode`, `hw-encode` | all yes | One feature per C library / encoder family. With none of them the crate builds without bindgen, pkg-config and C linkage; opening a codec then returns the existing `Error::Codec` |
| `mumcord-store` | none | | `SecretBackend` and `SecretStore::with_backend` become public. The `keyring` dependency and `SecretStore::os*` exist only for `cfg(not(any(target_os = "android", target_os = "ios")))` |
| `mumcord-audio` | none in phase 1 | | Must compile for the phone targets. Phase 3 adds a platform-fed backend (`BackendKind::External`, lock-free rings, no allocation in the real-time path) |
| `mumcord-camera` | none | | OS backends stay target-gated. Phones feed frames through the existing `CameraBackend` / `FrameSink` / `FramePool` (NV12) |

Phone targets: `aarch64-linux-android`, `x86_64-linux-android` (emulator only), `aarch64-apple-ios`, `aarch64-apple-ios-sim`.

### Mobile repository layout

```text
Cargo.toml              Rust workspace (member: bridge)
rust-toolchain.toml     same Rust version as the Mumcord repository, with the four phone targets
engine.lock.toml        pinned engine reference (see "Branches and pinning")
bridge/                 crate mumcord-mobile-bridge (UniFFI)
android/                Gradle project
  app/                  Compose UI (package app.mumcord.mobile)
  engine/               generated Kotlin bindings + native library (cargo-ndk)
  platform/             thin modules: securestore, call, audio, camera, video, notify
ios/                    project.yml (XcodeGen, so the project is plain text), Mumcord/ (SwiftUI),
                        Platform/ (Keychain, CallKit, audio, camera, video), Bridge/ (generated Swift + xcframework)
design/                 theme tokens and fonts taken from the desktop app, generators for Compose and SwiftUI
i18n/                   script that turns the desktop translation files into Android and iOS resources
e2e/                    emulator and simulator tests, scripts that start the local test server
scripts/                setup-android.sh, build-android.sh, build-ios.sh, testenv.sh
.github/workflows/      ci-rust.yml, ci-android.yml, ci-ios.yml, e2e.yml, release.yml
docs/                   device checklist, screenshots (dark and light) per UI change
```

- App identifier on both platforms: `app.mumcord.mobile` (pending product manager confirmation; it can never change after the first release). Display name "Mumcord".
- Android: Kotlin, Jetpack Compose, minSdk 31 (Android 12), ABIs `arm64-v8a` for releases plus `x86_64` for debug/emulator. iOS: Swift, SwiftUI, deployment target 17.0.
- Android builds need JDK 21 (the installed Java 27 is too new for the Android Gradle plugin); task 1.9 installs it next to the existing one.

### Bridge API shape

- Crate `mumcord-mobile-bridge`, library name `mumcord_mobile`, crate types `cdylib`, `staticlib`, `lib`. UniFFI in proc-macro mode (no UDL file), exact UniFFI version pinned. Kotlin package `app.mumcord.mobile.bridge`, Swift module `MumcordBridge`.
- One object `MumcordEngine`. Constructor: `new(config: EngineConfig, storage: SecureStorage, listener: EngineListener)`. The bridge owns the tokio runtime and the engine.
- **Commands in:** methods on `MumcordEngine`, never blocking the caller's UI thread; request/response commands are `async`; failures are a `BridgeError` enum with a kind and a plain message.
- **State and events out:** `snapshot()` returns the full current state; `EngineListener.on_event(event)` delivers changes as one `BridgeEvent` enum. Callbacks run on a bridge thread, never on the audio thread. Kotlin wraps this in `StateFlow`, Swift in `@Observable` stores; UI code never calls the engine from a composable or view body.
- **Host interfaces** (implemented in Kotlin and Swift, handed to the bridge): `SecureStorage { set(account, bytes), get(account) -> bytes?, delete(account) }` (mirrors the engine's `SecretBackend`), later `AudioHost` (phase 3), `CameraHost` and `VideoSink` (phase 7). Real-time audio data never crosses UniFFI per frame through allocating calls; phase 3 defines the low-level path.
- Bridge types are own records and enums; engine types do not leak into the generated bindings. Mapping lives in `bridge/src/convert/`.
- No secrets, chat text or audio in logs (same rule as the engine).
- UniFFI errors use the field `detail`, never `message`, which Kotlin exceptions reserve.
- The Rust toolchain is pinned to 1.99.1 (the engine needs at least 1.91).

### Test server and verification

- Only the local Docker test server of the Mumcord repository (`testenv/up.sh`, `mumblevoip/mumble-server`, default port 64738), reached from the Android emulator as `10.0.2.2` and from the iOS simulator as `localhost`. Never a public server.
- The second client in automated tests is `mumbot` (as in the desktop E2E tests); manual checks use a desktop Mumcord.
- Every UI task follows the mockups on issue #228 and delivers dark and light screenshots into `docs/images/screenshots/` of the repository it changes.
- Every phase ends with a verification task run by a fresh agent that did not write the code. Each emulator check it adds stays in `e2e/` as an automated test.

## Phase 1 – Foundation

Goal: the engine builds for phones without changing desktop, the mobile repository exists with CI, and the Android emulator connects to the local test server and lists the channels.

| ID | Title | Status | Owner | Depends on | Complexity | Covers | Repo · owned files/areas |
|---|---|---|---|---|---|---|---|
| 1.1 | Store: public secure-storage interface, `keyring` only on desktop targets | done | b1-1-store | – | complex | FR-5.3, FR-5.60, m#2 | Mumcord · `crates/store/Cargo.toml`, `crates/store/src/secrets.rs`, `src/error.rs`, `src/lib.rs` |
| 1.2 | Audio: `mumcord-audio` and `mumcord-codec` compile for the phone targets (cpal options, thread priority, `rt.rs`), desktop unchanged | done | b1-2-audio | – | complex | m#2 | Mumcord · `crates/audio/**`, `crates/codec/**`, root `Cargo.toml` (only the `cpal` and `audio_thread_priority` lines) |
| 1.3 | Screenshare crate: one feature per codec library, builds without C libraries when all are off | done | b1-3-screenshare | – | complex | m#2 | Mumcord · `crates/screenshare/**` |
| 1.4 | Camera crate: confirm it builds without an OS backend, document and test the platform-fed frame interface | done | b1-4-camera | – | simple | FR-5.74, m#2 | Mumcord · `crates/camera/src/lib.rs`, `src/pool.rs`, `src/testcam.rs` |
| 1.5 | Engine features part 1: feature table in the manifest, `positional`, `camera-effects`, `mumble-import` | done | b1-5-engine-features | 1.1, 1.3 | complex | m#2 | Mumcord · `crates/engine/Cargo.toml`, `src/lib.rs`, `src/positional.rs`, `src/camera/**`, `src/mumble_import/**`, the session call sites of these modules |
| 1.6 | Engine features part 2: `screen-capture`, codec forwarding, remaining platform code (`files/motw.rs`, `files/receive.rs`, `sysinfo.rs`, `crash.rs`, `diag.rs`) gets phone fallbacks | done | b1-6-engine-capture | 1.5 | complex | m#2 | Mumcord · `crates/engine/src/screen_share/**`, `src/files/**`, `src/sysinfo.rs`, `src/crash.rs`, `src/diag.rs` |
| 1.7 | `p2p` and `protocol`: confirm they build for the phone targets, fix what does not (network interface lookup, port mapping) | done | b1-7-p2p | 1.9 | simple | m#2 | Mumcord · `crates/p2p/**`, `crates/protocol/**` |
| 1.8 | CI target check: job that checks the engine with `--no-default-features` on Linux and for Android, and for iOS on a macOS runner; feature combinations of desktop stay checked | done | b1-8-ci-targets | 1.6, 1.7 | complex | AC-5.36, m#2 | Mumcord · `.github/workflows/ci.yml` (new job only) |
| 1.9 | Local Android toolchain: Rust phone targets, JDK 21, Android SDK, NDK, emulator image, cargo-ndk; written down as a repeatable script | done | b1-9-android-tools | – | complex | m#1 | this machine · mumcord-mobile `scripts/setup-android.sh`, `docs/development.md` |
| 1.10 | Mobile repository skeleton: layout, Rust workspace, `engine.lock.toml`, CLAUDE.md, CHANGELOG, licence headers, ignore rules | done | b1-10-skeleton | – | simple | m#1 | mumcord-mobile · root files, empty directories of the layout |
| 1.11 | Bridge crate with UniFFI, first API: create engine, connect, certificate prompt answer, disconnect, snapshot with channels and people, events; host test against the local test server | done | b1-11-bridge | 1.6, 1.10 | complex | FR-5.4, m#1, m#3 | mumcord-mobile · `bridge/**`, root `Cargo.toml` |
| 1.12 | Android project: Gradle, Compose, native library build with cargo-ndk, generated Kotlin bindings, app starts on the emulator and shows the engine version | done | b1-12-android | 1.9, 1.11 | complex | m#1 | mumcord-mobile · `android/**` except `android/platform/securestore`, `scripts/build-android.sh` |
| 1.13 | Android secure storage (Keystore-protected, excluded from backups) behind `SecureStorage`, and system certificate checking start-up | done | b1-13-securestore | 1.12 | complex | FR-5.3, FR-5.60, m#2 | mumcord-mobile · `android/platform/securestore/**`, backup rules XML |
| 1.14 | iOS project: XcodeGen project, SwiftUI app, xcframework build of the bridge, generated Swift bindings, Keychain `SecureStorage` (this device only); app starts in the simulator in CI | done | b1-14-ios | 1.11 | complex | FR-5.60, m#1, m#2 | mumcord-mobile · `ios/**`, `scripts/build-ios.sh` |
| 1.15 | Mobile CI: Rust (format, lint, bridge tests), Android (build, unit tests, lint), iOS (build and unit tests on macOS, only when iOS or bridge files change) | done | b1-15a-ci-ios, b1-15b-ci-android | 1.11 (Rust), 1.12 (Android), 1.14 (iOS) | complex | m#1 | mumcord-mobile · `.github/workflows/ci-rust.yml`, `ci-android.yml`, `ci-ios.yml` |
| 1.16 | First slice on Android: plain screen to enter server and user name, connect, unknown-certificate prompt, channel tree with people, disconnect | done | b1-16-slice | 1.13 | complex | part of FR-5.4, FR-5.10, m#5, m#6 | mumcord-mobile · `android/app/**` (slice screens and view models) |
| 1.17 | Emulator test harness: script starts the local test server and the emulator, instrumented test connects and checks the channel names | done | b1-17-harness | 1.16 | complex | m#27 | mumcord-mobile · `e2e/**`, `scripts/testenv.sh` |
| V1.1 | Verification: desktop regression. Desktop build, tests, lints and the end-to-end run pass unchanged with the engine features in place | done | CI run on PR #824 | simple | AC-5.36 | Mumcord · no files (report only) |
| V1.2 | Verification: first slice in the running app on the emulator against the local test server, with a desktop client or bot visible in the channel tree | done | v1-2-qa | simple | part of AC-5.2 | mumcord-mobile · no files (report only) |

Can start in parallel right now: **1.1, 1.2, 1.3, 1.4** (four different crates, no shared files; 1.2 is the only one touching the root `Cargo.toml`), **1.9** and **1.10**.

## Phase 2 – App shell, servers, channels

| ID | Title | Status | Owner | Depends on | Complexity | Covers | Repo · owned files/areas |
|---|---|---|---|---|---|---|---|
| 2.1 | Bridge: full state and event model for servers, favourites, server ping, connection errors, channels, people, permissions, `mumble://` parsing, settings read/write | done | b2-1-bridge | complex | FR-5.2 … FR-5.7, FR-5.10 … FR-5.14, m#3 | mumcord-mobile · `bridge/**` |
| 2.2 | Design system Android: theme tokens, fonts, base components, dark and light | done | b2-2-design-android | complex | section 5, m#4 | mumcord-mobile · `design/**`, `android/app` theme package |
| 2.3 | Design system iOS | done | b2-3-design-ios | complex | section 5, m#4 | mumcord-mobile · `ios/Mumcord/Design/**` |
| 2.4 | Android: first-start setup, server list, add/edit, connecting, certificate warnings, error states, one-server rule, `mumble://` links | done | b2-4-android-servers | complex | FR-5.1 … FR-5.7, m#5 | mumcord-mobile · `android/app` (setup, servers) |
| 2.5 | Android: Channels tab, join, channel passwords, person sheet frame, search, hints for unsupported things, bottom tabs | done | b2-5-android-channels | complex | FR-5.10 … FR-5.14, m#6 | mumcord-mobile · `android/app` (channels) |
| 2.6 | iOS: same scope as 2.4 and 2.5 | done | b2-6-ios-screens | complex | FR-5.1 … FR-5.14, m#5, m#6 | mumcord-mobile · `ios/Mumcord/**` |
| 2.7 | Local network permission: explanation, system prompt, denied state, on Android and iOS | in progress | b2-7a-localnet-android (Android done); iOS half in b3-6-ios-voice | complex | FR-5.8, AC-5.37 | mumcord-mobile · `android/app` (connect flow), `ios/Mumcord/**` |
| 2.8 | Bridge: banned connection error | done | b2-1-bridge | simple | FR-5.7 | mumcord-mobile · `bridge/**` |
| V2 | Verification on emulator and simulator | open | | 2.5, 2.6, 2.7, 2.8 | simple | AC-5.1, AC-5.2, AC-5.3, AC-5.4, AC-5.29, AC-5.32, AC-5.37 | mumcord-mobile · `e2e/**` |

## Phase 3 – Voice

| ID | Title | Status | Owner | Depends on | Complexity | Covers | Repo · owned files/areas |
|---|---|---|---|---|---|---|---|
| 3.1 | Engine: platform-fed audio backend (`BackendKind::External`), system echo cancellation instead of the engine's own on phones, bitrate cap setting for mobile data | done | b3-1-engine-audio | complex | FR-5.25, FR-5.26, section 6, m#2 | Mumcord · `crates/audio/src/io/**`, engine voice settings |
| 3.2 | Bridge: voice commands and state (talk mode, push-to-talk, mute, deafen, talking, levels, per-person volume, local mute, whisper targets), `AudioHost` interface | done | b3-2-bridge-voice | complex | FR-5.20 … FR-5.26, m#3, m#7 | mumcord-mobile · `bridge/**` |
| 3.3 | Android audio module: capture and playback in voice-communication mode, microphone permission flow | done | b3-3-android-audio | complex | FR-5.25, section 6, m#7 | mumcord-mobile · `android/platform/audio/**` |
| 3.4 | Android: talk mode choice, voice bar, full voice view, hold-to-talk, person sheet volume and mute for me | done | b3-4-android-voice-ui | complex | FR-5.20 … FR-5.23, FR-5.12, m#7 | mumcord-mobile · `android/app` (voice) |
| 3.5 | Android: whisper and shout bar | done | b3-4-android-voice-ui | simple | FR-5.24, m#18 | mumcord-mobile · `android/app` (whisper) |
| 3.6 | iOS: audio module (voice-chat audio session) and the scope of 3.4 and 3.5 | in progress | b3-6-ios-voice | complex | FR-5.20 … FR-5.26, m#7, m#18 | mumcord-mobile · `ios/**` |
| V3 | Verification with injected audio and a bot as listener; flags what needs a manual listening check | open | | 3.5, 3.6 | complex | AC-5.5, AC-5.6, AC-5.7, AC-5.14, AC-5.15, AC-5.36 (re-run after 3.1) | both · `e2e/**` |

## Phase 4 – Background, call integration, routing, network, battery

| ID | Title | Status | Owner | Depends on | Complexity | Covers | Repo · owned files/areas |
|---|---|---|---|---|---|---|---|
| 4.1 | Android: foreground service and self-managed call, call-style notification with Mute, Deafen, Leave, no recent-calls entry, notice after the system ended the app | in progress | b4-1-android-call (done locally, being integrated) | complex | FR-5.30 … FR-5.32, FR-5.37, m#8 | mumcord-mobile · `android/platform/call/**` |
| 4.2 | Android: phone call handling (mute and deafen, restore), other audio interruptions | in progress | b4-1-android-call (done locally, being integrated) | complex | FR-5.33, FR-5.36, m#8 | mumcord-mobile · `android/platform/call/**`, `audio/**` |
| 4.3 | Android: audio output choice, first-connect question, output sheet, headset button with feedback sound | open | | 4.1 | complex | FR-5.34, FR-5.35, m#9 | mumcord-mobile · `android/platform/audio/**`, voice view |
| 4.4 | Bridge and Android: network change reconnect, mobile data switches, idle disconnect with warning notification, no periodic work when idle | open | | 4.1 | complex | FR-5.40 … FR-5.43, m#10 | mumcord-mobile · `bridge/**`, `android/app` |
| 4.5 | iOS: system call integration, audio session interruptions, routing, headset button, and the scope of 4.4 | open | | 4.4 | complex | FR-5.30 … FR-5.43, m#8, m#9, m#10 | mumcord-mobile · `ios/**` |
| V4 | Verification: emulator and simulator where possible (simulated incoming call, network switch, shortened idle timer); the rest goes on the device checklist | open | | 4.3, 4.5 | complex | AC-5.8, AC-5.9, AC-5.10, AC-5.11, AC-5.12, AC-5.13, AC-5.30 | mumcord-mobile · `e2e/**`, `docs/device-checklist.md` |

## Phase 5 – Chat, private messages, notifications

| ID | Title | Status | Owner | Depends on | Complexity | Covers | Repo · owned files/areas |
|---|---|---|---|---|---|---|---|
| 5.1 | Bridge: chat, history, images, GIF favourites, mentions, replies, private conversations, message requests, blocking | done | b5-1-bridge-chat | complex | FR-5.50 … FR-5.53, m#3, m#11, m#12 | mumcord-mobile · `bridge/**` |
| 5.2 | Android: Chat tab (sanitised rendering, links, inline images and GIFs, replies, mentions, sending text, images, GIF favourites) | in progress | b5-2-android-chat (done locally, being integrated) | complex | FR-5.50, FR-5.51, FR-5.53, m#11 | mumcord-mobile · `android/app` (chat) |
| 5.3 | Android: Messages tab, requests, blocking, notifications while connected, "Show sender only" | open | | 5.2 | complex | FR-5.52, FR-5.54 … FR-5.56, m#12 | mumcord-mobile · `android/app` (messages), `platform/notify/**` |
| 5.4 | iOS: scope of 5.2 and 5.3 | open | | 5.3 | complex | FR-5.50 … FR-5.56, m#11, m#12 | mumcord-mobile · `ios/**` |
| V5 | Verification | open | | 5.3, 5.4 | simple | AC-5.16, AC-5.17, AC-5.18, AC-5.19 | mumcord-mobile · `e2e/**` |

## Phase 6 – Identities and QR transfer

| ID | Title | Status | Owner | Depends on | Complexity | Covers | Repo · owned files/areas |
|---|---|---|---|---|---|---|---|
| 6.1 | Engine: transfer format (payload, compression, slow key derivation from the 8-digit code, encryption, split into numbered parts, 2-minute validity) with tests; used by desktop to create and by the bridge to read | done | b6-1-transfer-format | complex | FR-5.64 … FR-5.67, m#14, m#15 | Mumcord · new module in `crates/store` or `crates/engine` (decided in the task plan) |
| 6.2 | Desktop: "Send to your phone" dialog in the identity settings, per mockup, with dark and light screenshots and a CHANGELOG entry | open | | 6.1 | complex | FR-5.64, FR-5.67, m#14 | Mumcord · `src-tauri/src/commands/` (new command file), `ui/src/features/settings` identity area, `ui/src/locales` |
| 6.3 | Bridge and Android: identities (create, import `.p12`, export, delete rules, default) | open | | V2 | complex | FR-5.60 … FR-5.63, m#13 | mumcord-mobile · `bridge/**`, `android/app` (identities) |
| 6.4 | Android: QR scanner, progress, code entry with three attempts, preview and import of identities and server list | open | | 6.1, 6.3 | complex | FR-5.65, FR-5.66, m#15 | mumcord-mobile · `android/app` (transfer) |
| 6.5 | iOS: scope of 6.3 and 6.4 | open | | 6.4 | complex | FR-5.60 … FR-5.66, m#13, m#15 | mumcord-mobile · `ios/**` |
| V6 | Verification, including the desktop end-to-end run after 6.2 | open | | 6.2, 6.4, 6.5 | complex | AC-5.20, AC-5.21, AC-5.36 (re-run) | both · `e2e/**` |

## Phase 7 – Streams and camera

| ID | Title | Status | Owner | Depends on | Complexity | Covers | Repo · owned files/areas |
|---|---|---|---|---|---|---|---|
| 7.1 | Cross-built libvpx and dav1d for the Android and iOS targets, cached in CI; engine built with `vp9` and `av1-decode` | open | | V1.2 | complex | section 6 streams, m#16 | mumcord-mobile · `scripts/build-codecs-*.sh`, workflows |
| 7.2 | Bridge: stream state, start/stop watching, decoded frames to a `VideoSink`, stream volume, reachability; direct path and relay on phone networks | open | | 7.1 | complex | FR-5.70 … FR-5.73, m#16 | mumcord-mobile · `bridge/**`; Mumcord `crates/p2p` only if a phone fix is needed |
| 7.3 | Android: live tile, tap-to-watch on mobile data, full screen with rotation, floating mini window, system picture-in-picture, hardware decoding where the phone supports the codec | open | | 7.2 | complex | FR-5.70 … FR-5.73, FR-5.76, FR-5.41, m#16 | mumcord-mobile · `android/app` (streams), `platform/video/**` |
| 7.4 | Bridge and Android: camera sharing (preview sheet, front/back, 540p30 cap, VP9 hardware or software fallback, pause in background) | open | | 7.3 | complex | FR-5.74, FR-5.75, m#17 | mumcord-mobile · `bridge/**`, `android/platform/camera/**`, `android/app` (camera) |
| 7.5 | iOS: scope of 7.3 and 7.4 | open | | 7.4 | complex | FR-5.70 … FR-5.76, m#16, m#17 | mumcord-mobile · `ios/**` |
| V7 | Verification; early measurement of software AV1 decoding load for the open risk in spec section 13 | open | | 7.4, 7.5 | complex | AC-5.22, AC-5.23, AC-5.24 | mumcord-mobile · `e2e/**` |

## Phase 8 – Profile, themes, languages, moderation, activity, settings

| ID | Title | Status | Owner | Depends on | Complexity | Covers | Repo · owned files/areas |
|---|---|---|---|---|---|---|---|
| 8.1 | Translations: script from the desktop translation files to Android and iOS resources; mobile-only texts added to the desktop files | open | | V2 | complex | FR-5.82, m#21 | mumcord-mobile · `i18n/**`; Mumcord · `ui/src/locales` (new keys only) |
| 8.2 | Bridge: profile, themes and theme codes, moderation actions and permissions, activity log, remaining settings | open | | V5 | complex | FR-5.80, FR-5.81, FR-5.90 … FR-5.94, FR-5.100, m#3 | mumcord-mobile · `bridge/**` |
| 8.3 | Android: edit profile, appearance and theme gallery, language override | open | | 8.1, 8.2 | complex | FR-5.80 … FR-5.82, m#21 | mumcord-mobile · `android/app` (profile, themes) |
| 8.4 | Android: moderation section in the person sheet (move, server mute, kick, ban) | open | | 8.2 | simple | FR-5.90 … FR-5.93, m#19 | mumcord-mobile · `android/app` (moderation) |
| 8.5 | Android: More tab, activity log, settings screens | open | | 8.2 | complex | FR-5.94, FR-5.100, m#20, m#22 | mumcord-mobile · `android/app` (more, settings, activity) |
| 8.6 | iOS: scope of 8.3 to 8.5 | open | | 8.5 | complex | FR-5.80 … FR-5.100, m#19 … m#22 | mumcord-mobile · `ios/**` |
| V8 | Verification | open | | 8.3 … 8.6 | simple | AC-5.25, AC-5.26, AC-5.27, AC-5.28 | mumcord-mobile · `e2e/**` |

## Phase 9 – Tablet and accessibility

| ID | Title | Status | Owner | Depends on | Complexity | Covers | Repo · owned files/areas |
|---|---|---|---|---|---|---|---|
| 9.1 | Android: two-column tablet layout with navigation rail, stream in the right column | open | | V7, V8 | complex | section 5 tablet, FR-5.71, m#23 | mumcord-mobile · `android/app` (layout) |
| 9.2 | iOS: tablet layout | open | | 9.1 | complex | section 5 tablet, m#23 | mumcord-mobile · `ios/**` |
| 9.3 | Android: screen-reader labels, focus order, talking announcements, 200 % text, reduced motion, touch target sizes | open | | 9.1 | complex | section 5 accessibility, m#24 | mumcord-mobile · `android/app` |
| 9.4 | iOS: same accessibility scope | open | | 9.2 | complex | section 5 accessibility, m#24 | mumcord-mobile · `ios/**` |
| V9 | Verification (automated accessibility checks plus manual TalkBack / VoiceOver pass on the checklist) | open | | 9.3, 9.4 | simple | AC-5.31, AC-5.33 | mumcord-mobile · `e2e/**` |

## Phase 10 – Releases, end-to-end tests, device checklist, wrap-up

| ID | Title | Status | Owner | Depends on | Complexity | Covers | Repo · owned files/areas |
|---|---|---|---|---|---|---|---|
| 10.1 | Android release workflow: signed APK from CI secrets, checksum, SBOM, third-party notices, release notes naming the engine version | open | | V2 | complex | section 10, m#25 | mumcord-mobile · `.github/workflows/release.yml`, `RELEASING.md` |
| 10.2 | Android: daily update check with notice and switch; opt-in crash reports; About screen with engine version and licences | open | | 10.1, 8.5 | complex | FR-5.101, FR-5.103, FR-5.100, m#25 | mumcord-mobile · `bridge/**`, `android/app` |
| 10.3 | iOS release workflow: signed build on macOS runner, TestFlight upload, About screen | open | | V2 | complex | FR-5.102, section 10, m#26 | mumcord-mobile · `.github/workflows/release.yml` (iOS job), `ios/**` |
| 10.4 | End-to-end suite complete and running in CI on the Android emulator and the iOS simulator against the local test server with a bot | open | | V9 | complex | section 10, m#27 | mumcord-mobile · `e2e/**`, `.github/workflows/e2e.yml` |
| 10.5 | Manual device checklist document per release, including battery and data measurements of section 9 | open | | V4 | simple | section 9, section 10, m#28 | mumcord-mobile · `docs/device-checklist.md` |
| 10.6 | Roadmap and docs: roadmap table links the spec and marks Epic 5, README of both repositories | open | | 10.1, 10.3 | simple | section 14 | Mumcord · `specs/roadmap.md`, `README.md`; mumcord-mobile · `README.md` |
| V10 | Verification: upgrade install keeps data, update notice, tagged test release produces APK with checksum and a TestFlight build, no secret in the repository | open | | 10.2, 10.3, 10.4 | complex | AC-5.34, AC-5.35 | mumcord-mobile · report only |
| D1 | Implementation report `specs/05-mobile-app-implementation.md` (report template of `/dev`), result per AC-5.1 … AC-5.36, open manual checks | open | | all V tasks | simple | all AC | Mumcord · `specs/05-mobile-app-implementation.md` |

## Acceptance criteria coverage

| Verification task | Acceptance criteria |
|---|---|
| V1.1 | AC-5.36 |
| V1.2 | AC-5.2 (partly) |
| V2 | AC-5.1, AC-5.2, AC-5.3, AC-5.4, AC-5.29, AC-5.32, AC-5.37 |
| V3 | AC-5.5, AC-5.6, AC-5.7, AC-5.14, AC-5.15, AC-5.36 (re-run) |
| V4 | AC-5.8, AC-5.9, AC-5.10, AC-5.11, AC-5.12, AC-5.13, AC-5.30 |
| V5 | AC-5.16, AC-5.17, AC-5.18, AC-5.19 |
| V6 | AC-5.20, AC-5.21, AC-5.36 (re-run) |
| V7 | AC-5.22, AC-5.23, AC-5.24 |
| V8 | AC-5.25, AC-5.26, AC-5.27, AC-5.28 |
| V9 | AC-5.31, AC-5.33 |
| V10 | AC-5.34, AC-5.35 |

Criteria that need a real phone (AC-5.8 for 30 minutes, AC-5.10 with a real call, AC-5.11 and AC-5.12 with real headsets, the iOS half of every criterion beyond the simulator) are checked as far as an emulator allows and are then listed on the manual device checklist (10.5).

## Open questions for the product manager

1. Which engine version does the mobile repository pin while 0.9.0 is not released (tags only come from `main`)?
2. May the Android development tools be installed on this machine?
3. Is there a Mac or an iPhone for looking at the iOS app, or is CI the only check?
4. Is `app.mumcord.mobile` the permanent app identifier on both platforms?

## Decisions by the product manager (2026-11-13, after the analysis)

- Android development tools may be installed on the development machine.
- iOS is built and checked in CI only (simulator runs and screenshots) until TestFlight; it follows Android one step behind per phase.
- Application ID on both platforms: `app.mumcord.mobile`, shown name "Mumcord".
- Engine pin: during development an exact commit of the 0.9.0 milestone work; the first public app release pins a released desktop tag.
- One pull request in the Mumcord repository carries the specification and all engine and desktop work of this epic (branch `feat/mobile-engine`, base `milestone/0.9.0`).

## Product review (2026-11-14)

- The screens built without a mockup on Android (connect forms and sheets, permission sheets and banners, voice states, whisper bar, call notification, notices, chat states) were reviewed by the product manager and approved as built. Later tasks and the iOS counterparts keep that look.
- Transfer code length: 8 digits, shown as 4 + 4 (replaces 6 digits).
