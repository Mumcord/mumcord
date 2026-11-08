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
| AC-13.3 | ✅ | `mumcord "mumble://127.0.0.1:64738/"` → `connected server_id=1`; murmur log: `Authenticated` for SmokeTester |
| AC-13.4 | 🎧 | Jabra `pcm0` opens both directions (`Input`/`Output audio stream started (fallback: false)`, threads at `real:10`); listening check with a second client pending |
| AC-13.5 | ⚠️ | Per-unit PCMs installed (`aplay -L` lists 8 "Sound card N (OSS)"), filter unit test; switching between two real cards not exercised (the machine has one `/dev/dsp`) |
| AC-13.6 | ❌ | Unplugging the USB device mid-call not tested (needs hands on the machine) |
| AC-13.7 | ❌ | Chat, file sharing and watch party need a second client |
| AC-13.8 | ❌ | Plugin install/chat card not tested |
| AC-13.9 | ❌ | PTT key and mouse side button not tested (needs interaction) |
| AC-13.10 | ✅ | KDE's StatusNotifierWatcher lists `…/org/ayatana/NotificationItem/tray_icon_tray_app_main`; no-host hint not exercised |
| AC-13.11 | ❌ | Direct connection to a second client not tested |
| AC-13.12 | ❌ | Screen share with a viewer not tested |
| AC-13.13 | ✅ | FreeBSD reports `CameraUnavailable::UnsupportedPlatform`, the entry stays visible and disabled with the note; engine + UI tests |
| AC-13.14 | ✅ | `AppInfo.limits` → About lists the gaps and the driver line; component test `AboutSection.test.tsx`; visual check pending (no screenshots on the test machine) |
| AC-13.15 | ✅ | Second launch with a link exits 0 and the running window receives it (`link received outcome=Shown`) |
| AC-13.16 | ✅ | `updater_freebsd.rs` reports `InstallKind::Pkg`, no download offered; the one-time question is skipped |
| AC-13.17 | ❌ | `pkg delete` not run yet |
| AC-13.18 | ✅ | `cargo test --workspace` on FreeBSD 15.1 amd64, 117 test targets green; Linux/Windows/macOS CI untouched |
| AC-13.19 | ✅ | `glxinfo -B` → `Mesa Intel(R) UHD Graphics (CML GT2)` (hardware), `dmesg` without `GPU HANG` over the whole session |
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

### Open points & known issues
- AC-13.6, 13.7, 13.8, 13.9, 13.11, 13.12, 13.17 still need a hands-on run (second client, two sound cards, unplug, screen share viewer, plugin, `pkg delete`).
- T6 (Diagnostics lines, FR-13.30/31), T7 (release wiring) and T8 (CI in a FreeBSD VM) are not implemented yet.
- Screenshots: the test machine returns black pixels for every capture tool, so UI screenshots have to come from the VM.
- KWallet on a fresh KDE profile accepts secrets and drops them; the app treats it as unusable and uses the encrypted file (documented behaviour, `e6e15205`).

### Technical notes
- New/changed: `src-tauri/src/single_instance.rs` (Unix socket handover), `crates/audio/src/rt.rs` (rtprio), `packaging/freebsd/alsa-mumcord.conf` + `pkg-message`, `crates/audio/src/io/devices.rs`, `AppInfo.limits` + `PlatformLimit`, `CameraUnavailable::UnsupportedPlatform`, `SoundUnavailableReason::Platform`.
- Build: `scripts/build-freebsd-pkg.sh` builds with `--features custom-protocol` (a plain release build is a dev build), `CARGO_BUILD_JOBS=4` is stable on 8 GB.
- Machine setup used: FreeBSD 15.1, `drm-66-kmod` (6.12 hangs this GPU), `mac_priority` + group `realtime` (gid 47), KDE Plasma 6 on X11, Jabra Link 380.
- No new environment variables or secrets; the profile layout is unchanged.
- Tested server: `mumble-server` 1.5.939 from the FreeBSD package collection.
