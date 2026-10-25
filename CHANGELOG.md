# Changelog

## [0.3.0] - 2026-10-25

_Not in this release: the in-game overlay on Windows (which uses the window overlay until then) and any overlay on macOS._

### Changed

- Show only talkers in your own channel in the overlay
- Make the settings sidebar sticky

### Added

- Add in-game overlay on Linux that shows who is talking inside your game (Vulkan layer, works with Steam and Proton), switched on per game and off by default for games with anti-cheat
- Add window overlay, a click-through window that shows who is talking in your own channel, with a helper for KDE
- Add "Now gaming" to share the game you are playing
- Add quick settings with a collapsible Beta group, settings search and accessibility switches
- Add person menu with switches, icons and "Join channel", and redesign the full profile with a private note
- Add friends with online state, shown in the right column with channel info, and keep private chats
- Add history sync between Mumcord users
- Add full-size images on demand (Beta), with switches to offer and load them
- Add dropping image files into the chat
- Add positional audio with Mumble Link and the official game plugins (Beta)
- Add all 24 official EU languages, including the tray menu, not yet reviewed by native speakers (send corrections via the "Report a bug" issue template)
- Add privacy step to the setup wizard
- Add changelog and support links to About
- Add collapsible member sections that show people in other channels and remember their state, and a server label above the channel tree
- Add Windows signing with Azure Artifact Signing and macOS signing and notarization when the signing setup is active

### Removed

- Remove the commercial license offer, Mumcord stays licensed under AGPL-3.0 only

### Fixed

- Report failed joins from the person menu
- Show the correct overlay state for configured games and refresh the Vulkan manifest on every start
- Fix a deadlock in the engine and a scroll problem in the wizard
- Make the user panel name clickable again

## [0.2.0] - 2026-10-25

_**License change:** Mumcord is licensed under the GNU AGPL-3.0 only from this version, while versions up to and including 0.1.1 were released under the MIT License and stay MIT._

### Changed

- Use a gear icon for the settings button

### Added

- Add chat image viewer with zoom, pan, navigation and save
- Add inline external images with dismissable previews and automatic clean-up
- Add Spotify link cards with a click-to-load player
- Add GIF favorites, Giphy and Tenor links with a source credit, and image-only messages without the URL
- Add "Now playing" to share the track you are listening to
- Add issue templates for bug reports and feature requests

### Fixed

- Center the settings gear icon
- Show the own track everywhere

## [0.1.1] - 2026-10-25

### Added

- Add music quality switch to the audio settings, with a higher bitrate cap

### Fixed

- Fix release build failing on a stale smoke test lockfile

## [0.1.0] - 2026-10-25

_First release._

[0.3.0]: https://github.com/ToBee94/mumcord/releases/tag/v0.3.0
[0.2.0]: https://github.com/ToBee94/mumcord/releases/tag/v0.2.0
[0.1.1]: https://github.com/ToBee94/mumcord/releases/tag/v0.1.1
[0.1.0]: https://github.com/ToBee94/mumcord/releases/tag/v0.1.0
