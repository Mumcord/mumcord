# FreeBSD support · Brainstorming

> Basis: Standalone feature, not part of the roadmap (`specs/roadmap.md`). Spec number 13: 12 belongs to `12-ssd-write-reduction-*`.
> Process: We work in several rounds. Each round contains questions; you answer directly in this file.
> Once all questions are answered, `specs/13-freebsd-support-specification.md` is created from it.
> Already settled (not asked again):
> - **Tech stack** – the same Tauri 2 + React UI and Rust core as everywhere else, no second UI (Spec 01).
> - **Platform** – FreeBSD 15.x on amd64; arm64 is out of scope for now.
> - **Build state** – the app already builds, links, starts, connects to a local Mumble server and plays/records through OSS in a test prototype; this brainstorming decides what the *supported* result promises.
> - **Secrets** – where no Secret Service (gnome-keyring/KWallet) answers, the identity and server passwords live in an encrypted file in the profile directory instead (AES-256-GCM, 0600 key next to it).
> - **Audio** – ALSA over OSS (`alsa-lib` + the OSS plugin); no PipeWire or PulseAudio in the first version.
> - **Updater** – the app never replaces itself; `pkg upgrade mumcord` is the only update path, and the app only says so.
> - **Screen capture** – X11 only (the whole screen); FreeBSD has no `xdg-desktop-portal`, so there is no window picker.
> - **Packaging** – a FreeBSD `.pkg` built from the release binary by `scripts/build-freebsd-pkg.sh`, installed under `/usr/local`.
> - **CI** – comes after the feature work; the checks it runs are decided in round 5.

## How to answer

- **Single choice** (`◉`): Exactly **one** box has an `[x]`. To change your choice, just move the `x`.
- **Multiple choice** (`☑`): Tick as many boxes as you like.
- ⭐ marks my **recommendation**. It is already preselected. If you agree, you don't need to change anything.
- Below each question there is `✍️ Note:`. Add anything there, e.g. questions back, edge cases or "not sure, explain this to me".
- I explain technical terms briefly. For purely technical questions (audio, network, protocol) it's perfectly fine to follow my recommendation.

## Planned rounds

1. Functionality & scope: what FreeBSD promises and what it openly does not have
2. UI & UX: how the app presents the platform gaps, desktop integration
3. Audio & network: OSS devices, share sound, positional audio, latency
4. Technology & platforms: supported FreeBSD versions, dependencies, secrets, updates
5. Data/state, packaging, CI & open points

(The plan may change depending on the answers.)

---

## Round 1 – Functionality & scope

Some Mumble features need a component that FreeBSD does not have (a desktop portal, PipeWire, ONNX Runtime for AI backgrounds). The questions below decide what a FreeBSD installation *promises* and what it hides or disables with a short note.

### A · What the first FreeBSD version promises

#### Q1.1 Which feature set is "FreeBSD support" in the first release? `◉`
Everything below is what the prototype already does today. The question is how much of it we call supported and test before we announce the platform.

- [x] ⭐ **The full client except the platform gaps listed in Q1.3–Q1.5.** Connect, voice (OSS), text chat, file sharing, watch party, plugins, hotkeys and push-to-talk, tray icon, notifications, "now playing", positional audio, screen sharing over X11.
- [ ] Only the core: connect, voice, chat. Everything else stays hidden on FreeBSD until it is proven.
- [ ] Everything including the gaps: we would need components FreeBSD does not have, so this is not realistic.

✍️ Note:

#### Q1.2 Screen sharing: the whole X11 screen, without sound. Ship it like that? `◉`
FreeBSD has no portal, so there is no "pick a window" dialog and no way to grab a single app's sound. A share is the whole screen and silent for the viewers; the voice of the sharer keeps working.

- [x] ⭐ **Ship it: whole screen, no share sound, and say so in the start dialog.** Viewers see the screen and hear the sharer's microphone as usual.
- [ ] Ship it but hide it on Wayland sessions (where there is no X11 screen to read).
- [ ] Do not ship screen sharing on FreeBSD; hide the button and note the reason.

✍️ Note:

#### Q1.3 Camera and AI backgrounds: both impossible on FreeBSD (no camera backend, no ONNX Runtime). How should the client behave? `◉`
The camera room would be empty and every background effect unavailable. Leaving the UI as it is would promise something that never works.

