# Screenshots

Screenshots for the project website and the README. All of them show the UI running on the
built-in mock engine with fictional demo data (people, servers and messages are made up; addresses
use `example.org`). Size: 1440×900 at 2× scale (2880×1800 pixels).

| File                                | Caption                                                                                                                                                                |
| ----------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `main-dark.png`                     | Talk and chat in one window: servers on the left, channels with avatars, who is talking (green ring), chat, and who is in the channel – including people listening in. |
| `main-light.png`                    | The same main view in the light theme.                                                                                                                                 |
| `wizard-1-identity.png`             | First start, step 1: create your identity with just a name, or bring the one you already use.                                                                          |
| `wizard-2-microphone.png`           | First start, step 2: pick microphone and speakers and test them with a live level meter.                                                                               |
| `wizard-3-talk-mode.png`            | First start, step 3: choose voice activation or push-to-talk with your own key.                                                                                        |
| `settings-01-identity-dark.png`     | Settings → Identity: back up your identity and set a default avatar.                                                                                                   |
| `settings-01-identity-light.png`    | Settings → Identity in the light theme.                                                                                                                                |
| `settings-02-audio-dark.png`        | Settings → Audio: devices and a microphone test with noise suppression applied.                                                                                        |
| `settings-03-voice-dark.png`        | Settings → Voice: push-to-talk key and voice processing (noise suppression, echo cancellation, automatic volume).                                                      |
| `settings-04-chat-history-dark.png` | Settings → Chat & history: chat history is kept only on your computer, per server.                                                                                     |
| `settings-05-appearance-dark.png`   | Settings → Appearance: follow the system theme or pick dark or light.                                                                                                  |
| `settings-06-language-dark.png`     | Settings → Language: English, Spanish and German.                                                                                                                      |
| `settings-07-sounds-dark.png`       | Settings → Sounds: choose which events play a short sound.                                                                                                             |
| `settings-08-diagnostics-dark.png`  | Settings → Diagnostics: save a diagnostic file without chat messages or personal data.                                                                                 |
| `settings-09-about-dark.png`        | Settings → About: version, license and open-source notices.                                                                                                            |
| `dialog-add-server.png`             | Add any standard Mumble server by address – or paste a mumble:// link.                                                                                                 |
| `dialog-certificate-trust.png`      | Mumcord asks before trusting a server certificate it has not seen before.                                                                                              |
| `banner-rejoin.png`                 | After a restart, Mumcord offers to rejoin the channel you were in.                                                                                                     |
| `follow-unread-badge.png`           | Follow a channel and see unread messages at a glance.                                                                                                                  |

## Regenerating

1. Start two dev servers on free ports (not 1420) with the mock forced:
   `VITE_MUMCORD_MOCK=1 VITE_MUMCORD_MOCK_SEED=demo npx vite --port 1437` and
   `VITE_MUMCORD_MOCK=1 VITE_MUMCORD_MOCK_SEED=demoFresh npx vite --port 1438`.
2. Open them in a headless Chromium (Playwright) at 1440×900, device scale factor 2, locale
   `en-US`, in dark and light colour scheme, and drive the UI to each screen. The mock's controls
   (`getApi().mock`, e.g. `setTalking`, `requireCertTrust`, `offerRejoin`, `receiveMessage`) set up
   talking, the certificate prompt, the rejoin banner and unread messages.
3. Copy `main-dark.png` to `docs/images/mumcord-main.png` for the README.
