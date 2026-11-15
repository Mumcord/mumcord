# FreeBSD support · Implementation report

> Specification: `specs/13-freebsd-support-specification.md`
> As of: 2026-11-15 · Mode: native FreeBSD verification
>
> **Latest status (Run 4): GPU endurance failed.** Hardware acceleration is confirmed, but the current development build exhausts memory during sustained 1080p screen-share playback. AC-13.19 is not currently passed; earlier successful checks below are historical results.

## Run 1 – 2026-11-09

### Result in one sentence
Mumcord installs from a FreeBSD package, starts as a single instance, connects to a local Mumble server, uses the Jabra USB device for voice with real-time threads, shows what the platform cannot do, and renders on the GPU — the remaining acceptance criteria that need a second client, a second sound card or a second machine are not yet exercised.

### Acceptance criteria

| AC | Result | Evidence / note |
|---|---|---|
| AC-13.1 | ✅ | `pkg add` on 192.0.2.10: pkg triggers refresh icon and MIME caches; `pkg info -l` lists binary, 18 backgrounds, desktop entry, 4 icons, licence, ALSA config |
| AC-13.2 | ✅ | Starts from the installed binary, wizard completed on a fresh profile (VM, setup wizard screenshot); menu entry installed |
| AC-13.3 | ✅ | `mumcord "mumble://127.0.0.1:64738/"` → `connected server_id=1` |
| AC-13.4 | ✅ | With a Logitech PRO X headset on `pcm1` the box captures and plays on `/dev/dsp1` (`Input/Output audio stream started (fallback: false)`, both directions open at once, real-time threads). Both directions were heard: the second client hears the box and the box hears the second client. Earlier runs proved the packet path (1984 voice packets in, 123+ forwarded; 361 frames out with push-to-talk held) |
| AC-13.5 | ✅ | With a Logitech PRO X next to the Jabra dongle the engine lists `Sound card 0`/`Sound card 1` for both directions, and picking one in Settings moved capture to `/dev/dsp0` while playback stayed on `/dev/dsp1` (both open at once). The list was clipped by its card until the dropdown was portalled (fixed, with a test) |
| AC-13.6 | ✅ | With two cards (Logitech active, Jabra left): pulling the Logitech logs `chosen Output device unavailable …; using the default` and both streams reopen on the Jabra (`fallback=true`), connection kept, `/dev/dsp0` open. Replugging logs `chosen Output device is back; switched to it` and both streams return to `/dev/dsp1` with `fallback=false`. No notice, exactly as on Linux — the spec wording was corrected to say so |
| AC-13.7 | ✅ | Chat ✅ (three messages, one synced "from another user's history"), image ✅ (inline, over the direct connection), watch party ✅ (invite, join, playlist, sync, watcher list, and the video plays on the FreeBSD viewer with subtitles). File sharing ✅ after a relay was available: with a BastilleBSD jail running a stock `mumble-server`, a `mumpeer` relay and the `mumbot` peer bot (`scripts/freebsd-relay-jail`), `fileshare-test.txt` (61 B) went from one client to the other and the received file's SHA-256 matched the offer (`✓ Matches the offer`); a second run with both clients in `relay_only` sent `relay-test.txt` (81 B) through the relay (`session ended … transports=udp/udp bytes_ab=3149 bytes_ba=2192 dropped=0`) and the hash matched again |
| AC-13.8 | ✅ | Signed registry loaded over TLS ("Official repository · signed index checked"), dice-roller installed with its permission dialog, `/roll` produced the chat card ("rolled d20 = 7") |
| AC-13.9 | ✅ | Push-to-talk key F9 set in Settings → Voice; holding it opened the mic and transmitted (361 forwarded frames) |
| AC-13.10 | ✅ | KDE's StatusNotifierWatcher lists `…/org/ayatana/NotificationItem/tray_icon_tray_app_main`; no-host hint not exercised |
| AC-13.11 | ✅ | `mumcord_engine::p2p: peer-to-peer connection open` while the share ran; image and share both travelled over it |
| AC-13.12 | ✅ | Whole X11 screen captured (`1080p · 30 fps · AV1-CPU`, thumbnail shows the real screen), the second client watched live for 10 minutes, the start dialog stated "On FreeBSD a share is the whole X11 screen and carries no sound" |
| AC-13.13 | ✅ | Apps menu: "Turn on camera" greyed with "Camera sharing is not available on FreeBSD yet"; engine reports `CameraUnavailable::UnsupportedPlatform` |
| AC-13.14 | ✅ | Settings → About lists "Not available on FreeBSD" with all five gaps and the `drm-66-kmod` line; component test `AboutSection.test.tsx` |
| AC-13.15 | ✅ | Second launch with a link exits 0 and the running window receives it (`link received outcome=Shown`) |
| AC-13.16 | ✅ | About → Updates: "installed from updates.install.pkg", "This copy cannot update itself, so Mumcord only tells you about new versions" |
| AC-13.17 | ✅ | `pkg delete -y mumcord`: binary, desktop entry and all icons removed, desktop-file and icon-cache triggers run again, the profile stays |
| AC-13.18 | ✅ | `cargo test --workspace` on FreeBSD 15.1 amd64, 117 test targets green; UI suite 306 files with `--maxWorkers=4` |
| AC-13.19 | ✅ | `glxinfo -B` → `Mesa Intel(R) UHD Graphics (CML GT2)` (hardware), no `GPU HANG` in `dmesg`, a 10-minute share stayed at ~90 % of one core without hangs |
| AC-13.20 | ✅ | `pkg add` prints the install message; `pkg info -D mumcord` shows it |

