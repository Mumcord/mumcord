# Screenshots

Screenshots for the project website and the README. All of them show the UI running on the
built-in mock engine with fictional demo data (people, servers and messages are made up; addresses
use `example.org`). Size: 1440×900 at 2× scale (2880×1800 pixels).

| File | Caption |
| --- | --- |
| `main-dark.png` | Talk and chat in one window: servers on the left, channels with avatars (stacked when crowded), who is talking (green ring), chat with avatars, a highlighted message that mentions you, code with syntax highlighting, a link preview card, status icons (guest, registered, muted, listening, music), and your user panel with the track you are listening to. |
| `main-light.png` | The same main view in the light theme. |
| `chat-code-toolbar.png` | Code block with automatic syntax highlighting, and the formatting toolbar above the message box (bold, italic, strikethrough, code, code block, link). |
| `chat-link-preview.png` | Link preview card with title, description and picture for a web page. |
| `chat-youtube.png` | YouTube card: the video plays only after you click it. |
| `chat-spotify.png` | Spotify card for a shared track, album or playlist. |
| `chat-images.png` | External pictures and GIFs shown inline in the chat; GIFs from GIPHY carry a "via GIPHY" credit. |
| `image-viewer.png` | Image viewer, zoomed in, with zoom, save, favorite and open-in-browser buttons. |
| `gif-picker.png` | GIF favorites picker: pick a saved GIF to send it as a link. |
| `mention-autocomplete.png` | Type @ to get name suggestions; a message that mentions you is highlighted. |
| `chat-compact.png` | Compact chat layout: time, name and message on one line, for busy channels. |
| `channel-avatar-stack.png` | Crowded channels (more than five people) show a stack of avatars with a "Show all" button instead of a long list; the eye marks a followed channel. |
| `follow-unread-badge.png` | Follow a channel with its eye and see unread-message counts at a glance, plus a pop-up for new ones. |
| `notification-toast.png` | A small in-app pop-up announces a new message in another channel. |
| `person-menu.png` | Right-click a person: view profile, mute for me, volume and private message. |
| `profile-preview.png` | Hover a person to preview their profile, including what they are listening to right now. |
| `wizard-1-identity.png` | First start, step 1: create your identity with just a name, or bring the one you already use. |
| `wizard-2-microphone.png` | First start, step 2: pick microphone and speakers and test them with a live level meter. |
| `wizard-3-talk-mode.png` | First start, step 3: choose voice activation or push-to-talk with your own key, and optionally open mumble:// links, start Mumcord at login and switch link previews on. |
| `settings-01-identity-dark.png` | Settings -> Identity: back up your identity and set a default avatar. |
| `settings-01-identity-light.png` | Settings -> Identity in the light theme. |
| `settings-02-audio-dark.png` | Settings -> Audio: devices, music quality and a microphone test with noise suppression applied. |
| `settings-03-voice-dark.png` | Settings -> Voice: manual threshold with a live level meter, connect muted until you join a channel, and voice processing. |
| `settings-03-voice-light.png` | Settings -> Voice in the light theme. |
| `settings-04-chat-history-dark.png` | Settings -> Chat & history: link previews switch and how long cached previews are kept. |
| `settings-04b-chat-history-more-dark.png` | Settings -> Chat & history, further down: chat history is kept only on your computer, per server, and old messages can be deleted automatically. |
| `settings-04-chat-history-light.png` | Settings -> Chat & history in the light theme. |
| `settings-05-appearance-dark.png` | Settings -> Appearance: colour theme, chat layout (cozy or compact), window behaviour and start at login. |
| `settings-05-appearance-light.png` | Settings -> Appearance in the light theme. |
| `settings-06-language-dark.png` | Settings -> Language: English, Spanish and German. |
| `settings-07-sounds-notifications-dark.png` | Settings -> Sounds & notifications: choose which events play a short sound (including mentions, joining a channel and hanging up) and how new messages are announced. |
| `settings-07-sounds-notifications-light.png` | Settings -> Sounds & notifications in the light theme. |
| `settings-08-diagnostics-dark.png` | Settings -> Diagnostics: save a diagnostic file without chat messages or personal data. |
| `settings-09-about-dark.png` | Settings -> About: version, AGPL-3.0 license, commercial licensing contact and open-source notices. |
| `dialog-add-server.png` | Add any standard Mumble server by address, or paste a mumble:// link. |
| `dialog-certificate-trust.png` | Mumcord asks before trusting a server certificate it has not seen before. |
| `banner-rejoin.png` | After a restart, Mumcord offers to rejoin the channel you were in. |

## Regenerating

1. Start two dev servers on free ports (not 1420) with the mock forced:
   `VITE_MUMCORD_MOCK=1 VITE_MUMCORD_MOCK_SEED=demo npx vite --port 1437` and
   `VITE_MUMCORD_MOCK=1 VITE_MUMCORD_MOCK_SEED=demoFresh npx vite --port 1438`.
2. Open them in a headless Chromium (Playwright) at 1440×900, device scale factor 2, locale
   `en-US`, in dark and light colour scheme, and drive the UI to each screen. The mock's controls
   (`getApi().mock`, e.g. `setTalking`, `requireCertTrust`, `offerRejoin`, `receiveMessage`, `emit("nowPlayingChanged", …)`) set up
   talking, the certificate prompt, the rejoin banner and unread messages.
3. Copy `main-dark.png` to `docs/images/mumcord-main.png` for the README.
