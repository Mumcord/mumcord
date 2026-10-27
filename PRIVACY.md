# Privacy policy

Mumcord is a desktop voice chat client for Mumble servers. This policy describes every network connection Mumcord makes, what it stores on your computer, and how to switch optional features off. It applies to the Mumcord desktop app and its installers. The website [mumcord.app](https://mumcord.app) is not covered here.

## In short

- Mumcord sends **no telemetry, no analytics and no crash reports**. There is no Mumcord account and no Mumcord server.
- Mumcord connects only to the Mumble servers you add, to web addresses that appear in chat (for previews and images, which you can switch off), and to other Mumcord users through the Mumble server you are both on – or, only if you allow it, directly.
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

### Direct connections to other Mumcord users

Full-size images and screen sharing can go directly between two Mumcord clients instead of through the Mumble server. A direct connection **reveals your IP address to the other person**, and Mumcord may ask your home router to open a port for it (UPnP, NAT-PMP or PCP; the port is closed again when the connection ends). Mumcord makes no direct connection until you agree: the first time one would be used, it asks you once.

To set up a connection, the two clients exchange the addresses where they can be reached: the addresses of the computer's network interfaces (public and private IPv6 addresses, the local network IPv4 address and a public IPv4 address if the computer has one; virtual interfaces of Docker, virtual machines and similar are left out, VPN interfaces are kept) and, if the router allowed a port mapping, the router's public address and port. These addresses are sent as plugin data through the Mumble server **only to that one person**, never to everyone on the server. No other server is asked to find your public address: no STUN, TURN or relay is involved, by design. The connection is encrypted and only accepted from the person whose Mumble certificate the server reports.

**Screen sharing** uses only these direct connections. The picture and the sound of what you share go only to the people who chose to watch your share and whom Mumcord accepted (at most 8, all in your voice channel); nothing of it passes through the Mumble server or any other system, and Mumcord records or stores none of it. When a direct connection cannot be made (for example when both networks have no IPv6 and no port mapping), that person cannot watch. Change your answer in _Settings → Privacy_ ("Allow direct connections to other Mumcord users"). With direct connections off, images still arrive through the Mumble server and screen sharing is unavailable.

### Links you open

Links in chat, the website, changelog and issue links in _Settings → About_, and "Join game" (a `steam://` link) are handed to your browser or the operating system only when you click them.

### Installation on Windows

Mumcord needs Microsoft Edge WebView2, which is part of current Windows versions. If it is missing, the Windows installer downloads the WebView2 installer from Microsoft. Microsoft's privacy statement applies: <https://privacy.microsoft.com/privacystatement>.

Mumcord has no automatic updater. If you switch on "Check for new versions" (asked in the first-start wizard, once for existing users, and always available in _Settings → About_), Mumcord asks GitHub (`api.github.com`) at most once a day for the latest release of Mumcord and shows a hint with a link to it. Nothing is downloaded or installed. The request carries no identifier besides a standard `User-Agent` (`Mumcord/<version>`); like any web request it reveals your IP address to GitHub, whose privacy statement applies: <https://docs.github.com/site-policy/privacy-policies/github-general-privacy-statement>. Nothing is sent while the option is off.

## Data stored on your computer

Mumcord stores the following locally, in your user profile's application data folder (and, for secrets, in the operating system's secure storage). Nothing of it is uploaded anywhere.

- **Identity and passwords:** your client certificate, saved server passwords and the channel passwords you entered (per server), in the operating system's secure storage (Windows Credential Manager, macOS Keychain, Secret Service or KWallet on Linux).
- **Settings, server list, favorites, friends and themes**, and which welcome messages and channel descriptions you hid.
- **Chat history** of channels and private chats, so you can read it after a restart. You can set a maximum age in _Settings_; older messages are deleted automatically.
- **Preview and image cache:** link previews, inline images and full-size chat images, deleted after the maximum age set in _Settings_ (7 days by default). You can clear it at any time ("Clear now").
- **Log files:** technical logs for troubleshooting, kept for at most 7 days. By design they contain no chat text, user or channel names, server addresses, passwords, certificates or audio. A diagnostic file is created only when you export it yourself, and you decide whether to share it.

To remove everything, uninstall Mumcord and delete its application data folder; remove the "Mumcord" entries from the operating system's secure storage if you want to delete your identity as well (export it first if you want to keep it).

Notifications, activity sounds (for watch parties and screen shares) and the activity banners in the chat are created on your computer from what the server and other Mumcord users already sent; showing or playing them sends nothing.

## Changes and contact

Changes to this policy are made in this file in the [Mumcord repository](https://github.com/Mumcord/mumcord) and are visible in its history. Questions: open an issue at <https://github.com/Mumcord/mumcord/issues>.
