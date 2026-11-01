# Release 0.5.0: manual test checklist

Everything Epic 3 (comfort and polish) needs a human or real hardware for. Automated tests cover the rest of the acceptance criteria in `specs/03-comfort-polish-specification.md`. Failures become bug tasks and never block development.

Rules for every check:

- Use a local test server (`testenv/README.md`) or a Mumble server you run yourself. Never a public or third-party Mumble server, except the server browser check in section 8, which only reads the public list.
- Use the build you want to release: the CI artefacts of the release commit or a local `npm run tauri build`.
- Write down per check: OS and version, pass / fail, and on a failure what you saw. Attach the Mumcord log (_Settings → Diagnostics_); it contains no chat, names, addresses or audio.
- Also run the still-valid checks of [docs/manual-checklist-0.4.0.md](manual-checklist-0.4.0.md) that touch areas changed in 0.5.0 (overlay, screen sharing, watch party).

## 1. Install, update and first start (Windows, macOS, Linux; about 30 min)

- [ ] **Fresh install:** install the build on a clean user account. The setup wizard starts, offers the setup styles (presets), and ends with Mumcord ready to connect.
- [ ] **Import from Mumble:** with the official Mumble client (1.4 or 1.5) installed and configured, the wizard offers the import step. Favourites, saved passwords, the certificate (as a new identity with the same registered name), audio devices and the push-to-talk key arrive; volume and voice settings only when ticked.
- [ ] **Run setup wizard again:** _Settings → About → Run setup wizard again_ has no identity step and changes nothing about the certificate.
- [ ] **In-app updater:** from the previous release (0.4.4) the **Update available** dialog appears with the changes of every version since, **Download and install** shows progress and time left, can be cancelled, and after the restart the About page shows 0.5.0. **Skip this version** and **Remind me later** behave as named. Check on Windows, macOS and the AppImage.
- [ ] **Updater signature:** an update file with a changed byte is refused with a clear message.
- [ ] **Windows portable zip:** unpack `Mumcord_0.5.0_x64-portable.zip`, start `Mumcord.exe`. Data stays in `MumcordData` beside it; no `mumble://` handler and no start at login; About says "Portable".
- [ ] **Upgrade of the database:** start 0.5.0 on a copy of a 0.4.4 profile (back it up first). All servers, friends, identities and history are still there.

## 2. Settings window (about 15 min)

- [ ] **Navigation:** all ten groups open with mouse and keyboard (arrow keys, Home / End); search finds "P2P", "relay", "noise" and a setting by its option name.
- [ ] **Badges and reset:** "Changed" shows where a value differs from the active preset; **Reset section** lists what changes and does nothing before the confirmation.
- [ ] **Language picker:** searching "Deutsch", "German" and "de" finds the same language; the app switches without a restart.
- [ ] **Scaling and theme:** with 150 % system scaling and a dark and a light system theme the window is readable and nothing is cut off.

## 3. Voice and audio (headphones, a second client talks; about 40 min)

- [ ] **Noise suppression levels:** Light, Standard, Advanced (AI) and Off with keyboard typing or a fan in the background. Advanced removes the most; note the CPU load of Advanced.
- [ ] **Low-latency mode:** on for two people playing an instrument or clapping together. The estimated mouth-to-ear delay is shown; switching it on and off does not reconnect and does not crackle.
- [ ] **Audio per channel:** set "Audio for this channel…" to the Music preset, join and leave the channel. The profile applies on joining and the normal settings return on leaving.
- [ ] **Whisper and shout:** whisper to one person, shout to a channel with sub-channels; only the targets hear it and the hold-to-talk key works while another app is focused.
- [ ] **Global hotkeys:** mute, deafen, Do Not Disturb, switch channel and switch voice server keys work with another app focused. On KDE Wayland as well.
- [ ] **Quick popovers:** right-click the microphone, headphones, hang-up and connection-info buttons. Device switching works while talking; **Leave voice** keeps chat working and joining a channel brings voice back.
- [ ] **PipeWire devices (Linux):** with a multi-output interface, every output and input appears with its friendly name and the chosen one plays and records.
- [ ] **Microphone prompt (macOS):** the prompt appears once, on first use of the microphone, not when listing devices.

## 4. Notifications and messages (two clients; about 30 min)

- [ ] **Notification centre:** the bell (Ctrl+Shift+N, Cmd+Shift+N on macOS) lists mentions, `@all` / `@channel`, replies, chat requests with Accept and Decline, and invites across two connected servers.
- [ ] **Do Not Disturb:** the switch in quick settings, settings and tray silences message, mention, watch party and screen share sounds and system notifications; voice join and leave sounds still play. With the system's Do Not Disturb on, the tray says "(system)".
- [ ] **Kick and ban sounds:** the slap on a kick and the kick on a ban play once; a normal disconnect plays neither.
- [ ] **Private messages:** "Friends only" and "Nobody" hold back messages from strangers; the exceptions (current channel, accepted conversations) work; the **Messages** overview shows both servers, pinned first, with search and filters.
- [ ] **Block and revoke:** block a person (their messages and requests stay hidden, they are not told), unblock; **Revoke chat consent…** makes them send a new request.
- [ ] **`@all` in the official client:** the official Mumble client just sees the text `@all` and `@channel`.
- [ ] **Own messages:** no sound, no notification and no unread count for your own messages, also when sent from a second session.

