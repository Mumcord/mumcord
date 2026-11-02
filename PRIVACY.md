# Privacy policy

Mumcord is a desktop voice chat client for Mumble servers. This policy describes every network connection Mumcord makes, what it stores on your computer, and how to switch optional features off. It applies to the Mumcord desktop app and its installers. The website [mumcord.app](https://mumcord.app) is not covered here.

## In short

- Mumcord sends **no telemetry, no analytics and no crash reports**. There is no Mumcord account and no Mumcord server.
- Mumcord connects only to the Mumble servers you add, to web addresses that appear in chat (for previews and images, which you can switch off), and to other Mumcord users through the Mumble server you are both on – or, only if you allow it, directly or through a relay that the server's community runs.
- Everything Mumcord stores stays on your computer.

This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it, except for the connections listed below.

## Network connections

### Mumble servers you add

Mumcord connects to a Mumble server only when you connect to it: from your server list, from a `mumble://` link you open, from the connect dialog, or at start for the servers you marked "Always connect at start" or were connected to when you quit. Several servers can be connected at once; each one only learns about its own connection. The connection is encrypted (TLS) for control data and chat, and voice uses the encrypted Mumble voice channel. Mumcord sends what the Mumble protocol needs: your user name, your client certificate (your identity), the server password if you set one, the passwords of protected channels you entered (as Mumble access tokens), the client name and version ("Mumcord <version>") and the type of operating system (for example "linux", which the server shows to its administrators, as with official Mumble clients), your voice while you talk, chat messages you send, and your avatar and comment.

- **Default avatar:** when you connect to a server for the first time, Mumcord sends the default avatar to it. Switch this off in _Settings_ ("Send to new servers automatically" under the default avatar).
- **Connection info:** to show ping, packet loss and bandwidth, Mumcord asks the server for the connection statistics of the people you look at; the server decides which fields it returns.
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

For YouTube, Twitch and Spotify links Mumcord shows a player card. The player itself is loaded **only after you click play**, and in a watch party only when you start or join it. It is then loaded from the provider's embed address (`www.youtube-nocookie.com`, `player.twitch.tv`, `clips.twitch.tv`, `open.spotify.com/embed`), and from that moment the provider's own privacy policy applies, including any cookies or tracking it uses. Each person in a watch party loads the video from YouTube themselves; Mumcord never streams the video to others:

- YouTube / Google: <https://policies.google.com/privacy>
- Twitch: <https://www.twitch.tv/p/legal/privacy-notice/>
- Spotify: <https://www.spotify.com/legal/privacy-policy/>

**Local player page:** YouTube and Twitch refuse to play inside the app's own internal address, so while a YouTube or Twitch player or a watch party is open, Mumcord runs a tiny web server on your own computer (`localhost`, a random port) that serves one small page holding the provider's player. It answers only on the loopback address, so other computers cannot reach it, only under a random secret path, and it stops when the player closes. It logs nothing and sends nothing anywhere itself; the player inside it talks to YouTube or Twitch as described above.

Images from Giphy and Tenor are fetched as described above (only with link previews on); their providers' policies are <https://giphy.com/privacy> (Giphy) and <https://policies.google.com/privacy> (Tenor, a Google service).

### Features shared with other Mumcord users on the same server

Some features work only between Mumcord users. They exchange small messages as Mumble plugin data **through the Mumble server you are connected to**, never through any other system, and only with people on that same server. Official Mumble clients ignore these messages. Every message type, with its exact contents, is documented on the wiki page [Plugin data protocol](https://github.com/Mumcord/mumcord/wiki/Plugin-Data-Protocol). Like everything you send to a server, these messages pass through the server, so its operator could read them.

- **Mumcord recognition:** on connect, Mumcord sends a short "hello" (which Mumcord features your client supports) to the people on the server.
- **Chat history sync:** Mumcord asks other Mumcord users for channel messages you missed, and answers such requests with the channel messages it has stored. Private messages are never shared. Switch answering off in _Settings_ ("Share my channel history", on by default).
- **Full-size images:** pictures you send in chat go to the server as a thumbnail. Other Mumcord users who open the picture fetch the full-size version from your client, over a direct connection when you both allow it (see below), otherwise through the server. Switch off offering your originals ("Offer my full-size images") or loading others' originals ("Load full-size images from others") in _Settings_ (both on by default).
- **Watch party (Beta):** when you start or join a watch party, the YouTube video IDs of the playlist, who added them, who is in the party, and play, pause, seek and skip commands go to the Mumcord users in your voice channel. Titles and thumbnails come from the link preview described above.
- **Screen sharing (Beta):** that you are sharing, requests to watch and their answers are exchanged with the Mumcord users in your voice channel. The picture and sound themselves never go through the server: see below.
- **Now playing** and **Now gaming:** share the track playing in your media player or the game you are playing. Both are **off by default**; while off, Mumcord reads nothing from your system. When on, the track or game is also added to your Mumble comment, which every user on the server can see.

### Plugins

Plugins are small add-ons that you install yourself; none is installed by default. They run inside a sandbox on your computer. They have no access to files, the network or other programs, only to what Mumcord lists in the permission prompt you accept at install time. You can revoke optional permissions or turn off all plugins at any time in _Settings → Plugins_.

**What a plugin can see.** Depending on the permissions you grant: who is in your channel and who is talking, your own user name, and the messages other users' copies of the same plugin send it. A plugin never sees the text of your chat messages, with two narrow exceptions: a command you type for it, such as `/roll`, which is not posted to the chat; and, only if you grant it "Read chat messages that start with its trigger words", messages in your current channel (yours and other people's) whose first line starts with one of the words it declares, such as `!poll 2` – the plugin then gets the sender, the word and the rest of that line, at most 200 characters, and nothing else of the chat; and, only if you grant it "Recognise certain chat messages", messages in your current channel (yours and other people's) that start the way the plugin declares, for example a pasted game fit – the install prompt shows what each of its patterns recognises. The plugin then gets the sender and the matching message as plain text, at most 4 KB, and nothing else of the chat. Messages that match no pattern are checked inside Mumcord only; they are not passed on, stored, logged or counted. A plugin also never sees your certificate hashes, your IP address or your passwords. Users you blocked never reach a plugin. Plugin messages from you are sent as Mumble plugin data **through the Mumble server**, only to Mumcord users on that server who have the same plugin, so the server operator can read them, like everything else you send to a server. If you set a plugin's visibility to friends only or nobody, its messages are limited accordingly. Each plugin identifies you to other users with its own key (`peer.key`); it differs per plugin, so two plugins cannot be linked to one person through it.

**Plugins that talk to the internet.** Only plugins signed by the official repository can ask for the "Talk to a server you choose" permission (shown in red). Such a plugin contacts only the host you typed into one of its forms, and the host sees your IP address and whatever the plugin sends. Passwords or keys you enter for it are kept in the operating system's secure storage; the plugin can use them in requests but can never read them back. Plugin screens that use a web view are sandboxed and make no network connections of their own.

**What repositories can see.** Mumcord downloads plugin lists and plugin files from the official repository (`Mumcord/plugins`, served by GitHub Pages) and from third-party repositories you add yourself. Requests carry no user id, no list of installed plugins and no server names, only a standard `User-Agent` (`Mumcord`) and the usual conditional-request header to save bandwidth. Like any web request they reveal your IP address to the host. For GitHub, its privacy statement applies: <https://docs.github.com/site-policy/privacy-policies/github-general-privacy-statement>. You can set a mirror under _Settings → Plugins → Repositories → Advanced_. A list of withdrawn plugins is fetched once a day, even when plugin updates are off, so Mumcord can switch off a dangerous plugin. Mumcord works offline with the last downloaded list. Third-party repositories are not operated by Mumcord; their own privacy policies apply, and you can see which repository a plugin came from on its card.

**Server suggestions.** A server's registered bot can suggest plugins to install. These suggestions appear in the notification centre and install nothing by themselves. Mumcord sends nothing back to the bot, not even whether you installed or dismissed a suggestion.

**Logs.** Mumcord logs plugin names, versions, start and stop events and errors only. Plugin data, stored values, secrets, contents of internet requests and chat text are never logged. Messages a plugin writes to its diagnostics view stay in memory and are gone when you quit.

**Your plugin data.** Each plugin's saved data stays on your computer. _Export data_ saves it to a file you choose; _Reset data_ deletes it, together with the plugin's secrets. When you remove a plugin you can delete its data at the same time; if you keep it, Mumcord deletes it after 30 days.

### Direct connections to other Mumcord users

Full-size images and screen sharing can go directly between two Mumcord clients instead of through the Mumble server. A direct connection **reveals your IP address to the other person**, and Mumcord may ask your home router to open a port for it (UPnP, NAT-PMP or PCP; the port is closed again when the connection ends). Mumcord makes no direct connection until you agree: the first time one would be used, it asks you once. _Settings → Privacy → Peer-to-peer_ chooses how these connections are made: direct first with the relay as fallback (default), always through the relay (hides your IP address), direct only, relay only, or off. A server can override this in its _Behaviour_ tab.

To set up a connection, the two clients exchange the addresses where they can be reached: the addresses of the computer's network interfaces (public and private IPv6 addresses, the local network IPv4 address and a public IPv4 address if the computer has one; virtual interfaces of Docker, virtual machines and similar are left out, VPN interfaces are kept) and, if the router allowed a port mapping, the router's public address and port. These addresses are sent as plugin data through the Mumble server **only to that one person**, never to everyone on the server. No other server is asked to find your public address: no STUN or TURN is involved. The connection is encrypted end to end and only accepted from the person whose Mumble certificate the server reports.

**Screen sharing** uses only these peer-to-peer connections, direct or through the community relay described below. The picture and the sound of what you share go only to the people who chose to watch your share and whom Mumcord accepted (at most 8, all in your voice channel); nothing of it passes through the Mumble server, Mumcord records or stores none of it, and a relay only forwards it encrypted. When neither a direct connection nor the relay can be used (for example when both networks have no IPv6 and no port mapping and the server has no relay), that person cannot watch. Change your answer in _Settings → Privacy_ ("Allow direct connections to other Mumcord users"). With direct connections off, Mumcord still uses the server's relay if it has one; without a relay, images arrive through the Mumble server and screen sharing is unavailable. With peer-to-peer set to "Off", screen sharing is unavailable and images always come through the Mumble server.

### Community relay (optional)

Some server communities run a relay (_mumpeer_) next to their Mumble server, together with a relay bot that is a registered user on that server. Mumcord itself runs no relay, and everything works without one. When the bot announces a relay, Mumcord can carry full-size images and screen sharing through it instead of directly, for example when no direct path works or when you chose "always relay" to hide your IP address. Voice never uses the relay.

- **Announcement and token:** the relay bot sends Mumcord the relay's address, ports and certificate fingerprint as plugin data. Mumcord remembers the first relay it sees on a server (trust on first use) and does not use a changed relay until you trust it in the server info panel. For each connection, Mumcord asks the bot for a one-time pair token; the bot tells the relay the Mumble certificate hashes of you and the other person, and sends the token to both of you.
- **What the relay operator sees:** your IP address and the other person's, the certificate hashes of each pair, and how much data flows and when. The relay **cannot read or change the content**: images and screen pictures stay end-to-end encrypted between the two Mumcord clients, and the relay only forwards those encrypted packets.
- **Nothing stored on disk:** the relay keeps pairs, sessions and counters only in memory. Its logs leave out full IP addresses, certificate hashes and tokens.
- **IP address hidden from the other person:** in the modes that hide your IP ("always relay" and "relay only", or with direct connections turned off), Mumcord sends the other person no addresses at all. They only see the relay's address, not yours; the connection info shows "IP hidden". The relay's operator still sees your IP address.

The relay operator is the server's community, not Mumcord. If you do not want a relay to see your IP address, choose "Direct only" or "Off".

### Links you open

Links in chat, the website, changelog and issue links in _Settings → About_, and "Join game" (a `steam://` link) are handed to your browser or the operating system only when you click them.

### Installation on Windows

Mumcord needs Microsoft Edge WebView2, which is part of current Windows versions. If it is missing, the Windows installer downloads the WebView2 installer from Microsoft. Microsoft's privacy statement applies: <https://privacy.microsoft.com/privacystatement>.

Mumcord's in-app updater only contacts GitHub while you allow it. With "When an update is found" set to **Off** (_Settings → Updates_; also asked in the first-start wizard and once for existing users) nothing is sent. With **Notify only** or **Download automatically**, Mumcord asks GitHub (`api.github.com`) for the list of Mumcord releases on start and then every few hours (12 by default), and downloads an update you accepted (or, with "Download automatically", the newest version) from `github.com/Mumcord/mumcord/releases` (GitHub's download servers). The requests carry no identifier besides a standard `User-Agent` (`Mumcord/<version>`): no user id, no server names. Like any web request they reveal your IP address to GitHub, whose privacy statement applies: <https://docs.github.com/site-policy/privacy-policies/github-general-privacy-statement>. Development builds never check.

## Data stored on your computer

Mumcord stores the following locally, in your user profile's application data folder (and, for secrets, in the operating system's secure storage). Nothing of it is uploaded anywhere.

