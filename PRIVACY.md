# Privacy policy

Mumcord is a desktop voice chat client for Mumble servers. This policy describes every network connection Mumcord makes, what it stores on your computer, and how to switch optional features off. It applies to the Mumcord desktop app and its installers. The website [mumcord.app](https://mumcord.app) is not covered here.

## In short

- Mumcord sends **no telemetry, no analytics and no crash reports**. There is no Mumcord account and no Mumcord server.
- Mumcord connects only to the Mumble servers you add, to web addresses that appear in chat (for previews and images, which you can switch off), and to other Mumcord users through the Mumble server you are both on.
- Everything Mumcord stores stays on your computer.

This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it, except for the connections listed below.

## Network connections

### Mumble servers you add

Mumcord connects to a Mumble server only when you connect to it: from your server list, from a `mumble://` link you open, or from the connect dialog. The connection is encrypted (TLS) for control data and chat, and voice uses the encrypted Mumble voice channel. Mumcord sends what the Mumble protocol needs: your user name, your client certificate (your identity), the server password if you set one, your voice while you talk, chat messages you send, and your avatar and comment.

- **Default avatar:** when you connect to a server for the first time, Mumcord sends the default avatar to it. Switch this off in _Settings_ ("Send to new servers automatically" under the default avatar).
- The server operator decides what the server stores and logs. Their privacy rules apply to everything you send there.

Mumcord does not query public server lists and does not ping servers in the background.

### Link previews, video cards and images from chat

When **link previews** are on (_Settings_, "Show link previews and embeds", on by default), Mumcord fetches web addresses that appear in chat messages so it can show a preview card, an inline image or a GIF:

- the linked page (to read its title, description and preview image),
- images and GIFs linked or embedded in messages (for example from Giphy or Tenor),
- for YouTube and Spotify links, the provider's public oEmbed endpoint (`www.youtube.com/oembed`, `open.spotify.com/oembed`),
- the cover image of a track someone shares with "Now playing" and the Steam store picture of a game someone shares with "Now gaming".

These requests are made by Mumcord itself, not by the built-in web view. They go to the website named in the link, so that website sees your IP address, the time and a generic user agent (`Mozilla/5.0 (compatible; Mumcord link preview)`). Mumcord sends no cookies and no information about you, the chat or the server. Requests to private or local network addresses are refused.

When link previews are **off**, Mumcord shows plain links and makes **none** of these requests.

### Video and music players (YouTube, Twitch, Spotify)

For YouTube, Twitch and Spotify links Mumcord shows a player card. The player itself is loaded **only after you click play**. It is then loaded from the provider's embed address (`www.youtube-nocookie.com`, `player.twitch.tv`, `clips.twitch.tv`, `open.spotify.com/embed`), and from that moment the provider's own privacy policy applies, including any cookies or tracking it uses:

- YouTube / Google: <https://policies.google.com/privacy>
- Twitch: <https://www.twitch.tv/p/legal/privacy-notice/>
- Spotify: <https://www.spotify.com/legal/privacy-policy/>

Images from Giphy and Tenor are fetched as described above (only with link previews on); their providers' policies are <https://giphy.com/privacy> (Giphy) and <https://policies.google.com/privacy> (Tenor, a Google service).

### Features shared with other Mumcord users on the same server

Some features work only between Mumcord users. They exchange small messages as Mumble plugin data **through the Mumble server you are connected to**, never through any other system, and only with people on that same server. Official Mumble clients ignore these messages.

- **Mumcord recognition:** on connect, Mumcord sends a short "hello" (which Mumcord features your client supports) to the people on the server.
- **Chat history sync:** Mumcord asks other Mumcord users for channel messages you missed, and answers such requests with the channel messages it has stored. Private messages are never shared. Switch answering off in _Settings_ ("Share my channel history", on by default).
- **Full-size images:** pictures you send in chat go to the server as a thumbnail. Other Mumcord users who open the picture fetch the full-size version from your client. Switch off offering your originals ("Offer my full-size images") or loading others' originals ("Load full-size images from others") in _Settings_ (both on by default).
- **Now playing** and **Now gaming:** share the track playing in your media player or the game you are playing. Both are **off by default**; while off, Mumcord reads nothing from your system. When on, the track or game is also added to your Mumble comment, which every user on the server can see.

### Links you open

Links in chat, the website, changelog and issue links in _Settings → About_, and "Join game" (a `steam://` link) are handed to your browser or the operating system only when you click them.

### Installation on Windows

Mumcord needs Microsoft Edge WebView2, which is part of current Windows versions. If it is missing, the Windows installer downloads the WebView2 installer from Microsoft. Microsoft's privacy statement applies: <https://privacy.microsoft.com/privacystatement>.

Mumcord has no automatic updater and does not check for updates.

## Data stored on your computer

Mumcord stores the following locally, in your user profile's application data folder (and, for secrets, in the operating system's secure storage). Nothing of it is uploaded anywhere.

- **Identity and passwords:** your client certificate and saved server passwords, in the operating system's secure storage (Windows Credential Manager, macOS Keychain, Secret Service or KWallet on Linux).
- **Settings, server list, favorites and friends.**
- **Chat history** of channels and private chats, so you can read it after a restart. You can set a maximum age in _Settings_; older messages are deleted automatically.
- **Preview and image cache:** link previews, inline images and full-size chat images, deleted after the maximum age set in _Settings_ (7 days by default). You can clear it at any time ("Clear now").
- **Log files:** technical logs for troubleshooting, kept for at most 7 days. By design they contain no chat text, user or channel names, server addresses, passwords, certificates or audio. A diagnostic file is created only when you export it yourself, and you decide whether to share it.

To remove everything, uninstall Mumcord and delete its application data folder; remove the "Mumcord" entries from the operating system's secure storage if you want to delete your identity as well (export it first if you want to keep it).

## Changes and contact

Changes to this policy are made in this file in the [Mumcord repository](https://github.com/ToBee94/mumcord) and are visible in its history. Questions: open an issue at <https://github.com/ToBee94/mumcord/issues>.
