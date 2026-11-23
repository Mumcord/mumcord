# Epic 5 – Android and iOS app · Implementation report

> Specification: `specs/05-mobile-app-specification.md` (with the decisions after review in section 12.1)
> Task list: `specs/05-mobile-app-tasks.md`
> As of: 2026-11-23 · Mode: team

## Run 1 – 2026-11-23

### Result in one sentence

The Android app is functionally complete and waits in the draft pull request
Mumcord/mumcord-mobile#29 for the first signed release; the iOS app is about 55 % built and
paused since 2026-11-15 until Android is finished.

### Where things stand

| Part | State |
|---|---|
| Engine preparation and desktop "Send to your phone" (Mumcord repository) | Done. Merged with #824 (0.9.0); the follow-ups the apps needed landed with #913 on `milestone/0.10.0`. |
| Android app (mumcord-mobile) | Functionally complete: every feature area of the specification is built and covered by automated tests in the emulator. Not yet released: the signing key does not exist yet, and nothing has been tried on a real phone. |
| iOS app (mumcord-mobile) | About 55 %: servers, connecting, voice, background voice as a system call, audio output, network changes, chat, private messages, identities and the QR transfer are built and tested in the simulator. Public servers, streams and camera, profile and themes, moderation, activity log, settings, tablet, accessibility and TestFlight are open. The list is in `docs/ios-backlog.md` of the mobile repository. |

### How it is built