### Try it yourself
1. Local test server: `mumble-server -ini /tmp/murmur/murmur.ini` (port 64738, database in `/tmp/murmur`, no registration, no SuperUser password in the repo).
2. Install the package: `sudo pkg add ./mumcord-0.7.2.pkg` (read the install message about `drm-66-kmod` and the realtime group).
3. Start with a link: `mumcord "mumble://127.0.0.1:64738/"` — the window shows the server, one channel, the user; voice devices are in Settings → Voice.
4. Settings → About shows the platform line and what FreeBSD cannot do.

### Completed tasks
| Task | Title | Owner |
|---|---|---|
| T1 | Single-instance lock and argument handover | lead |
| T2 | One device entry per `/dev/dsp` unit | lead |
| T3 | Real-time priority for the audio threads | lead |
| T4 | About note and platform line | lead |
| T5 | Features that cannot work are disabled with a reason | lead |

### Deviations from the specification
- T2: the per-unit entries come from an ALSA configuration the package installs (as specified), and Mumcord hides the units the machine does not have. Switching between two real cards is unverified.
- The About note names the gaps in one list instead of three separate lines (FR-13.28 wording "three lines"); the content is the same.
- Translations of the new About/camera/share texts are English in every locale for now.
- **Missing decoders are named** (FR-13.34 – FR-13.38, AC-13.21 – AC-13.23): with
  `gstreamer1-libav` removed, Settings → Diagnostics showed
  `sudo pkg install gstreamer1-libav` and the player showed the same line under the picture
  ("… Install it and restart Mumcord"). Installing it while the app runs does nothing — the
  WebView scans its decoders only at process start — after a restart the same video played.
- **Watch party**: party mechanics and playback work, but only with the GStreamer decoders
  WebKitGTK loads at runtime; without them YouTube reports "this video cannot be played in your
  browser" (its HTML5 error 5) and the frame stays black. Two limits stay: DRM-protected videos
  cannot play (this WebKitGTK build has no EME at all — `requestMediaKeySystemAccess` is
  undefined) and streams YouTube serves only as AV1/Opus cannot decode (no AV1 decoder, no Opus
  in MSE; `av1_mse`/`opus_mse` are false). Both are in spec §2.2 now.
- The package gains three runtime dependencies that `ldd` cannot see, because WebKitGTK loads
  its decoders through GStreamer: `gstreamer1-libav`, `gstreamer1-plugins-vpx`,
  `gstreamer1-plugins-opus` (`scripts/build-freebsd-pkg.sh`). Without them even H.264 does not
  decode.

### Tasks and the pipeline

| Task | State |
|---|---|
| T6 Diagnostics | done: release (`kern.osrelease`), session (X11/Wayland), audio devices and the crash-symbol note, seen on the FreeBSD machine |
| T7 Release wiring | done: FreeBSD build job in `build-and-smoke.yml`, `.pkg` in `SHA256SUMS`, release notes, changelog and the SBOM row (`freebsd-pkg-x86_64`) |
| T8 CI | done: `ci.yml` job `FreeBSD 15.1`; its commands (`cargo check`, `cargo clippy -- -D warnings`, `cargo test --workspace`) were run by hand on the FreeBSD machine and pass (clippy found one `needless_return` in `rt.rs`, fixed) |

