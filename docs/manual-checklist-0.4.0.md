# Release 0.4.0: manual test checklist

Everything Epic 2 needs a human or real hardware for (spec section 10, Q5.1). Automated tests and the QA tasks C1–C7 cover the rest of the acceptance criteria. These checks never block development; failures become bug tasks in `specs/02-multi-server-themes-tasks.md`.

Rules for every check:

- Use a local test server (`testenv/README.md`) or a Mumble server you run yourself. Never a public or third-party server.
- Use the build you want to release: the CI artefacts of the release commit or a local `npm run tauri build`.
- Write down per check: OS and version, pass / fail, and on a failure what you saw. Attach the Mumcord log (_Settings → Diagnostics_); it contains no chat, names, addresses or audio.

## 1. Listening (headphones, about 20 min)

A second client (Mumcord or the official Mumble client) talks or plays music in the same channel.

- [ ] **Per-person volume (F9, F13):** open the other person's profile card, drag their volume from 0 % to the maximum. The loudness changes evenly over the whole range (no dead half), and already while dragging, not only on release.
- [ ] **Music quality loudness (F14):** one client with music quality on plays music, another talks normally. Both are about equally loud. Note: voice activation may cut quiet instrumental parts; use push-to-talk or continuous for the music client.
- [ ] **Auto-ducking (B61, AC-2.57):** during a watch party, someone talks. The video gets quieter (about 30 %) within a moment and comes back about a second after they stop. Switch "Lower video sound while people talk" off: the volume stays. Set the slider to 60 %: the drop is smaller. Repeat while watching a screen share with sound (Windows or macOS sharer).
- [ ] **Screen-share sound (SS5):** a Windows or macOS sharer shares a window that plays sound. The viewer hears it clean (no crackling, no drift against the picture over 5 minutes), and does not hear the voice chat a second time. Deafening the viewer also mutes the share sound.
- [ ] **Activity sounds (B62, AC-2.58):** a second client starts a watch party, joins and leaves it, starts and stops a screen share, starts watching your share: each plays its sound once. Your own start or join plays nothing. A toggle switched off keeps that sound silent.
- [ ] **Epic 1 listening checks:** the "Listening with headphones" part of the [stage 1a gate](../specs/01-foundation-implementation.md#manual-checklist-for-the-stage-1a-gate-user), if not done yet.

## 2. Windows in-game overlay (Windows PC, about 30 min)

- [ ] **DX11 and DX12 games (AC-2.20):** one DX11 and one DX12 game in borderless fullscreen. The overlay shows who is talking, follows when the game window moves or resizes, and clicks go through to the game.
- [ ] **Alt-tab:** the overlay hides when another app is focused and returns when the game is focused again.
- [ ] **Exclusive fullscreen:** a game in exclusive fullscreen shows no overlay, and _Settings → Overlay_ shows the borderless / windowed fullscreen hint.
- [ ] **Anti-cheat game:** the overlay shows, and no file in the game folder changed (compare the folder's file list and dates before and after).
- [ ] **HDR (AC-2.21):** with Windows HDR on, the overlay colours look like on an SDR desktop (not too bright).
- [ ] **Multi-monitor and scaling:** a game on the second monitor and a desktop scaling of 150 %: the overlay sits on the game, at the right size.
- [ ] **Window overlay notice (AC-2.24):** update a 0.3.x install that had the window overlay switched on. One notice explains the removal and opens the in-game overlay settings; it does not come back after a restart; no window overlay settings or tray item exist.

## 3. Linux overlay (KDE and GNOME, about 30 min)

- [ ] **Borderless fullscreen (FR-2.31):** a Vulkan game in borderless fullscreen on KDE and on GNOME shows the overlay.
- [ ] **HDR10 (AC-2.21):** an HDR10 game on an HDR desktop: overlay colours match SDR appearance.
- [ ] **Flatpak Steam (AC-2.22):** a game started from Flatpak Steam shows the overlay.
- [ ] **Proton (Tyr test from Epic 1):** a Proton/DXVK game shows the overlay and draws the talking people.
- [ ] **OpenGL launch option (AC-2.38):** copy the launch option from _Settings → Overlay_, start `glxgears` (or a native OpenGL game) with it: the overlay draws; the path stays the same after restarting Mumcord, also from the AppImage.
- [ ] **No game crash on exit (AC-2.23):** quit Mumcord while a game shows the overlay. The game keeps running, the overlay disappears; start Mumcord again and the overlay comes back.

## 4. Screen sharing on Windows and macOS

- [ ] **Windows:** every step of [docs/screen-share-windows.md](screen-share-windows.md). Note the Windows version (`winver`).
- [ ] **macOS:** every step of [docs/screen-share-macos.md](screen-share-macos.md) (macOS 14 or newer; macOS 13 steps if a machine is available). Note the macOS version and Mac model.

## 5. Screen sharing on Linux (about 15 min)

- [ ] **Wayland picker (SS10):** on GNOME and on KDE, "Share screen" opens the desktop's picker; pick a window, then a whole screen; the viewer sees both. Cancelling the picker starts nothing and shows no error.
- [ ] **X11 (SS9):** on an X11 session only whole-screen sharing is offered and the viewer sees the screen.
- [ ] **No sound hint:** on Linux the sharer sees the hint that no sound is sent.
- [ ] **AppImage (SS10b):** sharing on Wayland also works from the AppImage (the host's PipeWire is used).

## 6. Two real home networks (about 30 min)

Two people in different homes (different internet connections, ideally one with IPv6 and one without), both in the same channel of a Mumble server you run yourself that both can reach, for example the test environment on one of the two machines with its Mumble port forwarded. Allow direct connections on both.

- [ ] **Direct connection:** open a full-size image from the other person. It loads over the direct connection (noticeably faster than through the server). Note which side had IPv6 and whether the router allows UPnP / NAT-PMP / PCP.
- [ ] **Screen share across homes:** one shares, the other watches at 720p30; then the other way round.
- [ ] **Slow upload:** the sharer starts a large upload at the same time (or shares to several viewers). The picture steps down to 480p15 instead of freezing, and recovers later.
- [ ] **No direct path:** if one connection is behind carrier-grade NAT (or with UPnP switched off on both routers and no IPv6), "Watch" ends after about 15 seconds with a clear reason, and full-size images still arrive through the server.
- [ ] **Watch party across homes:** start a party, the other joins; play, pause and seek stay in sync within about a second.

## 7. Official Mumble client (about 20 min)

An official Mumble client (1.5) in the same channel as Mumcord.

- [ ] **Theme code (AC-2.13):** share a theme into the channel. The official client sees the plain code text.
- [ ] **Client version (AC-2.19):** as SuperUser in Mumcord, open the official client's profile: it shows its Mumble version and OS. As a normal user these lines are hidden.
- [ ] **Rich-text comment (AC-2.32):** write a comment with bold, a link and a list in Mumcord; the official client shows the formatting. Edit in Mumcord a comment the official client wrote; its formatting stays.
- [ ] **Replies and quotes (AC-2.41, AC-2.49):** reply to a message and send "> hello" in Mumcord. The official client shows the quote as readable text before the reply, and "> hello" as a quote.
- [ ] **Channel password (AC-2.59):** the official client with the same access token as entered in Mumcord can join the protected channel.
- [ ] **Watch party and screen share (AC-2.28):** start both in Mumcord. The official client sees nothing of them and gets no chat messages.

## 8. Linux rendering, tray and desktop (about 15 min)

- [ ] **Flicker fix (F44c):** on Linux (WebKitGTK 2.54), run a watch party with a playing video, hover over chat messages and let someone talk. Nothing flickers. Start Mumcord with `MUMCORD_WEBKIT_COMPOSITING=1`: WebKit's default compositing is used (flicker may come back, which confirms the switch). An own `WEBKIT_DISABLE_COMPOSITING_MODE=0` is kept.
- [ ] **YouTube in release builds (AC-2.54):** in the AppImage and the `.deb`, a YouTube card and a Twitch card in the chat play inline, and a watch party plays in the widget. In the AppImage, a video that YouTube only offers as H.264 shows the codec hint; after installing gst-libav on the system and restarting, it plays.
- [ ] **Tray tooltip (AC-2.33):** on KDE, hovering the tray icon shows the tooltip.
- [ ] **App menu offer (AC-2.35):** start a plain downloaded AppImage: it offers "Add Mumcord to the app menu"; after accepting, the entry with icon appears.
- [ ] **Start at login (AC-2.36):** switch on "Start Mumcord when I log in", log out and in: Mumcord starts. On Linux, Windows and macOS.

## 9. Epic 1 leftovers

- [ ] **Stage 1a gate (G1):** the Linux/KDE, Windows and macOS parts of the [Epic 1 manual checklist](../specs/01-foundation-implementation.md#manual-checklist-for-the-stage-1a-gate-user), if not done yet.
- [ ] **Wayland push-to-talk compatibility mode (AC-1.38):** on KDE Wayland, switch on _Settings → Voice → "Compatibility mode (X11)"_ and restart. A key combination works as push-to-talk while another app is focused; switching it off and restarting returns to the modern path.
