# Changelog

All notable changes to Mumcord are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.0] - 2026-10-25

### Added

- In-game overlay on Linux that shows who is talking inside your game (Vulkan layer, works with Steam and Proton). It is switched on per game; games with anti-cheat are off by default.
- Window overlay: a click-through window that shows who is talking in your own channel, with a helper for KDE.
- "Now gaming": share the game you are playing.
- Quick settings with a collapsible Beta group, settings search and accessibility switches.
- Person menu with switches, icons and "Join channel"; the full profile is redesigned and has a private note.
- Friends with online state, shown in the right column with channel info, and private chats that are kept.
- History sync between Mumcord users.
- Full-size images on demand (Beta), with switches to offer and load them.
- Drop image files into the chat.
- Positional audio with Mumble Link and the official game plugins (Beta).
- All 24 official EU languages, including the tray menu. The translations have not been reviewed by native speakers yet; corrections are welcome via the "Report a bug" issue template.
- Privacy step in the setup wizard.
- Changelog and support links in About.
- Collapsible member sections that show people in other channels and remember their state; a server label above the channel tree.
- Windows builds are signed with Azure Artifact Signing and macOS builds are signed and notarized when the signing setup is active.

### Changed

- The overlay shows only talkers in your own channel.
- Settings have a sticky sidebar.

### Fixed

- Failed joins from the person menu are reported.
- Overlay state is shown correctly for configured games, and the Vulkan manifest is refreshed on every start.
- A deadlock in the engine and a wizard scroll problem.
- The user panel name is clickable again.

### Removed

- The commercial license offer. Mumcord stays licensed under AGPL-3.0 only.

### Not in this release

- The in-game overlay on Windows (until then Windows uses the window overlay). There is no overlay on macOS.

## [0.2.0] - 2026-10-25

### Added

- Chat features: image viewer with zoom, pan, navigation and save.
- Inline external images with dismissable previews and automatic clean-up.
- Spotify link cards with a click-to-load player.
- GIF favorites, Giphy and Tenor links with a source credit, and image-only messages without the URL.
- Now playing: share the track you are listening to.
- Issue templates for bug reports and feature requests.

### Changed

- **License:** Mumcord is licensed under the GNU AGPL-3.0 only from this version. Versions up to and including 0.1.1 were released under the MIT License and stay MIT.
- The settings button uses a gear icon.

### Fixed

- The settings gear icon is centered.
- The own track is shown everywhere.

## [0.1.1] - 2026-10-25

### Added

- Music quality switch in the audio settings, with a higher bitrate cap.

### Fixed

- The release build no longer fails on a stale smoke test lockfile.

## [0.1.0] - 2026-10-25

### Added

- First release: the foundation of the Mumble-compatible voice chat client for Windows, macOS and Linux.

[0.3.0]: https://github.com/ToBee94/mumcord/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/ToBee94/mumcord/compare/v0.1.1...v0.2.0
[0.1.1]: https://github.com/ToBee94/mumcord/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/ToBee94/mumcord/releases/tag/v0.1.0
