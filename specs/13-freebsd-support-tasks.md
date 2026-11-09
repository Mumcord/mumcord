# Mumcord · FreeBSD support · Tasks

> Spec: `specs/13-freebsd-support-specification.md`
> Mode: single session (no sub-agents available), task list kept as this file.
> Status: `open` · `in progress` · `done` · `blocked`. Only the lead edits this file.
> Commit only on request.

## T1 – Single-instance lock and argument handover
- **Status:** done (verified on the test machine: second launch exits 0, the link reaches the running window)

- **Complexity:** complex (new platform module, process handover)
- **Covers:** FR-13.12, AC-13.15
- **Files:** `src-tauri/src/single_instance.rs` (new), `src-tauri/src/lib.rs`, `src-tauri/src/deeplink.rs`
- **Plan:** a Unix socket in `$XDG_RUNTIME_DIR` (fallback: `std::env::temp_dir()` with the uid in the name), created before the Tauri builder starts. A second launch connects, sends its command line as one JSON line, and exits. The running instance reads it and hands every `mumble://` / `mumcord://` argument to the existing link handling.
- **Verify:** start the app, start it again with a link → one process, the running window gets the link, second process exits 0; unit tests for the line protocol (round trip, invalid line, socket in use).

## T2 – One device entry per `/dev/dsp` unit
- **Status:** done, switching between two units not exercised (the machine has one `/dev/dsp` only)

- **Complexity:** complex (audio + packaging + ALSA config)
- **Covers:** FR-13.15, AC-13.5
- **Files:** `packaging/freebsd/alsa-mumcord.conf` (new), `scripts/build-freebsd-pkg.sh`, `crates/audio/src/io/devices.rs`, `crates/audio/src/io/cpal_backend.rs`
- **Plan:** the package installs an ALSA configuration that defines one PCM per `/dev/dspN` (the `oss` plugin with an explicit `device`), so `cpal` lists them as hints; the FreeBSD filter keeps them. First prove the config with `aplay -D <name>` on the test machine.
- **Verify:** with two units (Jabra + onboard, or a second `/dev/dsp` from `virtual_oss`), the picker lists both, switching works inside one session, and the engine log shows the chosen device.

## T3 – Real-time priority for the audio threads
- **Status:** done (threads show `real:10` with `mac_priority` + realtime group; without it they stay normal)

- **Complexity:** complex (real-time threading)
- **Covers:** FR-13.17
- **Files:** `crates/audio/src/rt.rs`
- **Plan:** the existing Linux/macOS/Windows path asks rtkit/the OS; FreeBSD gets `rtprio(RTP_PRIO_REALTIME)` for the capture and playback threads, attempted once per thread with the same values as Linux. If the kernel denies it for an unprivileged user, log once and continue (never fail the stream).
- **Verify:** the thread reports realtime priority (`rtprio` of the audio thread), audio keeps working, no error spam.

## T4 – About note and platform line
- **Status:** done (component test; visual check pending, the test machine cannot screenshot)

- **Complexity:** simple
- **Covers:** FR-13.28, AC-13.14
- **Files:** `ui/src/features/settings/About*`, `ui/src/locales/*`, `src-tauri/src/commands/*` (platform info), `crates/engine/src/sysinfo.rs`
- **Plan:** the app exposes platform name/version, architecture, install kind and session type; the About panel shows them under the version plus the gap lines and the Intel driver line of the spec.
- **Verify:** Settings → About on the FreeBSD machine shows the lines; Linux/Windows/macOS output unchanged.

## T5 – Features that cannot work are disabled with a reason
- **Status:** done (camera reports `unsupportedPlatform` and stays visible but disabled; the share dialog names "whole X11 screen, no sound")

- **Complexity:** simple
- **Covers:** FR-13.27, FR-13.19, AC-13.13
- **Files:** `ui/src/features/…` (camera room, camera settings, share dialog), `src-tauri/src/commands/*`
- **Plan:** one capability command (`camera`, `screen_share_sound`, `overlay`) derived from `cfg!(target_os)`; the UI disables the camera entry with "not available on FreeBSD yet" and adds the share-dialog line "whole screen, no sound on FreeBSD".
- **Verify:** camera entry disabled with the line, share dialog shows the line, everything unchanged on the other platforms.

