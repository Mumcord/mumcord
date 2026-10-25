# Changelog

## [0.3.1] - 2026-10-25

### Fixed

- Keep voice packets within Mumble's size limit with Music quality on, so others keep hearing you on servers with a high bandwidth limit ([`740f784`](https://github.com/ToBee94/mumcord/commit/740f7843773e4d91e42dfc1301ad5dc88836ce16))

## [0.3.0] - 2026-10-25

_Not in this release: the in-game overlay on Windows (which uses the window overlay until then) and any overlay on macOS._

### Changed

- Show only talkers in your own channel in the overlay ([`8c6ddd8`](https://github.com/ToBee94/mumcord/commit/8c6ddd83d7e5c2af6f61d73c53728e94bf50cb50))
- Make the settings sidebar sticky ([`2a57f30`](https://github.com/ToBee94/mumcord/commit/2a57f3058d2499df0cfdfba14e815060cd29e663))

### Added

- Add in-game overlay on Linux that shows who is talking inside your game (Vulkan layer, works with Steam and Proton), switched on per game and off by default for games with anti-cheat ([`dfd2e1f`](https://github.com/ToBee94/mumcord/commit/dfd2e1f7bf07f2300041f3c60d0b76f1d4f1db5b))
- Add window overlay, a click-through window that shows who is talking in your own channel, with a helper for KDE ([`cca1731`](https://github.com/ToBee94/mumcord/commit/cca1731866a76cad5e4729d4dc40214fef5f1f70)) ([`f7d9dfa`](https://github.com/ToBee94/mumcord/commit/f7d9dfafe173e4d8037efb8de8cc83f2caae1c8a))
- Add "Now gaming" to share the game you are playing ([`b31e246`](https://github.com/ToBee94/mumcord/commit/b31e24663e4c6e8c151db0b9bba938027a41499e))
- Add quick settings with a collapsible Beta group, settings search and accessibility switches ([`62a93cc`](https://github.com/ToBee94/mumcord/commit/62a93ccd3295cca5d23326edf640f03cbba97d31)) ([`2a57f30`](https://github.com/ToBee94/mumcord/commit/2a57f3058d2499df0cfdfba14e815060cd29e663))
- Add person menu with switches, icons and "Join channel", and redesign the full profile with a private note ([`bb832c1`](https://github.com/ToBee94/mumcord/commit/bb832c1cc98b727b7895ddab8a55f510a6834d9a)) ([`a011704`](https://github.com/ToBee94/mumcord/commit/a0117046985453878400995e4dd9d35a77a24383))
- Add friends with online state, shown in the right column with channel info, and keep private chats ([`2885748`](https://github.com/ToBee94/mumcord/commit/2885748183d5f29c12c9733d3a4575c1ca38d3e9)) ([`93c71e6`](https://github.com/ToBee94/mumcord/commit/93c71e6c8ffb3dfbc1d211ed4935b2ed7d7cc910))
- Add history sync between Mumcord users ([`c2b9e6d`](https://github.com/ToBee94/mumcord/commit/c2b9e6d0e4eaf6857a8758a576a3eec35693f3bf))
- Add full-size images on demand (Beta), with switches to offer and load them ([`2a60276`](https://github.com/ToBee94/mumcord/commit/2a602761c32917e2e902abb61c5a7f5523e12546)) ([`02dcac8`](https://github.com/ToBee94/mumcord/commit/02dcac88e8db2b767e74d47b2d31d889862031a0))
- Add dropping image files into the chat ([`34d4448`](https://github.com/ToBee94/mumcord/commit/34d44482938612bc486f24b07c8262bab9a7337a))
- Add positional audio with Mumble Link and the official game plugins (Beta) ([`af3a7d1`](https://github.com/ToBee94/mumcord/commit/af3a7d177dbeb097ca6c7322070f02eb1cb1dfb6)) ([`c952a47`](https://github.com/ToBee94/mumcord/commit/c952a47467c5eb90d5f518ed57c5219b67a45571))
- Add all 24 official EU languages, including the tray menu, not yet reviewed by native speakers (send corrections via the "Report a bug" issue template) ([`76f9e48`](https://github.com/ToBee94/mumcord/commit/76f9e487f62e19ed398192eb8fd9f5d3eb3f6fab)) ([`e5c1391`](https://github.com/ToBee94/mumcord/commit/e5c13919b58ee9dc498e082765902597b41787b1))
- Add privacy step to the setup wizard ([`0ec61bd`](https://github.com/ToBee94/mumcord/commit/0ec61bd3cc2dab8300cc0b81b53ce1fffc445b36))
- Add changelog and support links to About ([`eb5ab93`](https://github.com/ToBee94/mumcord/commit/eb5ab933d96b85442d9fc48957cc94c004438667))
- Add collapsible member sections that show people in other channels and remember their state, and a server label above the channel tree ([`e97ef66`](https://github.com/ToBee94/mumcord/commit/e97ef6610d60e1d7d8c049f92f731f0721c8a393)) ([`169361f`](https://github.com/ToBee94/mumcord/commit/169361fbd2db873829392ea9b1abe37777896941))
- Add Windows signing with Azure Artifact Signing and macOS signing and notarization when the signing setup is active ([`8d6110c`](https://github.com/ToBee94/mumcord/commit/8d6110c75f261d42c4f381f459970f7895667e46)) ([`1631237`](https://github.com/ToBee94/mumcord/commit/1631237a1dc76dabbdb4d888ee48690646725dac))

### Removed

- Remove the commercial license offer, Mumcord stays licensed under AGPL-3.0 only ([`5f2c74a`](https://github.com/ToBee94/mumcord/commit/5f2c74ab27e3eddafab721769ac10a674a2edb9d))

### Fixed

- Report failed joins from the person menu ([`9368e18`](https://github.com/ToBee94/mumcord/commit/9368e18ce384a97ac31444d0c12f8b4f2af67848))
- Show the correct overlay state for configured games and refresh the Vulkan manifest on every start ([`d481dda`](https://github.com/ToBee94/mumcord/commit/d481ddab454ecad719b09767d0f5b58789f9ae1a)) ([`fa55bcb`](https://github.com/ToBee94/mumcord/commit/fa55bcba5410baa47c70fb55a1e1a14e29d9f4e9))
- Fix a deadlock in the engine and a scroll problem in the wizard ([`835dfff`](https://github.com/ToBee94/mumcord/commit/835dfffac6cc186ace699ff912ad99fd714332f7))
- Make the user panel name clickable again ([`4943baa`](https://github.com/ToBee94/mumcord/commit/4943baaa752347d90c9bde4078d8835134891834))

## [0.2.0] - 2026-10-25

_**License change:** Mumcord is licensed under the GNU AGPL-3.0 only from this version, while versions up to and including 0.1.1 were released under the MIT License and stay MIT._

### Changed

- Use a gear icon for the settings button ([`4134e84`](https://github.com/ToBee94/mumcord/commit/4134e847a5bcbca763d830bef068e21f18e6086b))

### Added

- Add chat image viewer with zoom, pan, navigation and save ([`94962d9`](https://github.com/ToBee94/mumcord/commit/94962d9efb0dce908d1b7fe90444f111a2f9b93e))
- Add inline external images with dismissable previews and automatic clean-up ([`9f1575d`](https://github.com/ToBee94/mumcord/commit/9f1575d41186e96ff697b15df32851972d8cd77f))
- Add Spotify link cards with a click-to-load player ([`292f1a3`](https://github.com/ToBee94/mumcord/commit/292f1a3823a19264b51a8f7b7178139e69441990))
- Add GIF favorites, Giphy and Tenor links with a source credit, and image-only messages without the URL ([`8a154ec`](https://github.com/ToBee94/mumcord/commit/8a154ec8d9c5baa279b916eb8006884a14a452b8)) ([`80df935`](https://github.com/ToBee94/mumcord/commit/80df9350bbe603d5975534a3503990097d466b46))
- Add "Now playing" to share the track you are listening to ([`7d9f0c4`](https://github.com/ToBee94/mumcord/commit/7d9f0c48aff81a86424e32febd85168452b66e5e))
- Add issue templates for bug reports and feature requests ([`911a0da`](https://github.com/ToBee94/mumcord/commit/911a0daaf972caf4f8bbbc9baba146dfbb8953e0))

### Fixed

- Center the settings gear icon ([`4b34dc8`](https://github.com/ToBee94/mumcord/commit/4b34dc84304a5aae62b0528fb998029338ed65db))
- Show the own track everywhere ([`246057e`](https://github.com/ToBee94/mumcord/commit/246057eb3ee44bf45a8b9a43fc8929c5a60a26b2))

## [0.1.1] - 2026-10-25

### Added

- Add music quality switch to the audio settings, with a higher bitrate cap ([`e4d870f`](https://github.com/ToBee94/mumcord/commit/e4d870f283fa35a8c04652b9333bca38141d0b7a)) ([`1311379`](https://github.com/ToBee94/mumcord/commit/1311379c5d7d612f8cc0ffc0389eb8ca2b3d35b2))

### Fixed

- Fix release build failing on a stale smoke test lockfile ([`e5972ee`](https://github.com/ToBee94/mumcord/commit/e5972eecf291b2234514d87cc505ce29bb7bf3f7))

## [0.1.0] - 2026-10-25

_First release._

[0.3.1]: https://github.com/ToBee94/mumcord/releases/tag/v0.3.1
[0.3.0]: https://github.com/ToBee94/mumcord/releases/tag/v0.3.0
[0.2.0]: https://github.com/ToBee94/mumcord/releases/tag/v0.2.0
[0.1.1]: https://github.com/ToBee94/mumcord/releases/tag/v0.1.1
[0.1.0]: https://github.com/ToBee94/mumcord/releases/tag/v0.1.0