## 5. Chat, links and media (about 30 min)

- [ ] **Audio links:** an mp3, an ogg and a flac link show the audio player; before Play only the start of the file is read; Save and Open in browser work; a file over 32 MB is not played inline.
- [ ] **Music links:** Spotify, SoundCloud and YouTube Music cards load their player only after **Play**; Apple Music, Tidal, Deezer and Bandcamp show the card with Open in browser.
- [ ] **Previews off:** with _Show link previews and embeds_ off, a hidden image, a player and a link card each leave the one quiet line, and the line opens _Settings → Privacy_. The watch party stays disabled with the reason.
- [ ] **Mumble links:** a `mumble://` link shows the server card with ping and users; **Join**, **Go to channel**, **Add to servers** and **Copy link** work; the query goes only to that server.
- [ ] **Channel menu and header:** right-click and the keyboard menu of a channel; the eye in the channel header follows and unfollows; **Copy channel link** has no user name or password.
- [ ] **Member column:** Ctrl+Shift+M hides and shows it; while hidden the button shows the count and the talking dot.

## 6. Cinema view and screen sharing (two clients, Windows and macOS sharer if available; about 40 min)

- [ ] **Watch together cinema view:** the player, chat, playlist (paste, drag reorder, Alt+Up / Alt+Down), "Who's watching" states and the mini player when leaving the view. Keys Space, arrows, M, F, Z, 1, Esc work. Play, pause and seek stay in sync within about a second.
- [ ] **Old clients:** the official Mumble client and a 0.4.4 Mumcord in the channel see no error and receive no chat messages from the party.
- [ ] **Screen share viewer:** the main-column viewer shows the chips, **Stream details**, volume and level of the shared sound, Fit / Fill / 1:1, Focus. The sharer's control bar (Pause, Sound, Change window, Viewers, Stop) and **Watching (n / 8)** list match what happens.
- [ ] **Pop-out window:** if the pop-out (#164) is part of the release, pop the party and the share out, move them to a second monitor, close and reopen them. Otherwise skip this item.
- [ ] **Screen share sound:** a Windows or macOS sharer's sound arrives clean for 5 minutes without drifting against the picture.

## 7. Community relay (Mumble server you run; about 30 min)

- [ ] **Relay mode:** with a _mumpeer_ relay and its announcing bot on the test server, _Direct + relay_ loads a full-size image and carries a screen share; _Relay only_ hides the own IP address from the other person; _Direct only_ ignores the relay.
- [ ] **No relay:** on a server without a relay everything works as in 0.4.4.

## 8. Servers and identities (about 30 min)

- [ ] **Server dialog:** the four tabs, inline validation that jumps to the field, and **Test connection** (ping, version, certificate trusted or not; nothing saved).
- [ ] **Server info:** the panel and the certificate section (issuer, fingerprint, validity, PEM) are correct, and are translated in a non-English language.
- [ ] **Channel passwords:** saved passwords are listed with the channel path and last use; the password dialog appears once per attempt and not again while someone else sits in a protected channel.
- [ ] **Identity manager:** create, import (PKCS#12), rename, export, set as default and delete (typed confirmation, "Back up first"). Connect to a server with each identity.
- [ ] **Public server browser:** the list loads only after opening it; search, country and ping filters work; **Connect** and **Favourite** work. Only the list is read, nothing is joined except on **Connect** to a server you run yourself.

## 9. Tray, layout and look (Windows, macOS, Linux; about 20 min)

- [ ] **Tray menu:** every connected server with its state, Mute, Deafen, Do Not Disturb, and the voice server and channel submenu. KDE shows the tooltip.
- [ ] **User panel and sidebar:** the panel keeps its height in every state (connecting, reconnecting, error); the wider channel tree fits long channel names; the logo shows correctly as dock, taskbar and Start menu icon and in the installers.
- [ ] **Packet inspector:** it follows new packets, pauses when scrolled up and shows "N new packets"; closing the window stops the capture.
- [ ] **Accessibility:** the new popovers, menus and the settings window work with the keyboard only and are announced by a screen reader (Orca, NVDA or VoiceOver).

## 10. Official Mumble client and other platforms (about 20 min)

- [ ] **Interop:** with the official client (1.5) in the channel: voice both ways, text chat, whisper from Mumcord is heard by the target only, the channel password with the same access token works.
- [ ] **Linux packages:** the AppImage, `.deb` and `.rpm` start, play a YouTube card inline and show the screen share picker on Wayland.
- [ ] **Stage 1a gate and 0.4.0 leftovers:** anything from [docs/manual-checklist-0.4.0.md](manual-checklist-0.4.0.md) not done yet.