- [x] ⭐ **Show the camera entry, disabled, with one short line ("not available on FreeBSD yet").** Users see that Mumcord has the feature and that it is missing here, instead of wondering if it is broken.
- [ ] Hide camera and effects completely on FreeBSD, as if the features did not exist.
- [ ] Show them enabled and let the attempt fail with an error message.

✍️ Note:

#### Q1.4 Positional audio (plugin-based, "Mumble Link"): the prototype can list the running programs and share memory, but no game on FreeBSD is known to write the Mumble Link data. `◉`
What is missing is not the client but games: the data a game has to publish is written by the game, and almost no native FreeBSD game does it. Wine games could, in theory.

- [x] ⭐ **Supported "best effort": settings and plugins work, and the UI says positional audio needs a game that publishes the data.** No promise that a specific game works.
- [ ] Hide positional audio on FreeBSD until a real game is known to work.
- [ ] Promise full positional audio and test it with a game before releasing.

✍️ Note:

#### Q1.5 Wasm plugins, watch party and file sharing run on the same code as on Linux. Any of them excluded on FreeBSD? `☑`
These need no portal, no PipeWire and no special hardware, so there is no technical reason to exclude them.

- [x] ⭐ Wasm plugins (install, run, plugin frames for chat cards)
- [x] ⭐ Watch party (embedding and syncing the shared player)
- [x] ⭐ File sharing (send, receive, previews)
- [ ] Exclude one of them — please say which and why in the note

✍️ Note:

### B · What we tell the outside world

#### Q1.6 FreeBSD as an official platform: how far do we go? `◉`
"Support" can mean a package in our releases, a line in the README, a page in the docs, and — the biggest step — an entry in the FreeBSD ports collection, where `pkg install mumcord` would work for every FreeBSD user.

- [ ] ⭐ **Package in our releases, README platform table, and a short docs page with the known limits.** The ports collection can come later once the platform has proven itself.
- [ ] The above plus submitting the port to the FreeBSD ports collection now.
- [x] Only the package in our releases, no README or docs changes.

✍️ Note:

#### Q1.7 How do we word the limits so nobody feels tricked? `◉`
A user who installs Mumcord on FreeBSD and finds a missing camera should understand why within a few seconds, not after searching.

- [x] ⭐ **One "FreeBSD" note in the app (About/Settings → diagnostics) plus the disabled entries from Q1.3.** Short, in the same place where the version is shown.
- [ ] Only the disabled entries, no extra explanation.
- [ ] A one-time dialog on the first start that lists what FreeBSD does not have.

✍️ Note:

---

## Round 2 – UI & UX

Round 1 settled *what* FreeBSD offers. This round is about how the app presents it inside the window, and about the desktop integration that lives outside it (tray, hotkeys, notifications, autostart, menu entry).

Because Q1.6 chose "no README or docs changes", the in-app note is the only place a user can learn about the limits — so its wording matters more than usual.

### A · The FreeBSD note in the app

#### Q2.1 Where exactly should the note live? `◉`
Candidates are the same places where the version and platform already appear. The note must be findable when something does not work, without being in the way of normal use.

```text
Settings → About
┌──────────────────────────────────────────────────────────┐
│ Mumcord 0.8.0                                            │
│ FreeBSD 15.1 · amd64 · package                            │
│                                                          │
│ Not available on FreeBSD: camera, camera background      │
│ effects, in-game overlay, screen-share sound.            │
│ Screen sharing captures the whole X11 screen.            │
│ Updates come with "pkg upgrade mumcord".                 │
└──────────────────────────────────────────────────────────┘
```

- [x] ⭐ **Settings → About, directly under the version line, always visible.** It is where users look when they check what they installed, and it needs no extra click.
- [ ] Settings → Diagnostics (the crash-report page) only.
- [ ] Both: About for the short list, Diagnostics for the technical detail.

✍️ Note:

#### Q2.2 How long should the note be? `◉`
The trade-off is honesty against noise: every extra sentence is read less often.

- [x] ⭐ **Three short lines, one per missing area, each naming the area and the reason in a few words.** Example: "Camera and AI backgrounds: no camera backend / no ONNX Runtime on FreeBSD."
- [ ] One single line: "Some features are not available on FreeBSD."
- [ ] A full list with a paragraph per feature.

✍️ Note:

### B · Desktop integration outside the window