The FreeBSD workflow jobs themselves (`vmactions/freebsd-vm`) have not run yet: they need a push
to the repository.

### Open points & known issues
- AC-13.4 audibility and AC-13.5 device switching need hardware the test machine lacks: a
  microphone/headset on the dongle and a second sound card.
- File sharing needs a relay in front of the Mumble server. The test rig for it is
  `scripts/freebsd-relay-jail` (BastilleBSD template: stock `mumble-server` + `mumpeer` +
  `mumbot` peer bot in one jail); it is test tooling, not shipped with the app.
- Watch party video playback is a platform gap (see deviations).
- T6 (Diagnostics lines, FR-13.30/31), T7 (release wiring) and T8 (CI in a FreeBSD VM) are not implemented yet.
- Screenshots: the test machine returns black pixels for every capture tool, so UI screenshots have to come from the VM.
- KWallet on a fresh KDE profile accepts secrets and drops them; the app treats it as unusable and uses the encrypted file (documented behaviour, `e6e15205`).

### Technical notes
- New/changed: `src-tauri/src/single_instance.rs` (Unix socket handover), `crates/audio/src/rt.rs` (rtprio), `packaging/freebsd/alsa-mumcord.conf` + `pkg-message`, `crates/audio/src/io/devices.rs`, `AppInfo.limits` + `PlatformLimit`, `CameraUnavailable::UnsupportedPlatform`, `SoundUnavailableReason::Platform`.
- Build: `scripts/build-freebsd-pkg.sh` builds with `--features custom-protocol` (a plain release build is a dev build), `CARGO_BUILD_JOBS=4` is stable on 8 GB.
- Machine setup used: FreeBSD 15.1, `drm-66-kmod` (6.12 hangs this GPU), `mac_priority` + group `realtime` (gid 47), KDE Plasma 6 on X11, Jabra Link 380.
- No new environment variables or secrets; the profile layout is unchanged.
- Tested server: `mumble-server` 1.5.939 from the FreeBSD package collection; for file sharing
  additionally `mumpeer` 0.2.1 and `mumbot` 0.5.3, both built from source inside a BastilleBSD
  jail (`scripts/freebsd-relay-jail`).
- FreeBSD quirks found while building the relay rig: a shared-IP jail has no IPv6 and no
  loopback of its own (so both services bind IPv4 addresses explicitly and the relay's allowlist
  API runs on the jail address with HTTPS + certificate pinning), and `mumbot`'s
  `mumcord-protocol` dependency needs a system `protoc` (a `[patch.crates-io]` path patch).

## Run 2 – 2026-11-15: Native rebuild, desktop E2E and performance checks

### Environment and isolation

- FreeBSD 15.1-RELEASE amd64, 8 logical CPUs and 8 GB RAM; Rust 1.99.1, Node 22.23.4 and npm 12.0.2.
- Sources copied from `b56cbc44` plus this branch's fixes into `~/mumcord-freebsd-e2e`, without changing the existing source checkout. The build reused the existing Cargo artifact cache.
- The installed `/usr/local/bin/mumcord` and the desktop user's profile were not replaced. Native desktop tests used their own Xvfb display, D-Bus session, two disposable profiles and an unmodified `mumble-server` bound to loopback on a temporary port. No public or third-party Mumble server was contacted.
- The native release-profile binary embeds the current UI (`custom-protocol`); its display version is `dev~20261115`. This is a development verification build, not a tagged release.

### Verified fixes

1. **Positional plugins repeatedly enumerated threads as programs.** A new FreeBSD regression test failed against the original implementation because several entries had the same process ID. `KERN_PROC_PROC` now replaces `KERN_PROC_ALL` inside the FreeBSD platform module. A native probe returned 944 rows (324 with positive process IDs) with the old query and 83 rows (82 with positive IDs) with the new query: approximately 75% fewer command-line queries in that sample, as well as fewer allocations and duplicate plugin candidates. This is a reduction in enumeration work, not a claim about whole-app CPU use. All 15 positional unit tests pass on FreeBSD; all 14 applicable tests also pass on Linux.
2. **A clean package install did not require the sound backend.** The original generated manifest omitted `alsa-plugins`, even though it owns the OSS PCM plugin and `alsa-lib` has no dependency on it. The package now declares it explicitly, alongside the dynamically loaded GStreamer decoders; the package smoke check also requires it. No installed dependency was removed to reproduce this: the missing declaration was verified in the generated package metadata.
3. **A relative package output directory lost the generated package.** `pkg create` ran inside the temporary staging tree, so a relative output was written there and removed during cleanup. Native packaging with `--no-build dist-freebsd` reproduced `pkg create produced no package`. Resolving the output before changing directory fixes it. Regression tests cover absolute output, relative output with spaces, declared runtime backends and refusal when the OSS plugin is unavailable. They fail against the original builder, pass on FreeBSD and Linux, and are added only to the FreeBSD CI job.