**Two repositories.** The Mumcord repository owns the engine: the Rust code that speaks the
Mumble protocol, handles voice, chat, identities and streams. The new repository
[Mumcord/mumcord-mobile](https://github.com/Mumcord/mumcord-mobile) holds the two apps. It pulls
in the engine at one exact commit, written in `engine.lock.toml`; today that is the merge of
#913 on `milestone/0.10.0`. The first public app release should pin a released desktop version
instead.

**Engine preparation.** Desktop-only parts of the engine (screen capture, camera effects,
positional audio, the Mumble import, desktop codecs) became optional build features. Desktop
builds switch them all on and behave as before; the phone builds leave them off. Secure storage,
audio and camera frames got interfaces that the apps fill with the phone's own means. A CI job
in the Mumcord repository checks that the engine still builds for Android and iOS.

**One shared engine, two native apps.**

- *Bridge*: a small Rust crate (`mumcord-mobile-bridge`) wraps the engine in a phone-friendly
  interface and generates the Kotlin and Swift code for it with UniFFI. The apps send commands
  in; the bridge sends back the full state once and then every change as an event. No engine
  call ever runs on the screen's main thread.
- *Android*: Kotlin with Jetpack Compose, Android 12 and later. Thin platform modules for secure
  storage (a key in the phone's secure hardware, excluded from backups), the self-managed call
  and its notification, voice audio in the phone's voice-call mode with the system's echo
  cancellation, the camera (CameraX) and notifications.
- *iOS*: Swift with SwiftUI, iOS 17 and later, with Keychain storage (this device only), the
  system call integration (CallKit), an audio unit in the voice-chat audio session, and the same
  design and wording as Android.
- *Design*: the colours, fonts and icons come from the desktop app and are turned into Compose
  and SwiftUI code by generators, so both apps look like Mumcord in dark and light.
- *Translations*: a script turns the desktop translation files into Android resources; the texts
  only the phone app has live in `i18n/mobile/`.

**Voice path.** The phone's own audio system records and plays; the engine receives and hands
back the sound through lock-free buffers without allocating, so the real-time rules of the
project hold on phones too.

**Video path.**

- *Watching*: the engine receives a screen share or camera and decodes it in software, VP9 with
  libvpx and AV1 with dav1d (1.5.5, as on the desktop). Each decoded picture is kept in the
  bridge as a plain I420 image (brightness plane plus two colour planes). The bridge tells the
  app that a new picture is there, and the app fetches it through a small C function and draws
  it. The pictures never pass through the generated UniFFI code, which would copy them.
- *Sending the camera*: the app captures frames with CameraX and hands them to the engine
  through a C function that turns the picture upright, cuts out the middle 16:9 and scales it to
  at most 640 × 360 without waiting or allocating. The engine encodes it as VP9 in software.
- *Codecs*: libvpx and dav1d are cross-built for every phone target by
  `scripts/build-codecs.sh` from pinned, checksummed releases and cached in CI.

**Continuous integration (mobile repository).**

| Workflow | What it checks |
|---|---|
| `ci-rust.yml` | Format, lint and tests of the bridge |
| `ci-android.yml` | Android build, unit tests, lint, screenshots with automatic accessibility checks, and the job "Emulator tests": instrumented tests plus every end-to-end scenario against the local Mumble test server in Docker |
| `ci-ios.yml` | iOS build and four parallel simulator test groups on macOS runners, only when iOS or bridge files change |
| `release.yml` | Tagged release (see below); on pull requests that touch the release pipeline, a dry run with a throwaway key |

**Release flow (Android).** Pushing a version tag such as `v0.1.0` builds the APK (arm64), checks
that it is the release product, writes the SBOM, the third-party notices and the release notes
naming the engine, then signs it in a separate job that alone may read the signing key (GitHub
environment `android-release`, version tags only), writes `SHA256SUMS` and creates a **draft**
GitHub release. Nothing is public until the product manager publishes the draft. The APK is
about 41 MB. The full steps are in `RELEASING.md` of the mobile repository. iOS (TestFlight) is
not part of the workflow yet.

### Status per task

Percentages are for the task as a whole; a verification task counts its Android and iOS halves
equally.

| Task | Title (short) | Status | Done |
|---|---|---|---|
| 1.1 – 1.17 | Foundation: engine builds for phones, toolchain, repository, bridge, Android and iOS projects, secure storage, CI, first slice, emulator harness | done | 100 % |
| V1.1, V1.2 | Desktop regression, first slice on the emulator | done | 100 % |
| 2.1 – 2.8 | Bridge state, design systems, servers, channels, local network permission, ban error | done | 100 % |
| 2.9 | Android public server browser | done | 100 % |
| 2.10 | iOS public server browser | paused | 0 % |
| V2 | Verification servers and channels | Android done, iOS paused | 50 % |
| 3.1 – 3.6 | Voice: engine audio backend, bridge, Android and iOS audio and voice screens, whisper | done | 100 % |
| V3 | Verification voice | Android done, iOS paused | 50 % |
| 4.1 – 4.5 | Background voice, call integration, phone calls, audio output, network and idle disconnect (Android and iOS) | done | 100 % |
| V4 | Verification background and routing | Android done in the emulator; real-phone part on the device checklist; iOS paused | 50 % |
| 5.1 – 5.4 | Chat, private messages, notifications (Android and iOS) | done | 100 % |
| V5 | Verification chat | Android done, iOS paused | 50 % |
| 6.1 – 6.5 | Transfer format, desktop "Send to your phone", identities and QR scan (Android and iOS) | done | 100 % |
| V6 | Verification identities and transfer | Android done (the `.p12` import of AC-5.20 only instrumented), iOS paused | 50 % |
| 7.1 – 7.4 | Codecs, bridge streams, Android watching and camera sharing | done | 100 % |
| 7.5 | iOS streams and camera | paused (codecs already built for iOS) | 10 % |
| V7 | Verification streams and camera | Android in progress (AC-5.24 paused state), iOS paused | 40 % |
| 8.1 | Translations | done on Android; iOS part in 8.6 | 100 % (Android) |
| 8.2 – 8.5 | Bridge profile, themes, moderation, activity, settings; Android screens | done | 100 % |
| 8.6 | iOS profile, themes, moderation, More, activity, settings, translations | paused | 0 % |
| V8 | Verification profile, themes, moderation, activity | Android done, iOS paused | 50 % |
| 9.1, 9.3 | Android tablet layout and accessibility | done | 100 % |
| 9.2, 9.4 | iOS tablet layout and accessibility | paused | 0 % |
| V9 | Verification tablet and accessibility | Android automated part done (TalkBack pass manual), iOS paused | 50 % |
| 10.1 | Android release workflow | done | 100 % |
| 10.2 | Android update check, crash reports, About | done | 100 % |
| 10.3 | iOS release (TestFlight) and About | paused | 0 % |
| 10.4 | End-to-end suite in CI | Android done, iOS paused | 50 % |
| 10.5 | Manual device checklist | done | 100 % |
| 10.6 | Roadmap and docs | done | 100 % |
| V10 | Release check (upgrade install, update notice, tagged release) | open, needs the signing key | 0 % |
| D1 | This report | done | 100 % |
| E1, E2, E5 | Engine follow-ups (identities as bytes, wrong password error, the #913 list) | done in #913 | 100 % |
| E3, E6 – E12 | Deferred engine items (decision D-5) | deferred | – |
| E4 | Bridge mapping of the iOS local network error | open, waits with iOS | 0 % |
| E13 | Deep Ocean contrast fix in the desktop theme | open | 0 % |

By platform: Android about 95 % (open: the release check V10, the device checklist, the paused
camera state of AC-5.24 and the "Direct connections" setting, both being built now); iOS about
55 %.

### Acceptance criteria

How each criterion is proven on Android is listed test by test in `docs/acceptance.md` of the
mobile repository. In numbers:

| Kind of proof on Android | Criteria |
|---|---|
| End-to-end in the emulator against the local test server, some with instrumented tests for a part | 27 |
| Instrumented or unit tests only (AC-5.20, AC-5.32) | 2 |
| Automated as far as the emulator allows, the rest on the device checklist (AC-5.8, AC-5.10, AC-5.11, AC-5.12, AC-5.24, AC-5.33) | 6 |
| Checked elsewhere: release check V10 (AC-5.34, AC-5.35) and the desktop CI (AC-5.36) | 3 |
| **Total** | **38** |

All automated checks run in CI on every push to a pull request. On iOS, the built features have
simulator UI tests against a fake engine; there is no iOS end-to-end run against the test server
yet, so the iOS half of every criterion is open.

| AC | Android | iOS |
|---|---|---|
| AC-5.1 – AC-5.3 | Met, end-to-end | Built, simulator tests |
| AC-5.4 | Met; the password channel only instrumented (the stock test server has no password channel) | Built, simulator tests |
| AC-5.5 – AC-5.7 | Met, end-to-end | Built, simulator tests |
| AC-5.8 | Met for 3 minutes locked; the full 30 minutes on the device checklist | Built, simulator tests |
| AC-5.9 | Met, end-to-end | Built, simulator tests |
| AC-5.10 – AC-5.12 | Met in the emulator; real calls and real headsets on the device checklist | Built, simulator tests |
| AC-5.13 – AC-5.19 | Met, end-to-end | Built, simulator tests |
| AC-5.20 | Met in instrumented tests; an end-to-end import needs the system file picker | Built, simulator tests |
| AC-5.21 | Met, end-to-end | Built, simulator tests |
| AC-5.22, AC-5.23 | Met, end-to-end | Not built (paused) |
| AC-5.24 | Partly: preview, front and back, and no picture in the background are proven; "paused" on the desktop needs the engine signal from #913, which the app is adopting now | Not built (paused) |
| AC-5.25 – AC-5.28 | Met (kicks in the activity log only instrumented) | Not built (paused) |
| AC-5.29, AC-5.30 | Met, end-to-end | Built, simulator tests |
| AC-5.31 | Met, end-to-end at a tablet screen size | Not built (paused) |
| AC-5.32 | Met in unit tests; an end-to-end check needs a desktop file transfer from the test helpers | Built, simulator tests |
| AC-5.33 | Met automatically (accessibility checks on every screen, 200 % text); TalkBack pass on the device checklist | Not built (paused) |
| AC-5.34 | Update notice met; installing over an older signed APK waits for V10 | Not applicable |
| AC-5.35 | Workflow built and dry-run in CI; a real tagged release waits for the signing key | TestFlight paused |
| AC-5.36 | Met: desktop build and end-to-end run unchanged in the Mumcord CI | Not applicable |
| AC-5.37 | Met, end-to-end | Built, simulator tests |
| AC-5.38 | Met, end-to-end against a local list | Not built (paused) |

### Try it yourself

1. Start the local Mumble test server of the Mumcord repository (`testenv/up.sh`).
2. In the mobile repository, follow `docs/development.md`: set up the Android tools
   (`scripts/setup-android.sh`), build the app and start it in the emulator.
3. Add the server `10.0.2.2` (port 64738), accept its certificate, join a channel and talk with a
   desktop Mumcord connected to the same server.
4. `e2e/run.sh` runs every end-to-end scenario the way CI does.

### Decisions after review (2026-11-15 … 2026-11-23)

Recorded in section 12.1 of the specification:

- **D-1** There is no server-offered theme; the only per-server theme is the one the user picks
  in the edit-server form.
- **D-2** iOS is paused since 2026-11-15 until Android is finished; what is left is in the mobile
  repository's `docs/ios-backlog.md`. TestFlight waits with it.
- **D-3** On servers without a relay, watching a stream or camera may use a direct connection that
  shows the IP address to the other person. Accepted as designed; the phone gets the desktop's
  "Direct connections" setting with the same hint (being built now).
- **D-4** The engine follow-ups the apps need landed in #913.
- **D-5** Deferred engine items: themes crate, portrait camera, hardware video encoder hook,
  keeping the public server list and the activity log, SuperUser subchannel rights, control
  round-trip time, keeping a whisper target across a reconnect. The Deep Ocean theme needs a
  contrast fix in the desktop theme (4.40 instead of 4.5).
- **D-6** APK of about 41 MB; engine libraries uncompressed inside it (recommendation, final call
  pending).
- **D-7** Channel rows 48 dp instead of the mockup's 44 dp, for the touch target rule.
- **D-8** German is complete; other languages show mobile-only texts in English until the
  desktop translation pass.

### Deviations from the specification

- Camera sent at 640 × 360 in software instead of up to 540p30 (FR-5.74): the phone's hardware
  encoder cannot be used by the engine yet (deferred, D-5). This is the fallback the
  specification names in section 13.
- End-to-end tests run inside `ci-android.yml` instead of a separate `e2e.yml` (task 10.4), to
  avoid building the app and booting the emulator twice.
- The activity log has no "Moderation" filter yet, because the engine did not report who kicked
  or banned; #913 adds that.
- Everything in decisions D-1 … D-8.

### Open points and known issues

- **iOS (paused):** public servers (2.10), streams and camera (7.5), profile, themes,
  moderation, More, activity log, settings and translations with correct plural forms (8.6),
  tablet and accessibility (9.2, 9.4), TestFlight (10.3), the iOS end-to-end run (10.4) and the
  bridge mapping of the local network error (E4). Without a Mac every iOS round takes 20–40
  minutes of CI.
- **Nothing tried on a real phone yet:** real sound, echo cancellation, Bluetooth, real calls,
  battery (at most 6 % per hour), data use (below 40 MB per hour) and start time are measured
  with the device checklist.
- **Being built now on Android:** the paused camera state on the desktop (AC-5.24) and the
  "Direct connections" setting.
- **Engine pin:** the apps pin a commit of `milestone/0.10.0`; the first public release should
  pin a released desktop version.
- **Deep Ocean contrast** (E13) in the desktop theme.
- **Still open from earlier:** the mockup row "Scan an invite code" (not built, product manager
  decides), the licence question for distribution through Apple (before the public TestFlight
  link), and the local network address ranges on a real phone and router.

### What the product manager still has to do

1. **Create the Android signing key** exactly as in `RELEASING.md` of the mobile repository: the
   key, two offline backups, the GitHub environment `android-release` and its four secrets. The
   key is the app's permanent identity; losing it means every user has to reinstall.
2. **Decide** whether the engine libraries stay uncompressed in the APK (D-6).
3. **Review pull request Mumcord/mumcord-mobile#29**, merge it, then tag the first version
   (`v0.1.0`), check the draft release and publish it: that is the first APK.
4. **Run the device checklist** (`docs/device-checklist.md`) on real phones before publishing.
5. **Product review** of the Android screens built without a mockup since the review of
   2026-11-14 (dark and light screenshots in the mobile repository's `docs/images/screenshots/`),
   and of the two texts flagged in the iOS backlog (notification explanation, camera permission).
6. **Later, for iOS:** the Apple developer secrets for TestFlight (signing certificate,
   provisioning profile, App Store Connect API key) in a protected GitHub environment, the
   TestFlight tester group, and ideally access to a Mac to speed up the iOS work.

### Technical notes

- New repository: Mumcord/mumcord-mobile (AGPL-3.0). Toolchain: Rust 1.99.1, JDK 21, Android SDK
  and NDK, cargo-ndk; on macOS runners Xcode and XcodeGen.
- App identifier on both platforms: `app.mumcord.mobile` (can never change after the first
  release).
- Secrets (names only): `ANDROID_KEYSTORE_BASE64`, `ANDROID_KEYSTORE_PASSWORD`,
  `ANDROID_KEY_ALIAS`, `ANDROID_KEY_PASSWORD` in the environment `android-release`. None exist in
  any repository.
- Tested against the stock `mumblevoip/mumble-server` Docker image of `testenv/`, in the Android
  emulator (phone and tablet sizes) and the iOS simulator. No real devices yet.