The table shows what each piece needs on FreeBSD. "Needs a host" means another program has to provide the service; without it the feature stays silent.

| Feature | What it needs on FreeBSD | Present by default? |
|---|---|---|
| Tray icon | a StatusNotifier host in the panel (KDE, GNOME with an extension, Xfce with a plugin, …) | no — depends on the desktop |
| Global hotkeys, keyboard push-to-talk | X11 (works under XWayland too) | yes in an X11 session |
| Mouse side-button push-to-talk | X11 XInput2 | yes in an X11 session |
| Notifications | a notification service on the session D-Bus | usually yes, some minimal window managers have none |
| Start at login | an XDG autostart file in `~/.config/autostart` | yes, any session |
| Menu entry, `mumble://` links | the `.desktop` file the package installs | yes |

#### Q2.3 Tray icon: what should happen when the desktop has no StatusNotifier host? `◉`
On a bare window manager the panel simply has no tray area, so the icon can never appear. The setting would look broken if we left it switched on.

- [x] ⭐ **Keep the setting, and show the same hint line the Linux build uses when no host answers.** The user learns why the icon is missing instead of filing a bug.
- [ ] Disable the tray setting on FreeBSD entirely and hide it.
- [ ] Leave it as it is with no hint.

✍️ Note:

#### Q2.4 Hotkeys and push-to-talk on a Wayland session (sway, Wayfire, …): global key grabbing needs X11 or a portal, and FreeBSD has no portal. `◉`
Under XWayland the X11 grab still works as long as the key is not swallowed by the compositor. Native Wayland windows get no global hotkey at all.

- [x] ⭐ **Allow hotkeys, and in Settings → Hotkeys show one hint line when the session is Wayland: "Global hotkeys need an X11 session on FreeBSD".** Nothing is hidden, but nobody is surprised by a hotkey that does nothing.
- [ ] Disable hotkeys and push-to-talk on Wayland sessions with a clear message.
- [ ] Say nothing; hotkeys simply may not work there.

✍️ Note:

#### Q2.5 Notifications: when no notification service answers the session D-Bus, should the app say so? `◉`
On Linux the same code path already exists; a minimal window manager without a notification daemon is more common on FreeBSD than on a full desktop.

- [x] ⭐ **Same behaviour as Linux: one quiet hint in Settings → Notifications when nothing answers, everything else unchanged.**
- [ ] Add a FreeBSD-specific warning dialog on the first start.
- [ ] Say nothing.

✍️ Note:

#### Q2.6 Start at login and the menu entry: the package ships `/usr/local/share/applications/app.mumcord.client.desktop`, and the app already removes its own user entry when a system one exists. Anything else needed? `◉`
Users who unpack a development build by hand have no system entry. Today the app writes one in that case, which is why a manual install still appears in the menu.

- [x] ⭐ **Keep exactly this: the package provides the entry, a hand-unpacked build writes its own, and `mumble://` links work in both cases.** No new UI.
- [ ] Never write a user entry on FreeBSD; hand-unpacked builds get no menu entry and no link handling.
- [ ] Always write a user entry, even with the package installed (two entries in the menu).

✍️ Note:

### C · Look and feel

#### Q2.7 Appearance: dark/light theme, scaling and the display language are taken from the GTK settings on FreeBSD, like on Linux, and the setup wizard keeps its ten steps unchanged. `◉`

- [x] ⭐ **Yes, keep the wizard and the GTK-driven appearance unchanged on FreeBSD.** No platform-specific screens.
- [ ] Skip the wizard on FreeBSD and jump straight into the main window.
- [ ] Add a FreeBSD-specific wizard step that lists the missing features.

✍️ Note:

---

## Round 3 – Audio & network

Voice is the core of Mumcord, and FreeBSD's sound stack differs from Linux': the system talks to `/dev/dsp` (OSS) and the ALSA library is only a compatibility layer on top. The prototype already plays and records that way; this round decides how much choice the user gets and what we promise about latency and direct connections.

### A · Sound devices

#### Q3.1 A machine with two sound cards (onboard + USB headset) — how should the user pick one? `◉`
Today the picker shows a single entry ("Open Sound System"): that is whatever the system's default unit is. Choosing between the two means changing `hw.snd.default_unit` in the whole system, not in Mumcord.