### Checks and results

| Check | Result |
|---|---|
| `npm ci --no-audit --no-fund` and `npm run build` | Passed; includes TypeScript checking and the production UI build |
| `CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 CARGO_PROFILE_RELEASE_DEBUG=0 cargo build --release --locked -p mumcord --features custom-protocol` | Passed; native rebuild took 17 minutes 24 seconds |
| `cargo test --workspace --locked` | Passed: 2,806 tests, 24 ignored, 118 reported test targets including doc tests |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | Passed |
| `cargo fmt --all --check` | Passed on FreeBSD and Linux |
| `npm test -- --maxWorkers=1` | Passed: 324 files, 3,567 tests |
| `NODE_OPTIONS=--max-old-space-size=4096 npm run lint` | Passed, with one pre-existing `react-refresh/only-export-components` warning in `UpdateCheckPrompt.tsx`; Prettier passed |
| `sh scripts/ci/test-freebsd-pkg.sh` | Three regression cases passed on FreeBSD and Linux |
| `bash scripts/ci/test-smoke.sh` | All nine existing helper cases passed on Linux |
| `sh scripts/build-freebsd-pkg.sh --no-build dist-freebsd` | Passed: package remains at `~/mumcord-freebsd-e2e/dist-freebsd/mumcord-0.8.0.pkg`, approximately 37 MB; 18 backgrounds, four icons and the explicit OSS/decoder dependencies verified |

The first full ESLint attempt exhausted Node's default heap. The successful rerun increased the heap for that command only; no shared build settings or other platform defaults were changed. The release build also emitted the existing unused `TEST_URL_ENV` warning and the expected unavailable ONNX Runtime warning on FreeBSD.

### Native desktop E2E

Run from the checkout on FreeBSD:

```sh
node e2e/freebsd/smoke.mjs target/release/mumcord
```

The standalone runner requires `npm ci`, `Xvfb`, `WebKitWebDriver-4.1`, `dbus` and `mumble-server`; it needs neither Docker nor root. Linux's existing E2E runner and configuration are unchanged. Xvfb was installed only for this verification and removed afterwards, so it must be installed again to repeat the native desktop tests. The task's small dedicated Rust probe cache was also removed; the user's pre-existing shared Cargo cache was retained.

Verified through the real native app:

- Two fresh-profile setup wizards, new identities, saved local servers, certificate trust and connection.
- Chat delivered in both directions between the two clients.
- Camera unavailable with `unsupportedPlatform`; screen-share sound unavailable with `platform`.
- Update check reports package ownership, `canInstall: false` and no check timestamp.
- Whole-screen X11 capture, live decoded pictures over a direct connection, and no share sound. The final run reported 1920×1080 at 24 fps after ten seconds of playback; an earlier successful run reported 18 fps while the full build/test workload was active. These headless measurements are not a guarantee of 1080p30 or a hardware-rendering benchmark.
- Pause and resume reach the viewer; stopping removes the share.
- Settings → About shows the FreeBSD limitations.
- A second launch with a local `mumble://` link exits successfully, the running instance logs receipt, and exactly one connection remains.
- The existing scaled-colour X11 capture integration test now runs on FreeBSD too and passed against Xvfb.

Screenshots and redacted client logs are in `e2e/artifacts/freebsd/` in the native checkout. Profiles, identities, vault files and keys are deleted after each run and are never copied to artifacts.

### Not reverified in this run

- Audible voice in both directions, physical push-to-talk keys, unplug/replug recovery and switching the two real sound cards. Both USB sound devices and realtime-group permissions were present, but the E2E clients mute their microphones after joining.
- A clean-system package install/uninstall: the package was built and inspected without replacing the user's installed app.
- Watch-party video/decoder removal, registry plugin installation, relay-backed file sharing, a second physical host and a ten-minute hardware-GPU stability run. Xvfb screenshots do not establish hardware acceleration.
- Windows/macOS builds or hosted CI. No push or pull request was made; only FreeBSD runtime code, FreeBSD packaging/CI, tests and this report changed.