## T6 – Diagnostics and hints
- **Status:** done (system block with release, session, audio devices and the crash-symbol note, verified on the FreeBSD machine)

- **Complexity:** simple
- **Covers:** FR-13.30, FR-13.31, FR-13.8–13.11
- **Files:** `crates/engine/src/sysinfo.rs`, `src-tauri/src/commands/diagnostics.rs`, `ui/src/features/settings/Diagnostics*`
- **Plan:** system information for FreeBSD (OS/kernel/desktop/CPU/GPU/memory, session type, renderer), the note about reduced crash symbols, and the existing tray/hotkey/notification hints confirmed for FreeBSD.
- **Verify:** Diagnostics lists the facts, no personal data; hints appear in the documented situations.

## T7 – Release wiring
- **Status:** open (release wiring)

- **Complexity:** complex (CI)
- **Covers:** §10, Q5.1, Q5.2, Q5.3
- **Files:** `.github/workflows/release.yml`, `scripts/release-manifest.sh`, `scripts/generate-sbom.sh`, `scripts/notices.js` (only if the checksum list needs it), `CHANGELOG.md`
- **Plan:** the `.pkg` becomes a release asset listed in `SHA256SUMS`, the SBOM and the notices; the release page names FreeBSD 15.x and the limits; `CHANGELOG.md` gets one "Added" entry.
- **Verify:** dry run of the release steps produces the asset and the checksum entry; `release-manifest.sh` records it.

## T8 – CI on a real FreeBSD machine
- **Status:** open (CI)

- **Complexity:** complex (CI)
- **Covers:** §10, Q5.4, Q5.5
- **Files:** `.github/workflows/ci.yml`, `.github/workflows/build-and-smoke.yml`, `scripts/ci/freebsd-*.sh` (new)
- **Plan:** `vmactions/freebsd-vm` job on pull requests: install the dependencies, `cargo check`, `cargo clippy`, `cargo test --workspace`; on the release path (and on demand) the job also builds the package and runs the connect smoke test against a local `mumble-server`.
- **Verify:** the workflow runs green on the branch (dispatched run), the package from CI installs, the smoke test connects.

## T9 – Verification run
- **Status:** done (report written; the criteria that need a second client or hands on the machine are listed as open in the report)

- **Complexity:** simple
- **Covers:** all ACs
- **Plan:** every AC on the physical FreeBSD machine (KDE, Jabra) and, where a screenshot is needed, on the VM; results into `specs/13-freebsd-support-implementation.md`.
- **Verify:** the report lists pass/fail per AC with evidence.

## T10 – Media hints: name the missing package
- **Status:** done (player hint and the stall fallback, verified on the FreeBSD machine with the decoder removed)

- **Complexity:** simple (frontend)
- **Covers:** FR-13.34 – FR-13.36, AC-13.21, AC-13.22
- **Files:** `ui/src/features/watchparty/codecSupport.ts`, `ui/src/features/watchparty/CodecHint.tsx`, `ui/src/locales/*/watchparty.json`
- **Plan:** `codecSupport` learns the FreeBSD packages and their `pkg install` commands next to the existing Linux ones; the decoder probe (`isTypeSupported` for H.264, VP8/VP9, Opus, AV1) runs once per session and caches. The hint keeps its rule "only after a real failure and only for a codec the WebView lacks". AV1-only and DRM-protected failures get the browser button without a command.
- **Verify:** unit tests for every mapping (decoder present → nothing, missing → command, AV1/DRM → browser), plus AC-13.21/13.22 on the FreeBSD machine with a decoder removed.

## T11 – Diagnostics and About name the media state
- **Status:** done (Diagnostics line and the About DRM line, verified on the FreeBSD machine)

- **Complexity:** simple (frontend, rides on T6)
- **Covers:** FR-13.37, FR-13.38, AC-13.23
- **Files:** `ui/src/features/settings/sections/DiagnosticsSection.tsx`, `AboutSection.tsx`, `ui/src/features/settings/settings.css`, `ui/src/locales/*/settings.json`
- **Plan:** Diagnostics shows a "media packages" line with the copyable command while something is missing, and nothing when all decoders answer; About gains the permanent DRM line in its gap note.
- **Verify:** component tests for "missing → line with command" and "complete → no line"; AC-13.23 on the FreeBSD machine.
