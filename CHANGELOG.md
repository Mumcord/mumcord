# Changelog

## [Unreleased]

### Added

- Windows releases also ship a portable zip (`Mumcord_<version>_x64-portable.zip`) that runs without installing: unpack it and start `Mumcord.exe`. The `portable` file next to the exe keeps the data, settings, logs and caches in `MumcordData` beside it instead of `%APPDATA%`; it registers no `mumble://` handler and no start at login unless turned on, and About, the diagnostic file and the update hint say "Portable". The identity stays in Windows Credential Manager (per Windows user). A missing WebView2 Runtime now shows a message with the download link instead of failing silently. See `docs/windows-portable.md` (#181)
- Right-clicking (or pressing the menu key on, or using the small arrow beside) the microphone or headphones button in the voice bar opens a quick audio popover; left-click still mutes or deafens. The microphone popover lists the input devices (current one checked, system default marked) with an input volume slider (0–200 %), a live level meter, a noise suppression switch and a link to the voice settings; the headphones popover lists the output devices with an output volume slider (0–100 %), a Test sound button and the same link. Switching applies at once without reconnecting, plugged-in devices appear live, and when a chosen device is unplugged Mumcord falls back to the system default and says so briefly (#153)

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