## Run 3 – 2026-11-15, 13:36–13:51 UTC: Fresh verification after reboot

Revalidated source revision `8a377d68` on the same FreeBSD 15.1-RELEASE amd64 host after its reboot. No new application regression was found and no runtime changes were needed. The app shell was explicitly recompiled with the current embedded UI and `MUMCORD_VERSION=dev~8a377d68`; dependency build artifacts were reused. All Mumble connections again used disposable, loopback-only test servers and profiles.

| Check | Fresh result |
|---|---|
| `npm ci` and `npm run build` | Passed; lockfile dependencies restored, TypeScript and production UI build passed |
| Native release rebuild, `custom-protocol`, two build jobs | Passed in 5 minutes 48 seconds |
| `cargo test --workspace --locked` | Passed: 2,806 tests, 24 ignored, 118 reported targets including doc tests |
| FreeBSD X11 scaled-colour capture integration test | Passed against Xvfb, not skipped |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | Passed |
| `cargo fmt --all --check` | Passed on FreeBSD and Linux |
| `npm test -- --maxWorkers=2` | Passed: 324 files, 3,567 tests, 306 seconds |
| `NODE_OPTIONS=--max-old-space-size=4096 npm run lint` | Passed; the same existing Fast Refresh warning in `UpdateCheckPrompt.tsx`, no errors; Prettier passed |
| Package regression helper and existing smoke-helper tests | Three package cases and nine smoke-helper cases passed on both FreeBSD and Linux |
| Native package build using relative output `dist-freebsd-redo` | Passed with `pkg` 2.9.2; approximately 37 MB, 18 backgrounds, four icons, and explicit `alsa-plugins`/libav/VPX/Opus runtime dependencies verified |
| Native two-client E2E, two consecutive fresh runs | Both passed: setup, trust, two-way chat, platform/update limits, direct live screen playback, pause/resume/stop, About, and single-instance link handoff |

Both E2E runs reported 1920×1080 at 29 encoder-reported fps after the ten-second playback hold, with positive viewer frame reports over the direct connection. These were headless Xvfb measurements after the build/test workload finished, not a hardware-GPU benchmark or evidence of a new code-level speedup.

The process-enumeration improvement remains verified: a fresh kernel probe returned 978 rows, 357 positive-PID rows and 895 duplicate-PID rows with `KERN_PROC_ALL`, versus 83 rows, 82 positive-PID rows and no duplicate PIDs with `KERN_PROC_PROC`. Approximately 77% fewer command-line queries are needed in this sample; this is not a whole-app CPU measurement. The positional regression tests passed again in the full native suite.

A read-only check of the real desktop before and after testing reported `Mesa Intel(R) UHD Graphics (CML GT2)`, Mesa 26.2.2, `Accelerated: yes`, with `drm-66-kmod` loaded. No matching GPU hang/reset/timeout markers were found in `dmesg` at either check. This verifies driver availability, not a ten-minute hardware-rendered Mumcord session. Audible voice, physical hotplug/PTT, clean-system install/uninstall and the other manual/second-host checks excluded in Run 2 remain unverified in this run.

The native package is at `~/mumcord-freebsd-e2e/dist-freebsd-redo/mumcord-0.8.0.pkg`; it contains the development build identified above, not a tagged release. Current screenshots and redacted client logs are under `e2e/artifacts/freebsd/`; the previous run's native artifacts were retained under `e2e/artifacts/freebsd-run2/`. Disposable profiles, identities, vault keys and server databases were removed, and no test client or Xvfb process remained.

Installing the temporary Xvfb prerequisite automatically updated FreeBSD's `pkg` tool from 2.8.4 to 2.9.2; no general system upgrade was run. Xvfb 21.1.26 was removed after verification. The installed Mumcord package remains 0.7.2 and the normal profile was not used or replaced. No new dedicated Rust build cache was created. Only this report changed in the repository; nothing was pushed and no pull request was created.

## Run 4 – 2026-11-15: Real-desktop GPU checks and failed endurance