- [x] ⭐ **The package adds a small ALSA config that exposes each `/dev/dsp` unit as its own entry, so the picker lists "Sound card 0/1/…" and Mumcord alone chooses.** Cost: one config file in the package; risk: other audio apps see the same extra entries, which is harmless.
- [ ] One entry only ("System default"); switching cards means `sysctl hw.snd.default_unit=N` in a terminal, explained in the About note.
- [ ] Do it later; ship one entry now and add per-card entries in a follow-up.

✍️ Note:

#### Q3.2 When the chosen device disappears (USB headset unplugged mid-call): what should happen? `◉`
Linux already handles this by falling back and telling the user. FreeBSD's ALSA layer reports the loss too.

- [x] ⭐ **Same as Linux: fall back to the system default, keep the call, and show the existing "device changed" notice.** No FreeBSD-specific behaviour.
- [ ] Stop the audio and show an error until the device is back.
- [ ] Do nothing and let the user notice.

✍️ Note:

### B · Latency and load

#### Q3.3 Audio threads and real-time priority: Linux asks rtkit over D-Bus, which does not exist on FreeBSD, so the audio threads run at normal priority there today. FreeBSD can raise a thread's priority itself (`rtprio`). `◉`
Without it a fully loaded CPU can make the voice stutter, which is the one thing users notice immediately. It is a small change in the audio crate, FreeBSD only.

- [x] ⭐ **Raise the capture and playback threads to real-time priority on FreeBSD (same values as the Linux path asks rtkit for).** Few lines, no dependency, matches what other FreeBSD audio apps do.
- [ ] Leave normal priority; a busy CPU may stutter.
- [ ] Only after a real-world report of stutter.

✍️ Note:

#### Q3.4 Sound processor settings (noise suppression, echo cancellation, AGC), volume handling and the voice-activation threshold are pure Rust code and identical to Linux. `◉`

- [x] ⭐ **Keep them unchanged, with the same defaults as on Linux.** Nothing to port.
- [ ] Disable echo cancellation on FreeBSD by default (it is the most CPU-hungry part).
- [ ] Ask again after a measurement on FreeBSD hardware.

✍️ Note:

### C · Network

#### Q3.5 TCP/UDP to the server, TLS, the certificate "identity" and reconnects are shared code; nothing is platform-specific. Direct Mumcord-to-Mumcord connections (QUIC/UPnP/NAT-PMP) use a BSD-capable network library, but are unproven on FreeBSD. `◉`
Direct connections are optional: without them two Mumcord users still talk through the Mumble server.

- [x] ⭐ **Keep them enabled and verify them once against a second client on the test network; if they fail, hide the option on FreeBSD and note it.** Users keep the option everywhere it works.
- [ ] Disable direct connections on FreeBSD until they are proven.
- [ ] Leave them enabled with no verification.

✍️ Note:

#### Q3.6 The local test server for the acceptance checks: `audio/mumble-server` from the FreeBSD package collection (same Mumble server version as on Linux) instead of the Docker setup the Linux tests use. `◉`

- [x] ⭐ **Yes: a native `mumble-server` on the test host, started from a test config, exactly like the Linux CI uses Docker for the same purpose.**
- [ ] Use only a public test server (not allowed by our rules, listed for completeness).
- [ ] Skip server tests on FreeBSD and check only the app start.

✍️ Note:

---

## Round 4 – Technology & platforms

What is left to decide are the edges around the package: which FreeBSD versions we stand behind, what the package pulls in, what happens on a second launch, and how much of the diagnostics machinery still works without `/proc`.

### A · Supported versions and installation

#### Q4.1 Which FreeBSD releases do we support? `◉`
FreeBSD keeps two "supported branches" alive at a time (today 14.3 and 15.x). A package built on one major version cannot be installed on the other, so every extra branch is a second build, second test run and second set of bug reports.

- [x] ⭐ **15.x only for the first release (amd64), with 14.x added when someone asks for it.** 15 is the current release; a single target keeps the build and the CI small.
- [ ] Both supported branches (14.3 and 15.x) from day one.
- [ ] Only the newest point release (e.g. 15.1), refusing older 15.x.

✍️ Note:

#### Q4.2 The package depends on what it links: WebKitGTK 4.1, GTK 3, ALSA with the OSS plugin, the appindicator library for the tray, and the three video codecs for screen sharing. Installing Mumcord therefore pulls roughly a gigabyte of libraries. `◉`
Everything except the codecs is needed before the window can even open.

