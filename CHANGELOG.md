# Changelog

## [Unreleased]

### Added

- Direct links to audio files (mp3, ogg/opus, m4a/aac, wav, flac, webm) get an inline audio player under the message: play/pause, a seek bar with the time, volume and mute, Open in browser and Save. Before you press Play Mumcord only reads the start of the file to check its format and size (shown on the card); the whole file is fetched on Play, by Mumcord itself and never by the chat view, and files over 32 MB or in another format are refused with a "Can't play this file" message and **Try again**. Nothing plays by itself, only one player under the messages plays at a time, and the sound goes to your output device, never into your microphone or the voice stream. Like the other previews it follows Settings > Privacy "Show link previews and embeds": with it off nothing is fetched (#260)
- Music links get a music card with cover, title, artist or album details and a plain-text badge naming the service. Spotify, SoundCloud and YouTube Music (through the standard YouTube player) play in the service's official embed, which loads only after you press **Play**; Apple Music, Tidal, Deezer and Bandcamp, which have no documented embed, and other pages that say they are music get the same card with **Open in …** instead of a player. The card says which third-party site the content comes from, playback limits (such as previews without a subscription) are up to the service, starting one player stops the other, and nothing is fetched while link previews and embeds are off. The existing Spotify player card became this music card (#262)
- The channel tree is easier to read and use: right-clicking a channel (or pressing the menu key / Shift+F10 on it) opens the same menu as the channel name in the chat header, plus **Join channel** (Shift+Enter) at the top and **Move users here…** for people allowed to move others; items you may not use are left out. It replaces the old "Show channel description" item, as Channel info shows the description. Channels with a user limit show it after the name, e.g. `Raid (1/10)`; a full channel is dimmed, its count turns the warning colour and it shows a "Channel full" tooltip (screen readers hear "Channel full, 5 of 5 places taken"). Temporary and linked channels get icons with spoken labels, and your own voice channel says "you are here" when the column is wide enough. With the move permission you can drag a person onto another channel to move them; the server still checks every move, so it works on any standard Mumble server. The tree starts with a section header for the server instead of "Server overview": a collapse arrow, a connection dot (connected, reconnecting or offline, also read out), the server name in capitals with its ⋯ menu, and "N online · XX ms" below it. (#172)
- Audio settings per channel: **Audio for this channel…** in the channel-name menu (and a **My audio profile** row in Channel info) sets Music quality, low-latency mode and a bitrate cap for one channel, with the quick presets Voice, Music and Jam. Each value can also follow the global setting. The profile applies automatically when you join that channel and your normal settings come back when you leave it; your global settings are never changed. Channels with a music or low-latency profile show a ♪ or ⚡ badge in the header, and the voice quality panel and connection info mark values that come from it, e.g. "Music quality (channel profile)". The server's bandwidth limit still wins. Profiles are kept only on this device, per server, and still match a channel after it is renamed or recreated; nothing is sent to the server, so it works on any standard Mumble server (#171)
- The channel header is tidier: the separate Follow and Clear history buttons are gone. An eye in front of the channel name shows whether you follow the channel (filled when you do) and toggles it with a click. Clicking the channel name (with ▾), or pressing Enter, Space, the menu key or Shift+F10 on it, opens a menu with **Follow / Unfollow channel**, **Copy channel link** (a `mumble://` link without user name or password), **Channel password…** (only for a protected channel you cannot enter yet), **Channel info** and **Clear chat history…** (asks first; history sync still does not bring cleared messages back). Next to the name a line shows the channel path and the start of its description. **Channel info** shows the name and path, the description (sanitised), users and the user limit, permanent or temporary, whether entering is restricted and whether a saved channel password unlocks it, linked channels, your permissions there, the server's quality limit, whether you follow it and when you last cleared its history, with a **Copy link** button. Everything comes from what the server already sends to every client. Private chats keep their Clear history button; the members button stays in the header (#170)
- Settings > About has a **Run setup wizard again** button that goes through the first-start setup once more to review the microphone, the privacy choices, the update check and the talk mode with the start options — without the identity step, so the certificate, the registered name and the permissions on servers can never be changed by accident (the note in the wizard links to the identity manager). Every value is filled in from the saved settings, the step list marks the identity step as skipped and the progress counts four steps. Nothing is written while the wizard is open: **Cancel** keeps everything as it was (it asks first when something would be dropped), and **Finish** saves only the values that actually changed. The microphone test there runs on the freshly picked devices without saving them first (#149)
- The packet inspector follows new packets: the list scrolls down by itself, pauses when you scroll up and shows a "N new packets, jump to latest" button, and has a Follow toggle next to Pause. Packets are now captured only while the inspector window is open, and dropped when it closes, unless the new "Keep capturing in the background" switch (off by default) is on (#169)
- Low-latency mode for playing music together (Settings > Audio, Beta): 10 ms low-delay Opus packets, a minimal jitter buffer, small device buffers (128 frames) and no echo cancellation, noise suppression or automatic volume. It switches live without reconnecting and shows an estimate of the mouth-to-ear latency with its parts there and in the connection info (about 40 ms instead of about 100 ms plus the network on the same machine). It needs more bandwidth and can glitch on Wi-Fi; headphones are recommended. Works with standard servers and official clients (#156)
- Right-clicking the connection info button (or pressing the menu key / Shift+F10 on it, or clicking the small arrow next to it) opens a **Voice quality** quick panel: the bitrate slider with the server limit and the bitrate being sent right now, the Music quality switch (both are the same settings as in Settings > Audio and are saved at once), the estimated bandwidth including packet headers against the server limit, the audio health rating, and a **Voice settings…** link. The Low-latency mode row is shown as "Coming soon" until that mode exists (#155)
- Right-clicking the hang-up button in the voice bar, pressing the menu key on it, or clicking the small arrow next to it opens a menu: **Leave voice** (stop talking and listening but stay connected, so chat and read-along keep working; the server becomes read-only like any other connected one and joining a channel brings voice back), **Disconnect from <server>**, **Switch voice to <server>** (with several servers connected) and **Disconnect from all servers…** (asks first). A left-click still disconnects at once. Leave voice only self-deafens, so it works on every standard Mumble server (#154)
- Saved channel passwords show which channel they are for: the edit-server dialog lists each one as "Officers (Raid)" with the full channel path and when it was last used, instead of "Channel password 1". A password used for several channels lists them all, a channel renamed or removed on the server is marked "(not found)", and each password can be renamed. Only the names and channels are kept in the local database; the passwords stay in the system's secure storage. Passwords saved before keep the name "Password 1", "Password 2" … until they are entered for a channel again (#146)
- Windows releases also ship a portable zip (`Mumcord_<version>_x64-portable.zip`) that runs without installing: unpack it and start `Mumcord.exe`. The `portable` file next to the exe keeps the data, settings, logs and caches in `MumcordData` beside it instead of `%APPDATA%`; it registers no `mumble://` handler and no start at login unless turned on, and About, the diagnostic file and the update hint say "Portable". The identity stays in Windows Credential Manager (per Windows user). A missing WebView2 Runtime now shows a message with the download link instead of failing silently. See `docs/windows-portable.md` (#181)
- A server info panel (server header menu > Server info) shows name and address, version and operating system, welcome text, limits and allowed codecs, your session, registration state and current-channel permissions, how many users run Mumcord, and the connection figures. The TLS certificate section shows the issuer, SHA-256 fingerprint and validity period, has a Show certificate action with the PEM and a Copy fingerprint button; Copy address and Copy diagnostics put the rest on the clipboard for bug reports. Everything comes from messages the server already sends or from local state, so standard Mumble servers need no change (#147)
- Right-clicking (or pressing the menu key on, or using the small arrow beside) the microphone or headphones button in the voice bar opens a quick audio popover; left-click still mutes or deafens. The microphone popover lists the input devices (current one checked, system default marked) with an input volume slider (0–200 %), a live level meter, a noise suppression switch and a link to the voice settings; the headphones popover lists the output devices with an output volume slider (0–100 %), a Test sound button and the same link. Switching applies at once without reconnecting, plugged-in devices appear live, and when a chosen device is unplugged Mumcord falls back to the system default and says so briefly (#153)
- **Do Not Disturb**: one switch in quick settings, in Settings > Sounds & notifications and in the tray menu silences notification sounds (message, mention, watch party and screen share sounds) and in-app or system notifications on all servers at once. Voice keeps working (join, leave and your own mute sounds still play), unread badges keep counting, and the user panel shows "Do Not Disturb · manual" while it is on. It is saved across restarts. **Follow system Do Not Disturb** (on by default) also turns it on while the desktop's Do Not Disturb is on — read live on KDE Plasma and GNOME, and from Focus Assist on Windows — and the user panel then shows "Do Not Disturb (system)". Where the state can't be read (macOS, other desktops), the settings say so and only Mumcord's own switch works (#69)
- The tray menu (menu bar menu on macOS) is complete: it shows every connected server with its state and channel ("Night Owls – Connected · Lobby", "Reconnecting…", or "Not connected" after a dropped connection), Mute, Deafen and **Do Not Disturb** (marked "(system)" while the system's Do Not Disturb is on), a **Voice server and channel** submenu to move voice to another connected server or join a channel (with user counts; very large servers end with a link to the window), a **Recent chats** submenu with the last five private chats (unread ones first) that opens the chat, and Open Mumcord, Settings… and Quit. The tray icon shows the state: greyed out while disconnected, a green ring while you or someone in your channel talks, dimmed with a red slash while muted or deafened, a pink dot for unread messages with the number of mentions (servers with muted notifications do not count), and a small moon during Do Not Disturb. Everything updates live. On macOS a click on the menu bar icon opens the menu; elsewhere a click shows the window and the menu is on right click. On GNOME without the AppIndicator extension there is no tray and closing the window follows the close setting as before (#65)
- Being kicked or banned from a server plays a sound: a short slap when you are kicked, a slapstick groin kick when you are banned. Each has its own switch in Settings > Sounds & notifications (both on by default); like the other sounds they play only while voice is on and not while you are deafened. A normal disconnect, a lost connection or a server shutdown plays neither (#205)
- The add/edit server dialog is grouped in four tabs — Connection, Identity, Passwords, Behaviour — with one component for both, inline validation that names what is wrong and jumps to the field (also across tabs), a warning dot on the tab that needs attention, and a **Test connection** button that reports the ping, the server version and whether the certificate is already trusted, without saving anything or storing a trust decision. New servers can now set "Always connect at start" and a theme too, and an edit reopens on the tab used last (#151)
- Settings > Identity is an identity manager: several client certificates with a local label, their certificate name, fingerprint, creation date and the servers that use them, plus create, import (PKCS#12, added to the list instead of replacing), rename (label only), export, details, "set as default" and delete with a typed confirmation and a "Back up first" button. The first identity is the default, the last one cannot be deleted, and deleting the default promotes the oldest remaining one; deleting an identity never touches chat history, friends or notes. Every server can use the default identity or its own ("Identity…" in the server menu, also in the edit-server dialog), and changing it asks whether to reconnect that server now. The identity in use is shown in the chat header (only when it is not the default), in the connection info, in the user panel's offline name and in the registration dialog, which exports exactly that identity. Certificates and keys stay in the OS secure storage; the database only holds labels, names, hashes and dates. The single identity of older versions becomes "Default" on the first start and every server keeps connecting as before (#148)
- A `mumble://` link in a chat message gets a card under it: the server name (or host), the channel path, the ping, the users online and the user limit, and the server version, with **Join** (or **Go to channel** when that server is already connected), **Add to servers**, **Copy link** and a Hide preview button. Saved servers show their saved label. The figures are the answer to the standard public server query, never a connection, and are only requested while link previews are on in Settings > Chat & history; with them off the card says so instead. A link that carries a user name or password is warned about, never shows them, and Copy link leaves them out. A server that does not answer shows "Server not reachable" with **Check again**, a link that is not a server link gets no card, and one message shows at most three cards (#175)
- Settings > Privacy > **Private messages from: Everyone / Friends only / Nobody**, also in the quick settings, with a per-server override and two exceptions: people in your current channel, and conversations you already accepted (you wrote to them first, replied, or accepted their request; listed per identity in Settings > Privacy, where you can revoke them). Mumble servers still deliver every message, so Mumcord filters on your side and never tells the sender. With "Friends only", messages from standard Mumble clients wait under **Message requests** at the top of the channel list (or are dropped, if you turn that off), where you can read them and accept, decline or add the sender as a friend; other Mumcord users can send you a **chat request** with a short note instead (person menu > Send chat request…). Accept lets them write to you and tells them; Decline is silent. Requests are kept in memory only and are gone after a disconnect or when you quit Mumcord.
- The member column on the right can be hidden so the chat uses the full window width. A members button in the chat header hides and shows it and is highlighted while the column is open; while the column is hidden it shows how many people are in the channel and a green dot while someone there talks, and its tooltip names who ("Freya is talking") and the shortcut. Ctrl+Shift+M (Cmd+Shift+M on macOS) does the same, and the choice is remembered. In a narrow window the column is hidden and slides in over the chat below the header; the same button, Escape or a click beside it closes it. The column animates its width unless reduced motion is on (#165)
- `@channel` and `@all` mentions: in a channel, typing `@` suggests `@channel` (everyone in the channel) and `@all` (everyone who can read the channel, people listening to it included) above the people. They are sent as plain text, so other Mumble clients just see the words. Mumcord highlights them and treats them like a personal mention (highlighted message, ping, notification and mention count); `@channel` only counts for people in the channel. Anyone may use them; if they get too much, turn them off per server with **Mute @all and @channel** in the server's menu.
- A **Messages** button at the top of the server bar opens the private messages overview: every conversation from every connected server in one list, pinned ones first, then the newest. Each row shows the online dot, the name, a small server tag, a plain-text preview, the time and the unread count (grey when muted). Search (Ctrl+K), the filters All / Unread / Requests with live counts, **Start a new conversation** (anyone on any connected server), message requests on top (more than one collapse to "Show n more requests"), and a conversation menu (right-click, menu key or Shift+F10) with Mark as read, Pin to top, Mute (1 hour, 8 hours, until turned back on), View profile and Delete history (this device only, after a confirmation). Pin and mute are saved; muted conversations raise no toast or sound. Blocking is shown but not available yet (#173)
- The person menu (right-click a person in the channel tree, the member list, the chat or the friends list) starts with **Send message**, **Join ‹channel›** and **Add friend** / **Remove friend**. Send message opens the conversation, or the chat request dialog where the private message rules need one first; while a request is pending it is greyed out with "Chat request pending". Join is hidden when you are already in that channel and greyed out with the reason when the channel is full or you may not enter it (no permission or a channel password) (#219)
- **Revoke chat consent…** in the person menu and in the conversation menu of the private messages overview, for people whose conversation you accepted: after a confirmation they need to send a new message request before their messages reach you again (#219)
- **Import from Mumble**: when the official Mumble client (1.4 or 1.5, on Linux, Windows or macOS) is installed, the setup wizard starts with an optional step that copies your server favourites, their saved passwords, your client certificate (as a new identity, so you keep your registered name), your audio devices and your push-to-talk key; volume and voice settings can be copied too but are off by default. Settings > Servers > **Import from Mumble…** does the same later. A preview lists what was found with a checkbox for each part and a **Review list** of the servers; servers already in Mumcord are skipped, never overwritten. A certificate protected by a password asks for it. Passwords and the certificate go straight into the system's secure storage, never into a file. Mumble's own files are only read, never changed, and nothing leaves your computer. The summary shows what was added and skipped and has **Undo import** (#66)

### Changed

- The "Direct messages" list below the channel tree is gone: the private messages overview (Messages button at the top of the server bar) shows every conversation with its unread count and opens it with mouse or keyboard (#219)

- A new **search button in the server bar** opens the public server browser: the list of public Mumble servers the official Mumble client shows. Search by name, address or country, filter by country and ping, hide empty or full servers, and sort by name, country, ping or users. **Connect** saves the server and connects in one click; **Favourite** adds it to your servers. For privacy the list is loaded only when you open the browser, never in the background, and only the rows on screen are pinged (with the standard Mumble ping, one every 50 ms, a few at a time). Everything in the list is treated as untrusted and shown as plain text.

### Fixed

- Your own messages never play the mention sound or show a notification, even when the server or another session of yours sends them back; they also no longer count as unread (#215)
- When the desktop has Do Not Disturb on, the user panel no longer shows the long "Turn it off in your desktop's notification settings." line under "Do Not Disturb (system)": a small "?" next to it shows the hint on hover and keyboard focus, like the other explanations in the panels (#217)
- The server info dialog is now translated in all 24 languages: its labels, permission names, trust status and limits were English everywhere because the English text had been copied into every language file (#207)
- The watch party respects Settings > Privacy > "Show link previews and embeds": with it off, "Watch together" in the Apps menu stays listed but disabled with the reason and a link to the Privacy settings, a running party shows "A watch party is running: enable embedded players to join" with the same link instead of the banner's video title, the floating player and playlist (no player page, no thumbnails) — so nothing is requested from YouTube or Twitch (#160)
- Linux with PipeWire: the device lists show every output and input of the sound server as the desktop's sound settings do, so a multi-endpoint interface such as the RØDECaster Pro II offers "System" and "Chat" as outputs and "Microphone" and "Main" as inputs, with their friendly names, instead of one "USB Audio" entry per direction. The choice is stored by the stable PipeWire node name and the stream plays to exactly that node; hot-plugging updates the list and a missing device falls back to the system default. A device saved from the old list moves to the matching node when there is exactly one, otherwise to the system default with a one-time notice. Without PipeWire the ALSA devices are listed as before (#183)

### Internal

- The member-column E2E spec now hovers the members button from outside it and re-hovers until the tooltip opens, and keeps the test bot talking long enough that the checks cannot run after the short sample audio ended. The tooltip layer only reacts when the pointer enters a trigger, and a click keeps that trigger's tooltip away until the pointer leaves it, so a single `moveTo()` could wait forever — the flake in CI (#216). Two unit tests pin that behaviour (click and leave, page scroll).

## [0.4.4] - 2026-10-30

### Added

- Settings > Privacy shows how much space the caches use (link previews and images, the web engine cache, temporary files) and has a Clear cache button with per-category checkboxes. A confirmation says that chat history, settings, servers, certificates, friends and notes are never touched. The web engine cache is removed at the next start so that the stored settings stay safe (#116)
- Code blocks in chat messages have Copy and Download buttons in their corner, shown on hover and keyboard focus: Copy puts the plain code on the clipboard, Download saves it as `snippet.<ext>` named after the detected language (`.txt` if unknown) (#111)
- Code blocks keep the language written after the fence (for example `sql`, `rust` or `php`) and are highlighted as exactly that language, with the language shown in the block's corner. New languages: PHP, Ruby, Kotlin, Swift, Lua, PowerShell, Windows batch (`cmd`), Dockerfile, TOML, Markdown, Makefile, nginx, plain text and hex dumps (`hex`: offset, bytes and ASCII column), plus aliases such as `sh`, `py`, `rs`, `js`, `ts`, `c++`, `bat` and `yml`. Unknown languages stay plain text with their name. The language travels as a `data-mumcord-lang` attribute that standard Mumble servers pass through; code from other clients is still detected automatically. See `docs/code-blocks.md` (#112)
- Right-clicking an image in the chat (sent images, full-size images, inline external images, GIFs and message-card images), or pressing the menu key on it, offers **Copy image**, **Save image…**, **Copy image address** (external images) and **Open in image viewer**; the image viewer has Copy and Save buttons too. Copy puts the image on the clipboard as image data (a GIF as its first frame), Save writes the original file (the full-size version once it has arrived, a GIF with its animation) under its own name or `image-<date>.<ext>`. Both refuse images above 50 MB with a clear message, and an external image that was not loaded offers only its address, so nothing is fetched (issue #115)
- About > Show third-party licences opens the licences in a window of their own, which remembers its size, follows the theme and language and closes with Ctrl+W: one collapsed row per component (`name version — licence`, tagged Rust, npm or native, including libvpx, Opus, SQLite and the AppImage's GStreamer and WebKitGTK) that expands by click, Enter or Space to the full licence text with a Copy button and the repository link, a search by name or licence, a licence filter and Expand all / Collapse all. The header names Mumcord's own licence (AGPL-3.0-only) with a link to the source code. The notices are generated as `ui/public/third-party-notices.json` as well, with each licence text stored once (#138)
- About > What's new opens the changelog in a window of its own instead of the website, and it opens by itself once after an update (Settings > About > Show “What's new” automatically after updates): one collapsible section per version with its date and a one-line summary, the current version open, Added / Changed / Fixed groups in their own colours, the developer-only Internal group collapsed, search across all versions, Expand all / Collapse all, and issue numbers and release pages opening through the link menu. It remembers its size, follows the theme and language and closes with Ctrl+W; dev builds show the version in development and never open it by themselves (issue #145)

### Changed

- Connection info rates the audio as Good, Fair or Poor over the last two minutes, with an icon, a colour and a one-line reason that names the likely cause (network loss, high CPU load or the audio device). The raw counters moved into a Details section that stays collapsed until opened (remembered), with an explanation of each counter, what is normal, the figure for the last two minutes and small trend graphs. The diagnostics file and the packet inspector's voice counters show the rating and reason too. The counters no longer count normal silence: concealment only counts while someone's voice stream is running (not the end of every speech burst), late frames and playback underruns only count while sound is being sent or played, and the jitter buffer shows the last speaker's delay between bursts instead of 0 ms (#152)
- The post-release changelog step no longer adds a second empty "Unreleased" section when the next version already has an open "X.Y.Z - Unreleased" section, and the release notes fall back to a plain "Unreleased" section
- After a release is published, a workflow opens a pull request that writes the released version into the manifests and lock files and regenerates the third-party notices and dates the changelog section, so `main` no longer stays on the previous version and a release needs no pull request beforehand; the release notes take their "What's new" from the changelog

### Fixed

- Linux: Mumcord no longer gets killed (SIGXCPU) when a voice thread at real-time priority runs past its time budget, for example with noise suppression in a development build. An overrunning thread now drops back to normal priority and audio keeps running; the budget leaves room for bursts (half of rtkit's hard limit instead of one 10 ms frame), the capture thread rests briefly while catching up on a backlog, development builds no longer use real-time scheduling, and they build the voice processing crates optimised (#139)
- The Copy buttons on code blocks, the packet inspector, the theme share action and the OpenGL launch option now copy through the app's own clipboard access, so they work on Linux where the web engine's clipboard fails (issue #134)
- Selected chat text stays highlighted while its right-click menu is open and after choosing Copy, instead of looking unselected; it clears when you click elsewhere (issue #135)
- Voice no longer crackles or drops out as easily when the computer is under heavy load: Mumcord's capture and playback threads, and the audio device threads, ask the system for real-time scheduling (rtkit on Linux, MMCSS on Windows, a time-constraint policy on macOS; without it they keep running at normal priority), and a buffer under- or overrun of the audio device no longer restarts the whole audio stream. The connection info and the diagnostic file show audio dropouts, late audio frames, concealed frames, the jitter-buffer delay, the processing time and whether real-time audio is on (#123)
- Deleted chat history no longer comes back through history sync when you rejoin: clearing a channel, a server or all history (and the automatic deletion after N days) now also limits what is fetched from other Mumcord users' copies, and the confirmation says that others keep their copies (issue #119)
- In the compact chat layout, the shared theme card no longer draws its title over the thumbnail, and message cards and image transfer bars no longer shift left; the theme card fits its content and shrinks cleanly on narrow windows (#121)
- Large GIPHY GIFs no longer slow the whole app down: the chat now shows GIPHY's downsized GIF, sized for the chat box, instead of the full-size WebP, which kept the renderer about four times as busy while it played. GIFs already cached from earlier messages are fetched again in the lighter format, and moving the mouse over a large GIF no longer keeps a CPU core busy (#109)
- Pasting copied image data, such as a Spectacle screenshot or "Copy image" in a browser, attaches the image again on Linux; the WebKitGTK bundled in the AppImage hands such a paste nothing, so the image is now read from the system clipboard (issue #110)
- Selected chat text can be copied again: with text selected, the chat's right-click menu starts with **Copy**, before the message actions, and Ctrl+C copies the same plain text with its line breaks but without hidden labels, the hover time or button text. Before, the message menu replaced the webview's own menu and offered no Copy at all (issue #114)

### Internal

- Published releases are fingerprinted and re-checked daily: `releases/<version>.json` (tag commit, asset sizes and SHA-256, the version's changelog section, crates.io checksums) is added to the post-release sync pull request, and the new Release integrity workflow opens an issue when a tag, asset, changelog section or crate no longer matches. A pull request that edits the changelog section of an already released version fails the new "Released changelog sections" CI check. The manifests for v0.4.1 to v0.4.3 are backfilled (#144)
- CI has a fast "Lint workflows and scripts" job that checks the GitHub workflows with actionlint and every shell script with shellcheck (pinned versions), also for pull requests that only touch workflows or scripts. `scripts/ci/lint-ci.sh` runs it locally; existing findings in the scripts are fixed (#104)

- CI checks dependencies for known security advisories, disallowed licences and unknown sources with cargo-deny (`deny.toml`, same licence list as the third-party notices) and runs `npm audit` for production packages, on every pull request and weekly on `main`. Accepted advisories are listed with a reason and an issue (#103)

## [0.4.3] - 2026-10-28

### Added

- The Mumble protocol library `mumcord-protocol` and the voice audio library `mumcord-audio` are now published on crates.io with every release, so other projects such as the bot can depend on them by version
- The voice library is split: the new crate `mumcord-codec` holds everything that works without audio devices (Opus codec, jitter buffer, mixer, bitrate control, processing chain) and has no cpal or ALSA dependency, so headless bots can build it for static musl targets; `mumcord-audio` keeps the device I/O and re-exports the codec, and both are published on crates.io
- Message cards: bots and people can post a compact card with a title, a status colour (info, success, warning, error, always also named in text), a built-in icon, chat-formatted text, an image and a link by writing `[card title="…" color=warning icon=chart image="…" url="…"]text[/card]`. Official Mumble clients see the same message as readable formatted text; images load only with link previews on, through Mumcord like other previews. See `docs/message-cards.md` (#72)
- The local storage library `mumcord-store` (database, identity certificates, OS secure storage) and the direct-connection library `mumcord-p2p` (QUIC with certificate pinning) are now published on crates.io with every release too
- A hidden packet inspector for debugging Mumcord's plugin data (issue #60): seven clicks on the version in Settings → About unlock it, and **Open packet inspector** in Settings shows the sent and received packets in a window of its own, with filters, counters, decoded content and JSON or Markdown export. The window remembers its size and position, follows the app's theme and language, closes with Ctrl+W, and can only read the capture; capturing runs while the developer tools are unlocked, whether the window is open or not

### Changed

- The "couldn't connect" banner now hides after 60 seconds (hovering or focusing it pauses the timer) and has a dismiss button; it comes back only for a new failure, and the server icon keeps showing the failed connection. Servers without a name are shown as host and port
- The Linux install script no longer tells you to install gst-libav (H.264 video support) when it is already installed; when it is missing it asks whether to install it right away
- The Linux install script got a new look: a Mumcord banner, numbered steps with a download progress spinner, clear success and warning marks and a summary at the end; logs, pipes, `NO_COLOR` and `TERM=dumb` get plain text

- The Linux setting for running on X11 now says "Compatibility mode (X11)" instead of naming other products

### Fixed

- Pasting an image from the clipboard into the message input works again, including images copied from screenshot tools such as Spectacle (#50)
- Disconnecting, removing a server or quitting right after sending no longer loses the last chat message, status change or plugin data that was still waiting for the server's rate limit: the connection now sends what is queued first (at most 2 s) and quitting waits for it. `mumcord-protocol` gains `Handle::flush` and `Handle::closed` for the same guarantee in other programs (#74)
- Pasting an image file copied in the file manager (for example right-click → Copy in Dolphin or Files) into the message input now attaches it like a dropped image, with the same preview, size limit and "Only images can be sent" message for other files; on Linux the webview hides the copied files from the page, so Mumcord reads them from the system clipboard itself and accepts only regular image files up to 25 MB

### Internal

- The repository moved to the Mumcord organisation on GitHub and all links point to `Mumcord/mumcord`
- The test bot now lives in its own repository, `Mumcord/mumbot`, and CI uses its pinned release instead of building the bot crate; the webhook bot documentation moved there too
- The README is rewritten, and mentions of other voice chat products are removed from the docs
- Updated dependencies: `rubato` 5, `rusqlite` 0.41, `rustls-platform-verifier` 0.7, `sha2` 0.11, the npm and WebdriverIO test packages and the GitHub Actions used by CI
- The version is set at build time instead of by a version-bump commit: release builds show the tag's version, CI builds `dev~<short commit>` and local builds `dev~YYYYMMDD` (plus `-dirty` with uncommitted changes) in About, the diagnostics file, the log and the update check; dev builds no longer look for updates. A release pull request now only edits `CHANGELOG.md` and, for feature releases, the release notes block in `release.yml`
- Every release ships a software bill of materials (SBOM) per package type, `mumcord-<version>-<platform>.cdx.json` in CycloneDX format, listing the Rust crates, npm packages and bundled native libraries (libvpx, libopus and, in the AppImage, WebKitGTK, GStreamer and its plugins); the SBOMs are listed in `SHA256SUMS`, and all release files get a GitHub build provenance attestation
- The third-party notices are generated by CI instead of by hand: each release regenerates them from the tagged commit before bundling, so About → Third-party licences is always correct, and after dependency changes on main a workflow opens a pull request with the updated files
- Generating the notices fails when a dependency uses a licence that is not on the allowlist (`about.toml` for Rust crates, the script for npm packages), so an incompatible dependency is noticed at once; the licence and SBOM tools are pinned in `scripts/license-tools.env`
- The AES library is updated to 0.9, which now uses the hardware-accelerated path by default on modern x86 CPUs; the voice crypto code follows the renamed API of its cipher layer
- `bindgen` stays at 0.72: the 0.73 series needs `prettyplease` 0.3, which `prost-build` does not accept yet, so the two cannot be resolved together; Dependabot no longer proposes that bump until `prost-build` allows it
- The D-Bus library `zbus` is updated to 6.1, which merged the `zvariant` crate into it: proxy support has to be switched on with the `proxy` feature, `Value` and `OwnedValue` are renamed to `Variant` and `OwnedVariant`, and the proxy builder no longer reports a bad name or path at the setter but when the proxy is built; the desktop notification and MPRIS code follows the new names
- The random number library `rand` is updated to 0.10, which drops the duplicated `rand` and `rand_core` trees from the dependency graph; the network simulator now imports the trait that provides range sampling, renamed from `Rng` to `RngExt` in that version

## [0.4.2] - 2026-10-27

### Fixed

- Fixed YouTube videos in watch parties showing "This video can't be played in your browser" in the Linux AppImage: the AppImage now ships the Opus audio parser YouTube playback needs
- People who join a voice channel while someone is sharing their screen or a watch party is running now see it more reliably: the announcement is sent a second time in case the server dropped the first
- Fixed a flood of keychain prompts on macOS after updating: a denied or failed keychain read is no longer repeated for every server list refresh, notification or reconnect, and the identity is read only once per session

### Internal

- Dependabot now checks dependencies and GitHub Actions weekly, with grouped and blocked major updates
- Updated the `rtrb` and `x11rb` libraries and the GitHub Actions used by CI and releases
- CI switches to another Ubuntu package mirror when apt stalls

## [0.4.1] - 2026-10-26

### macOS

- Fixed connection failures: Mumcord now asks for the Local Network permission, without which macOS 15 and later blocked connections to servers in your home network
- Clear reasons when a connection fails, in the app and in the log (for example "No route to host" with a pointer to System Settings → Privacy & Security → Local Network)
- Far fewer keychain prompts: each saved secret is read only once per session and Mumcord uses fewer keychain items (items from older versions are merged on first use)
- The microphone prompt now appears only once, when the microphone is first used, not when listing devices in the setup wizard or settings
- The diagnostics page lists audio devices again, even when no audio is running
- Signed and notarized builds when signing is configured; other builds get a stable signature so macOS can remember your choices

### Fixes

- Fixed two servers failing at the same time: their password and certificate prompts no longer collide

### Internal

- Faster CI builds and end-to-end tests

## [0.4.0] - 2026-10-26

_Watch party and screen sharing are **Beta**. The window overlay is removed: on Windows the new in-game overlay replaces it, on Linux the in-game overlay stays; people who had it switched on see a one-time notice after updating._

### Multi-server

- Stay connected to up to 5 servers at once (configurable 1–15): voice on one, chat and notifications on the others, with unread and mention badges in the server bar
- Move your voice to another server by joining one of its channels; "Join channel" for a friend on another server does the same
- Connect servers automatically at start ("Always connect at start"), reconnect only the server that dropped, and mute notifications per server
- Private messages, friends, history sync, now playing and now gaming work on every connected server
- Ask again for the password when a server rejects it, with "Save password"
- Channel passwords: a lock marks protected channels, joining asks for the password once and remembers it per server (manage it in the edit-server dialog); works with the access tokens official Mumble clients use

### Themes and look

- Colour themes with presets shown as gradient circles, your own themes (save, rename, delete) and an editor with a Preview button
- Every theme has a dark and a light version; Dark / Light / System picks the version, the second one is generated automatically and can be fine-tuned
- Share a theme as a code: other Mumcord users get a preview card in the chat with "Apply theme"; set a theme for one server only
- The website look: glowing primary buttons, gradient accents, glass popovers and film grain, with a "Visual effects" switch (off automatically with reduced motion or transparency)
- Themed dropdowns everywhere and a consistent settings layout

### Chat

- Reply to messages, with a reply header that jumps to the original; official clients see a readable quote. A reply to your message counts as a mention
- "> text" sends a quote, also with the new Quote button
- Messages appear at once while sending, greyed until confirmed, with Retry and Delete if sending fails; messages the server refuses are marked with the reason
- Sticky author banner while scrolling a long message, and the avatar now sits at the top of tall messages
- Copy or open links from a right-click menu
- Dismiss the welcome message and channel descriptions (they come back when their text changes)
- Rich-text comment editor with bold, links and lists
- A progress bar under chat images while the full-size image loads; the transfer keeps running when you close the viewer, and × cancels it

### Watch party (Beta)

- Watch YouTube together with the Mumcord users in your voice channel: "Watch together" from the Apps menu next to the image picker, or from a YouTube card
- Everyone can play, pause, seek and skip; a shared playlist with reorder and auto-next; people who come in later start at the current position
- The party runs in a floating widget you can move, resize and collapse; a banner at the top of the channel chat invites the others ("Join"); nothing is posted into the chat
- Official Mumble clients see nothing of the party

### Screen sharing (Beta)

- Share a window or your screen with up to 8 Mumcord users in your voice channel, at up to 720p30; quality drops automatically to 480p15 when your upload is too slow
- Watch in a floating widget or full screen; a red "You are sharing your screen" bar with "End" and a LIVE ring on your avatar show that you are sharing
- Sound: on Windows (10 version 2004 or newer) and macOS (13 or newer) viewers hear only the shared window's or app's sound, never the voice chat. Sharing a whole screen on Windows sends no sound, and on macOS 12.3 and 13 the whole main screen is shared. Linux sends no sound
- Linux: on Wayland the system picker chooses a window or screen; on X11 only the whole screen can be shared
- Needs a direct connection between sharer and viewer. Without IPv6 or a port mapping on the home router (for example behind carrier-grade NAT), watching is not possible for that pair, and Mumcord says so after a short wait
- Auto-ducking lowers watch-party and screen-share sound while people talk (setting and slider in Settings → Audio); optional sounds for party and share activity

### Direct connections

- Mumcord users can connect directly to each other (asked once before first use, changeable in Settings → Privacy). Used for screen sharing and faster full-size images; no outside server is involved

### Connection info

- Ping, packet loss, bandwidth and UDP/TCP in the profile card and the person menu ("Connection info…"); admins also see the other person's client and operating system
- A signal icon next to people with a weak connection
- A bitrate slider up to the real maximum the server allows

### Big servers

- Calmer talking rings, a "Syncing big server…" state and much faster switching between big servers
- "Hide empty channels" and "Show talking people first" switches
- Listeners appear inline in the member list with an ear icon ("Listening only")

### Overlay

- In-game overlay on Windows: a click-through window over borderless and windowed games, with nothing loaded into the game, so it is safe with anti-cheat. Games in exclusive fullscreen hide it
- HDR10 games show correct overlay colours; games from Flatpak Steam get the overlay on Linux
- Linux OpenGL games: Settings → Overlay shows a Steam launch option to copy (issue [#11](https://github.com/Mumcord/mumcord/issues/11))

### Audio

- Per-person volume works across the whole range and changes live while dragging
- Music quality is no longer quieter than speech

### Packaging and desktop

- Linux packages: `.deb`, `.rpm`, AUR `mumcord-bin` and AppImage, each for x86_64 and ARM64, plus a one-line curl installer
- App-menu entry, `mumble://` links and an optional desktop icon; the AppImage asks before adding itself to the app menu
- "Start Mumcord when I log in" on Linux, Windows and macOS
- Optional "Check for new versions" hint with a link (nothing is downloaded)
- Linux tray tooltips
- The AppImage bundles the GStreamer plugins for YouTube (VP9) but not H.264; install gst-libav on the system to get H.264 (see README)

### Removed

- The window overlay and its KDE helper and tray item (see the note above)

### Fixed

- Closing or updating Mumcord no longer crashes a running game that shows the in-game overlay
- Ping no longer shows 0 ms for other people
- Chat messages show the sender's real avatar after rejoining
- "Now gaming" can join the game's server from the Mumble Link context
- Flickering chat hover and talking rings on Linux (Mumcord sets `WEBKIT_DISABLE_COMPOSITING_MODE=1`; see README)

## [0.3.1] - 2026-10-25

### Fixed

- Keep voice packets within Mumble's size limit with Music quality on, so others keep hearing you on servers with a high bandwidth limit ([`740f784`](https://github.com/Mumcord/mumcord/commit/740f7843773e4d91e42dfc1301ad5dc88836ce16))
- Open links in the system browser again from the Linux AppImage ([`745fe60`](https://github.com/Mumcord/mumcord/commit/745fe6058bf9c774fe697414f209db36d857a66e))

## [0.3.0] - 2026-10-25

_Not in this release: the in-game overlay on Windows (which uses the window overlay until then) and any overlay on macOS._

### Changed

- Show only talkers in your own channel in the overlay ([`8c6ddd8`](https://github.com/Mumcord/mumcord/commit/8c6ddd83d7e5c2af6f61d73c53728e94bf50cb50))
- Make the settings sidebar sticky ([`2a57f30`](https://github.com/Mumcord/mumcord/commit/2a57f3058d2499df0cfdfba14e815060cd29e663))

### Added

- Add in-game overlay on Linux that shows who is talking inside your game (Vulkan layer, works with Steam and Proton), switched on per game and off by default for games with anti-cheat ([`dfd2e1f`](https://github.com/Mumcord/mumcord/commit/dfd2e1f7bf07f2300041f3c60d0b76f1d4f1db5b))
- Add window overlay, a click-through window that shows who is talking in your own channel, with a helper for KDE ([`cca1731`](https://github.com/Mumcord/mumcord/commit/cca1731866a76cad5e4729d4dc40214fef5f1f70)) ([`f7d9dfa`](https://github.com/Mumcord/mumcord/commit/f7d9dfafe173e4d8037efb8de8cc83f2caae1c8a))
- Add "Now gaming" to share the game you are playing ([`b31e246`](https://github.com/Mumcord/mumcord/commit/b31e24663e4c6e8c151db0b9bba938027a41499e))
- Add quick settings with a collapsible Beta group, settings search and accessibility switches ([`62a93cc`](https://github.com/Mumcord/mumcord/commit/62a93ccd3295cca5d23326edf640f03cbba97d31)) ([`2a57f30`](https://github.com/Mumcord/mumcord/commit/2a57f3058d2499df0cfdfba14e815060cd29e663))
- Add person menu with switches, icons and "Join channel", and redesign the full profile with a private note ([`bb832c1`](https://github.com/Mumcord/mumcord/commit/bb832c1cc98b727b7895ddab8a55f510a6834d9a)) ([`a011704`](https://github.com/Mumcord/mumcord/commit/a0117046985453878400995e4dd9d35a77a24383))
- Add friends with online state, shown in the right column with channel info, and keep private chats ([`2885748`](https://github.com/Mumcord/mumcord/commit/2885748183d5f29c12c9733d3a4575c1ca38d3e9)) ([`93c71e6`](https://github.com/Mumcord/mumcord/commit/93c71e6c8ffb3dfbc1d211ed4935b2ed7d7cc910))
- Add history sync between Mumcord users ([`c2b9e6d`](https://github.com/Mumcord/mumcord/commit/c2b9e6d0e4eaf6857a8758a576a3eec35693f3bf))
- Add full-size images on demand (Beta), with switches to offer and load them ([`2a60276`](https://github.com/Mumcord/mumcord/commit/2a602761c32917e2e902abb61c5a7f5523e12546)) ([`02dcac8`](https://github.com/Mumcord/mumcord/commit/02dcac88e8db2b767e74d47b2d31d889862031a0))
- Add dropping image files into the chat ([`34d4448`](https://github.com/Mumcord/mumcord/commit/34d44482938612bc486f24b07c8262bab9a7337a))
- Add positional audio with Mumble Link and the official game plugins (Beta) ([`af3a7d1`](https://github.com/Mumcord/mumcord/commit/af3a7d177dbeb097ca6c7322070f02eb1cb1dfb6)) ([`c952a47`](https://github.com/Mumcord/mumcord/commit/c952a47467c5eb90d5f518ed57c5219b67a45571))
- Add all 24 official EU languages, including the tray menu, not yet reviewed by native speakers (send corrections via the "Report a bug" issue template) ([`76f9e48`](https://github.com/Mumcord/mumcord/commit/76f9e487f62e19ed398192eb8fd9f5d3eb3f6fab)) ([`e5c1391`](https://github.com/Mumcord/mumcord/commit/e5c13919b58ee9dc498e082765902597b41787b1))
- Add privacy step to the setup wizard ([`0ec61bd`](https://github.com/Mumcord/mumcord/commit/0ec61bd3cc2dab8300cc0b81b53ce1fffc445b36))
- Add changelog and support links to About ([`eb5ab93`](https://github.com/Mumcord/mumcord/commit/eb5ab933d96b85442d9fc48957cc94c004438667))
- Add collapsible member sections that show people in other channels and remember their state, and a server label above the channel tree ([`e97ef66`](https://github.com/Mumcord/mumcord/commit/e97ef6610d60e1d7d8c049f92f731f0721c8a393)) ([`169361f`](https://github.com/Mumcord/mumcord/commit/169361fbd2db873829392ea9b1abe37777896941))
- Add Windows signing with Azure Artifact Signing and macOS signing and notarization when the signing setup is active ([`8d6110c`](https://github.com/Mumcord/mumcord/commit/8d6110c75f261d42c4f381f459970f7895667e46)) ([`1631237`](https://github.com/Mumcord/mumcord/commit/1631237a1dc76dabbdb4d888ee48690646725dac))

### Removed

- Remove the commercial license offer, Mumcord stays licensed under AGPL-3.0 only ([`5f2c74a`](https://github.com/Mumcord/mumcord/commit/5f2c74ab27e3eddafab721769ac10a674a2edb9d))

### Fixed

- Report failed joins from the person menu ([`9368e18`](https://github.com/Mumcord/mumcord/commit/9368e18ce384a97ac31444d0c12f8b4f2af67848))
- Show the correct overlay state for configured games and refresh the Vulkan manifest on every start ([`d481dda`](https://github.com/Mumcord/mumcord/commit/d481ddab454ecad719b09767d0f5b58789f9ae1a)) ([`fa55bcb`](https://github.com/Mumcord/mumcord/commit/fa55bcba5410baa47c70fb55a1e1a14e29d9f4e9))
- Fix a deadlock in the engine and a scroll problem in the wizard ([`835dfff`](https://github.com/Mumcord/mumcord/commit/835dfffac6cc186ace699ff912ad99fd714332f7))
- Make the user panel name clickable again ([`4943baa`](https://github.com/Mumcord/mumcord/commit/4943baaa752347d90c9bde4078d8835134891834))

## [0.2.0] - 2026-10-25

_**License change:** Mumcord is licensed under the GNU AGPL-3.0 only from this version, while versions up to and including 0.1.1 were released under the MIT License and stay MIT._

### Changed

- Use a gear icon for the settings button ([`4134e84`](https://github.com/Mumcord/mumcord/commit/4134e847a5bcbca763d830bef068e21f18e6086b))

### Added

- Add chat image viewer with zoom, pan, navigation and save ([`94962d9`](https://github.com/Mumcord/mumcord/commit/94962d9efb0dce908d1b7fe90444f111a2f9b93e))
- Add inline external images with dismissable previews and automatic clean-up ([`9f1575d`](https://github.com/Mumcord/mumcord/commit/9f1575d41186e96ff697b15df32851972d8cd77f))
- Add Spotify link cards with a click-to-load player ([`292f1a3`](https://github.com/Mumcord/mumcord/commit/292f1a3823a19264b51a8f7b7178139e69441990))
- Add GIF favorites, Giphy and Tenor links with a source credit, and image-only messages without the URL ([`8a154ec`](https://github.com/Mumcord/mumcord/commit/8a154ec8d9c5baa279b916eb8006884a14a452b8)) ([`80df935`](https://github.com/Mumcord/mumcord/commit/80df9350bbe603d5975534a3503990097d466b46))
- Add "Now playing" to share the track you are listening to ([`7d9f0c4`](https://github.com/Mumcord/mumcord/commit/7d9f0c48aff81a86424e32febd85168452b66e5e))
- Add issue templates for bug reports and feature requests ([`911a0da`](https://github.com/Mumcord/mumcord/commit/911a0daaf972caf4f8bbbc9baba146dfbb8953e0))

### Fixed

- Center the settings gear icon ([`4b34dc8`](https://github.com/Mumcord/mumcord/commit/4b34dc84304a5aae62b0528fb998029338ed65db))
- Show the own track everywhere ([`246057e`](https://github.com/Mumcord/mumcord/commit/246057eb3ee44bf45a8b9a43fc8929c5a60a26b2))

## [0.1.1] - 2026-10-25

### Added

- Add music quality switch to the audio settings, with a higher bitrate cap ([`e4d870f`](https://github.com/Mumcord/mumcord/commit/e4d870f283fa35a8c04652b9333bca38141d0b7a)) ([`1311379`](https://github.com/Mumcord/mumcord/commit/1311379c5d7d612f8cc0ffc0389eb8ca2b3d35b2))

### Fixed

- Fix release build failing on a stale smoke test lockfile ([`e5972ee`](https://github.com/Mumcord/mumcord/commit/e5972eecf291b2234514d87cc505ce29bb7bf3f7))

## [0.1.0] - 2026-10-25

_First release._

[0.4.4]: https://github.com/Mumcord/mumcord/releases/tag/v0.4.4
[0.4.3]: https://github.com/Mumcord/mumcord/releases/tag/v0.4.3
[0.4.2]: https://github.com/Mumcord/mumcord/releases/tag/v0.4.2
[0.4.1]: https://github.com/Mumcord/mumcord/releases/tag/v0.4.1
[0.4.0]: https://github.com/Mumcord/mumcord/releases/tag/v0.4.0
[0.3.1]: https://github.com/Mumcord/mumcord/releases/tag/v0.3.1
[0.3.0]: https://github.com/Mumcord/mumcord/releases/tag/v0.3.0
[0.2.0]: https://github.com/Mumcord/mumcord/releases/tag/v0.2.0
[0.1.1]: https://github.com/Mumcord/mumcord/releases/tag/v0.1.1
[0.1.0]: https://github.com/Mumcord/mumcord/releases/tag/v0.1.0