**Result: hardware rendering verified; the ten-minute acceptance session failed because playback exhausted memory.** This is not a successful GPU endurance result, even though no kernel GPU-hang/reset markers were recorded. The current development build must not inherit the historical AC-13.19 pass from Run 1.

### Method and hardware evidence

The binary was the Run 3 release build, `dev~8a377d68`, on the same FreeBSD 15.1 amd64 machine with eight logical CPUs and 8 GB RAM. Tests used the real 1920×1080 X11 desktop, two disposable profiles, a private session bus and an unmodified loopback-only Mumble server. During the playback checks both clients were in voice with microphones muted; native capture and playback streams opened without fallback. Audible voice was not tested.

- `glxinfo -B`, before and after: `Accelerated: yes`, `Mesa Intel(R) UHD Graphics (CML GT2)`, Mesa 26.2.2; `drm-66-kmod` 6.6.25.1501000_11 loaded, render-device PCI ID `8086:9b41`.
- Both clients opened `/dev/drm/128` in short two-window GPU probes. The later normal-viewer run independently confirmed its WebKit process held that render node and mapped `libgallium-26.2.2.so` and `libEGL_mesa.so.0.0.0`, without injecting a WebGL context or forcing software rendering.
- Browser-reported WebGL renderer names were not treated as hardware proof: WebKit exposed the generic string `Apple GPU` on this Intel machine. Early probes also encountered animation-timing and GTK-window-coordinate assumptions; those were corrected rather than counted as driver failures.
- To avoid recursively capturing the receiving window, the sustained test placed the publisher on private Xvfb and the normal, unmodified receiver on the real GPU desktop. A small changing colour pattern on the private source was detected in the physically presented receiver window using X11. Only colour statistics were retained, not screenshots or desktop images.

### Endurance failure and bounded reproduction

An initial instrumented run timed out on a UI-control request after roughly one minute. Repeating without the injected WebGL animation or full-canvas readbacks reproduced the failure, so it cannot be dismissed as only an intrusive-probe issue.

In the normal-viewer attempt starting at 14:59:22 UTC, 1920×1080 capture and direct playback initially worked. At approximately one minute of playback, WebDriver lost the receiving page. The kernel reported `swap_pager: out of swap space` and killed the receiving `WebKitWebProcess`, the publishing `mumcord` process and its `WebKitWebProcess` with `failed to reclaim memory`. Those PIDs belonged to the disposable test run, not the user's desktop applications. No GPU HANG, GPU reset or preemption-timeout marker was found before or after the tests. The workload did not reach ten minutes, so **AC-13.19 failed**; sustained 1080p receiving is also not accepted on this revision.

A final bounded reproduction starting at 15:04:38 UTC sampled only this run's descendant processes once per second. It stopped both test apps when summed process RSS crossed 3 GiB, avoiding another host OOM:

| Seconds after launch | Receiving WebKit process RSS | Publishing WebKit process RSS | Summed test-process RSS |
|---|---:|---:|---:|
| 10.0 | 358.4 MiB | 382.5 MiB | 1,973.4 MiB |
| 15.1 | 450.7 MiB | 392.2 MiB | 2,248.0 MiB |
| 20.1 | 930.1 MiB | 397.2 MiB | 2,872.9 MiB |
| 22.0, safety stop | 1,258.8 MiB | 397.4 MiB | 3,202.7 MiB |

Summed RSS includes shared mappings and is not unique physical-memory accounting. The rapid growth of the individual receiving WebKit process is nevertheless clear. The precise allocation/queue responsible is not yet proven; the next step is to isolate the receiving frame-delivery/rendering memory growth, add a regression check and fix it before rerunning endurance. Forcing software rendering or silently lowering quality would not establish that the normal hardware-rendered path is fixed.

### Evidence and cleanup

Run output, redacted client logs, native-device evidence and colour statistics are retained in ignored `e2e/artifacts/freebsd-gpu/`; the guarded memory samples are in ignored `e2e/artifacts/freebsd-gpu-memory/memory.jsonl`. The final normal-viewer log is `mumcord-gpu-check.log`. Ad-hoc probe sources are retained alongside the artifacts for diagnosis, not added to the application or CI.

All disposable profiles, identities, server databases and test clients were removed. The temporary Xvfb package was removed again. Xorg and Plasma remained running; the installed Mumcord package is still 0.7.2, and the normal Mumcord profile was never used or replaced. No application runtime code or other operating system's behaviour changed. No new Rust build cache was created, nothing was pushed and no pull request was created.