- [x] ⭐ **Keep them all as hard dependencies — installing Mumcord must be enough to use it.** One `pkg install`, no follow-up hunt for missing libraries.
- [ ] Make the screen-share codecs optional ("recommended"), so a voice-only install is smaller; screen sharing then fails until they are installed.
- [ ] Ship everything bundled inside our own package, ignoring the system libraries.

✍️ Note:

#### Q4.3 Besides the repository package, should there be a plain archive for users without root? `◉`
Some users have no root on their machine (company laptop, shared server). A `.tar.gz` with the binary and the resource folder runs from the home directory but gets no menu entry or `mumble://` handling from the package.

- [x] ⭐ **Package only.** A tarball would be a second distribution path that has to be tested and updated, for a small group.
- [ ] Package plus a `.tar.gz` that the app can also run from.
- [ ] Package plus a self-contained single-file launcher.

✍️ Note:

### B · Running the app

#### Q4.4 A second launch: the single-instance module Mumcord uses has no FreeBSD implementation, so today a second `mumcord` — or a `mumble://` link clicked while the app is already running — opens a *second* window instead of handing the link over. `◉`
That would also mean two connections, two sound streams and two tray icons; the link would open a second window with its own profile lock.

- [x] ⭐ **Add a small FreeBSD single-instance lock (a Unix socket in `$XDG_RUNTIME_DIR`, like the macOS path does) so a second launch hands its arguments over and exits.** Fixes the double window and makes `mumble://` links behave like on the other platforms; roughly 100 lines.
- [ ] Accept the second window on FreeBSD for now and document it in the About note.
- [ ] Block the second launch with an error message instead of handing over.

✍️ Note:

#### Q4.5 Crash reports: Mumcord writes a report on a fatal crash and resolves symbols at the next start. FreeBSD has no `/proc` by default, so the address-to-library mapping is missing and the symbols are less precise than on Linux. `◉`
The alternative, reading the kernel's process data instead, is a bigger piece of platform code.

- [x] ⭐ **Keep crash reports on FreeBSD, best effort: the native stack and the system information (already collected) but coarser symbol names.** Say it in the Diagnostics page so nobody wonders about "??" in a report.
- [ ] Disable crash reports on FreeBSD entirely.
- [ ] Build the full `/proc`-free symbol machinery now.

✍️ Note:

#### Q4.6 Logs, the profile database, the encrypted secret file and the cache folders keep the same XDG locations as on Linux (`~/.local/share/app.mumcord.client`, `~/.cache/app.mumcord.client`). `◉`
FreeBSD desktops follow the same XDG spec, so a user's home looks identical to a Linux one.

- [x] ⭐ **Yes, unchanged.** Nothing platform-specific, no new paths in the About note.
- [ ] Move them to FreeBSD's classic `/usr/local` conventions.
- [ ] Ask again once the package has run on a real desktop.

✍️ Note:

---

## Round 5 – Data, packaging, CI & open points

The last round. It decides how the package reaches users, what the CI proves before a release, and whether anything in the stored data changes.

### A · What a release ships

#### Q5.1 The other platforms' releases carry installers, a `SHA256SUMS` file, a software bill of materials and the third-party notices. Should the FreeBSD package join them? `◉`

- [x] ⭐ **Yes: the `.pkg` becomes a normal release asset, listed in `SHA256SUMS`, in the SBOM and in the notices, exactly like the AppImage and the installers.** One place to download, one integrity story.
- [ ] Upload it to the release but keep it out of the checksum/SBOM steps.
- [ ] Host it outside the releases (own server).

✍️ Note:

#### Q5.2 Where do users get the package from? `◉`
A FreeBSD *repository* lets `pkg install mumcord` work and `pkg upgrade` find new versions, but it needs hosting, a repository index and signing. A single `.pkg` file from the release page needs one `pkg add` by hand, without automatic updates.

- [x] ⭐ **Release asset only for the first version: download the `.pkg`, `pkg add` it, and update by repeating that.** A signed repository can follow once the platform is established.
- [ ] Build and host a signed Mumcord package repository right away.
- [ ] Ask the FreeBSD ports collection to carry it (decided against in Q1.6).

✍️ Note:

#### Q5.3 Should the release notes and the changelog name FreeBSD as a new platform? `◉`

