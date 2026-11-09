# FreeBSD support · Implementation report

> Specification: `specs/13-freebsd-support-specification.md`
> As of: 2026-11-09 · Mode: single session

## Run 1 – 2026-11-09

### Result in one sentence
Mumcord installs from a FreeBSD package, starts as a single instance, connects to a local Mumble server, uses the Jabra USB device for voice with real-time threads, shows what the platform cannot do, and renders on the GPU — the remaining acceptance criteria that need a second client, a second sound card or a second machine are not yet exercised.

### Acceptance criteria

| AC | Result | Evidence / note |
|---|---|---|
| AC-13.1 | ✅ | `pkg add` on 192.0.2.10: pkg triggers refresh icon and MIME caches; `pkg info -l` lists binary, 18 backgrounds, desktop entry, 4 icons, licence, ALSA config |
| AC-13.2 | ✅ | Starts from the installed binary, wizard completed on a fresh profile (VM, setup wizard screenshot); menu entry installed |
| AC-13.3 | ✅ | `mumcord "mumble://127.0.0.1:64738/"` → `connected server_id=1` |
| AC-13.4 | ⚠️ | Receive: 1984 voice packets from the second client reached the server, 123+ (~330 B) were forwarded to the box's client over loopback, output stream open at real-time priority (`real-time scheduling on thread=playback`). Send: with F9 held the server forwarded 361 voice frames to the second client. Audibility itself could not be judged: the machine has only a Jabra Link 380 dongle without headset, so its capture device delivers no audio |
| AC-13.5 | ⚠️ | Per-unit PCMs installed (`aplay -L` lists 8 "Sound card N (OSS)"), filter unit test; switching between two real cards not exercised (the machine has one `/dev/dsp`) |
| AC-13.6 | ⚠️ | Pulling the dongle mid-call: `Input/Output audio stream failed; reopening` → `no audio device available … may have been disconnected`, connection kept, no crash; replugging reopened both streams by itself. No "device changed" notice and no fallback, because with a single device there is nothing to fall back to (same code path as Linux) |
| AC-13.7 | ⚠️ | Chat ✅ (three messages, one synced "from another user's history"), image ✅ (inline, over the direct connection), watch party ✅ (invite, join, playlist, sync, watcher list, and the video plays on the FreeBSD viewer with subtitles). File sharing impossible on this server: files need the relay, and the test server has none, so not exercised |
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
- AC-13.7 file sharing needs a relay; the local test server has none, so files stay untested.
- Watch party video playback is a platform gap (see deviations).
- T6 (Diagnostics lines, FR-13.30/31), T7 (release wiring) and T8 (CI in a FreeBSD VM) are not implemented yet.
- Screenshots: the test machine returns black pixels for every capture tool, so UI screenshots have to come from the VM.
- KWallet on a fresh KDE profile accepts secrets and drops them; the app treats it as unusable and uses the encrypted file (documented behaviour, `e6e15205`).

### Technical notes
- New/changed: `src-tauri/src/single_instance.rs` (Unix socket handover), `crates/audio/src/rt.rs` (rtprio), `packaging/freebsd/alsa-mumcord.conf` + `pkg-message`, `crates/audio/src/io/devices.rs`, `AppInfo.limits` + `PlatformLimit`, `CameraUnavailable::UnsupportedPlatform`, `SoundUnavailableReason::Platform`.
- Build: `scripts/build-freebsd-pkg.sh` builds with `--features custom-protocol` (a plain release build is a dev build), `CARGO_BUILD_JOBS=4` is stable on 8 GB.
- Machine setup used: FreeBSD 15.1, `drm-66-kmod` (6.12 hangs this GPU), `mac_priority` + group `realtime` (gid 47), KDE Plasma 6 on X11, Jabra Link 380.
- No new environment variables or secrets; the profile layout is unchanged.
- Tested server: `mumble-server` 1.5.939 from the FreeBSD package collection.
