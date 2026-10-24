# Screenshots

Screenshots for the project website and the README. All of them show the UI running on the
built-in mock engine with fictional demo data (people, servers and messages are made up; addresses
use `example.org`). Size: 1440×900 at 2× scale (2880×1800 pixels).

| File                                | Caption                                                                                                                                                                |
| ----------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `main-dark.png`                     | Talk and chat in one window: servers on the left, channels with avatars (stacked when crowded), who is talking (green ring), chat with avatars, your user panel with microphone level, and who is in the channel – including people listening in. |
| `main-light.png`                    | The same main view in the light theme.                                                                                                                                 |
| `wizard-1-identity.png`             | First start, step 1: create your identity with just a name, or bring the one you already use.                                                                          |
| `wizard-2-microphone.png`           | First start, step 2: pick microphone and speakers and test them with a live level meter.                                                                               |
| `wizard-3-talk-mode.png`            | First start, step 3: choose voice activation or push-to-talk with your own key, and optionally open mumble:// links and start Mumcord at login.                                                                                        |
| `settings-01-identity-dark.png`     | Settings → Identity: back up your identity and set a default avatar.                                                                                                   |
| `settings-01-identity-light.png`    | Settings → Identity in the light theme.                                                                                                                                |
| `settings-02-audio-dark.png`        | Settings → Audio: devices and a microphone test with noise suppression applied.                                                                                        |
| `settings-03-voice-dark.png`        | Settings → Voice: push-to-talk key and voice processing (noise suppression, echo cancellation, automatic volume).                                                      |
| `settings-04-chat-history-dark.png` | Settings → Chat & history: chat history is kept only on your computer, per server.                                                                                     |
| `settings-05-appearance-dark.png`   | Settings → Appearance: follow the system theme or pick dark or light, choose the chat layout and window behaviour, and start Mumcord at login.                                                                                                  |
| `settings-06-language-dark.png`     | Settings → Language: English, Spanish and German.                                                                                                                      |
| `settings-07-sounds-notifications-dark.png` | Settings → Sounds & notifications: choose which events play a short sound and how new messages are announced (in-app pop-up, system notification, both or off). |
| `settings-07-sounds-notifications-light.png` | Settings → Sounds & notifications in the light theme.                                                                                                      |
| `settings-05-appearance-light.png`  | Settings → Appearance in the light theme: colour theme, chat layout (cozy or compact), window behaviour and start at login.                                           |
| `chat-compact.png`                  | Compact chat layout: time, name and message on one line, for busy channels.                                                                                            |
| `channel-avatar-stack.png`          | Crowded channels (more than five people), with the stack on its own line show a stack of avatars with a "Show all" button instead of a long list.                                                      |
| `notification-toast.png`            | A small in-app pop-up announces a new message in another channel.                                                                                                      |
| `settings-08-diagnostics-dark.png`  | Settings → Diagnostics: save a diagnostic file without chat messages or personal data.                                                                                 |
| `settings-09-about-dark.png`        | Settings → About: version, license and open-source notices.                                                                                                            |
| `dialog-add-server.png`             | Add any standard Mumble server by address – or paste a mumble:// link.                                                                                                 |
| `dialog-certificate-trust.png`      | Mumcord asks before trusting a server certificate it has not seen before.                                                                                              |
| `banner-rejoin.png`                 | After a restart, Mumcord offers to rejoin the channel you were in.                                                                                                     |
| `follow-unread-badge.png`           | Follow a channel with its star and see unread messages at a glance, plus a pop-up for new ones.                                                                                                                  |

## Regenerating

1. Start two dev servers on free ports (not 1420) with the mock forced:
   `VITE_MUMCORD_MOCK=1 VITE_MUMCORD_MOCK_SEED=demo npx vite --port 1437` and
   `VITE_MUMCORD_MOCK=1 VITE_MUMCORD_MOCK_SEED=demoFresh npx vite --port 1438`.
2. Open them in a headless Chromium (Playwright) at 1440×900, device scale factor 2, locale
   `en-US`, in dark and light colour scheme, and drive the UI to each screen. The mock's controls
   (`getApi().mock`, e.g. `setTalking`, `requireCertTrust`, `offerRejoin`, `receiveMessage`) set up
   talking, the certificate prompt, the rejoin banner and unread messages.
3. Copy `main-dark.png` to `docs/images/mumcord-main.png` for the README.