- [x] ⭐ **Yes: one `CHANGELOG.md` entry under "Added" and a line on the release page listing the download, the supported version (15.x, amd64) and the known limits in one sentence each.**
- [ ] Only the changelog entry, no release-page text.
- [ ] No announcement; the asset is enough.

✍️ Note:

### B · What the CI proves

#### Q5.4 Which FreeBSD checks run on every pull request, and which only before a release? `◉`
GitHub has no FreeBSD machines; the honest way is a small FreeBSD virtual machine started on a Linux runner (`vmactions/freebsd-vm`). It is real FreeBSD, but slower than the other runners: booting the VM and a cold build cost several minutes.

- [x] ⭐ **On every pull request: `cargo check`, `cargo clippy` and `cargo test` for the workspace in the FreeBSD VM.** Building the package and starting the app against a test server runs before a release (and on demand), because that is the slow part.
- [ ] Only `cargo check` on pull requests, everything else before a release.
- [ ] Everything on pull requests, including the package build and the smoke test.

✍️ Note:

#### Q5.5 The package build and the end-to-end check need a real FreeBSD machine with the dependencies installed. Run them on our own test host (the one used so far) or inside a FreeBSD VM on GitHub? `◉`
A self-hosted machine is fast and keeps the caches, but it has to be maintained and it runs code from pull requests. A VM is thrown away after each run and needs no care.

- [x] ⭐ **A FreeBSD VM on a GitHub runner for the release build as well, started from a script in the repository, so nothing depends on one private machine.** Our own host stays a development convenience.
- [ ] Self-hosted FreeBSD runner for everything.
- [ ] Build the release package by hand on the test host and upload it to the release.

✍️ Note:

### C · Stored data

#### Q5.6 Does FreeBSD need a new database schema, migration or stored setting? `◉`
Everything the app stores (profile database, settings, chat history, secrets) is shared code; the only new files are the encrypted `secrets.vault` and its key in the profile directory.

- [x] ⭐ **No schema change and no migration: the existing profile format is used as it is, and a profile is not moved between platforms.**
- [ ] Add a FreeBSD-specific migration for profiles copied from a Linux machine.
- [ ] Change the profile format for FreeBSD.

✍️ Note:

#### Q5.7 Anything else that must be in the specification, or that we got wrong so far? `☑`
Free text here is welcome — this is the last round before the specification is written.

- [ ] Something is missing — please write it in the note
- [ ] I want to change an earlier decision — please name the question number

✍️ Note:

---

## Closing

2026-11-08: All rounds answered. The specification has been created: `specs/13-freebsd-support-specification.md`.

## Round 6 – Addendum: graphics driver and rendering

Found while testing on the physical machine (`192.0.2.10`, Intel Comet Lake UHD, FreeBSD 15.1): the first Mumcord start froze the entire desktop, not just the app.

**Cause:** `drm-612-kmod` — the module the `drm-kmod` metaport installs by default — hangs these GPUs (`GPU HANG: ecode 9:1:85…`, `Resetting rcs0 for preemption time out`) as soon as a hardware OpenGL context is created. It is a known upstream report: freebsd/drm-kmod issue #509 ("drm-612-kmod - Comet Lake iGPU, sporadic rcs0 preemption-timeout hangs", open), with the same device family and error code.

**Measured on the machine:**
- with `drm-612-kmod`: GPU hang, X server wedged, reboot required
- with `drm-66-kmod`: `glxinfo -B` → `Mesa Intel(R) UHD Graphics (CML GT2)`, OpenGL 4.6, accelerated; KWin on OpenGL; Mumcord with hardware GL; **zero hangs**, desktop responsive

#### Q6.1 What should Mumcord do about it? `◉`
Answered in chat on 2026-11-08.

- [x] ⭐ **Use the driver the system has loaded (hardware GL), and state the requirement where users see it: the package's install message and the release notes.** Best performance on working setups; a user with the default 6.12 driver reads the note and switches to `drm-66-kmod`. `LIBGL_ALWAYS_SOFTWARE=1` stays the documented escape hatch.
- [ ] Force software rendering on FreeBSD (safe, but slower and hides the real fix).
- [ ] Detect the installed module version and decide per machine.

✍️ Note: Chosen because the bug is in the driver and is tracked upstream; punishing every FreeBSD user with software rendering would be the wrong default.

---