- **Identity and passwords:** your client certificate, saved server passwords and the channel passwords you entered (per server), in the operating system's secure storage (Windows Credential Manager, macOS Keychain, Secret Service or KWallet on Linux).
- **Settings, server list, favorites, friends and themes**, and which welcome messages and channel descriptions you hid.
- **Relay pins:** for each server and identity where a relay was announced, the relay bot's certificate hash, the relay's certificate fingerprint and when it was first seen, so a changed relay is noticed.
- **Chat history** of channels and private chats, so you can read it after a restart. You can set a maximum age in _Settings_; older messages are deleted automatically.
- **Preview and image cache:** link previews, inline images and full-size chat images, deleted after the maximum age set in _Settings_ (7 days by default). You can clear it at any time ("Clear now").
- **Plugins:** the plugins you installed, their permissions and settings, their saved data and the repositories you added, with the key of each pinned on first use. Plugin secrets are in the operating system's secure storage.
- **Log files:** technical logs for troubleshooting, kept for at most 7 days. By design they contain no chat text, user or channel names, server addresses, passwords, certificates or audio. A diagnostic file is created only when you export it yourself, and you decide whether to share it.

To remove everything, uninstall Mumcord and delete its application data folder; remove the "Mumcord" entries from the operating system's secure storage if you want to delete your identity as well (export it first if you want to keep it).

Notifications, activity sounds (for watch parties and screen shares) and the activity banners in the chat are created on your computer from what the server and other Mumcord users already sent; showing or playing them sends nothing.

## Changes and contact

Changes to this policy are made in this file in the [Mumcord repository](https://github.com/Mumcord/mumcord) and are visible in its history. Questions: open an issue at <https://github.com/Mumcord/mumcord/issues>.
