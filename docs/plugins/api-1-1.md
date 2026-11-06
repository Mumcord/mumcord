# Plugin API 1.1: author guide

API 1.1 (`mumcord:plugin@1.1.0`) extends API 1.0 with new places to show your plugin, websites, relay data, sounds and a few actions that Mumcord performs for you. Everything in 1.0 keeps working unchanged. Minors only add: a 1.0 plugin never sees 1.1 imports or events, and a plugin gains a new permission only through a new version that you approve.

Mumcord 0.7.0 runs API 1.0 and 1.1.

## Using 1.1

In the Rust SDK, turn on the feature:

```toml
mumcord-plugin-sdk = { version = "...", features = ["1.1"] }
```

You never type the manifest field `api`: `mumcord-plugin build` writes `"^1.1"` from the imports your component uses, and `mumcord-plugin check` fails when `api` and the imports disagree. The feature adds typed wrappers for every new interface (`stage`, `app_menu`, `relay`, `audio`, `files`, `chat::post_image`, the new `webview` places) and the new events and errors.

Start from a template:

```sh
mumcord-plugin new my-activity --template stage-tree   # activity drawn from building blocks
mumcord-plugin new my-game --template stage-web        # activity with its own web page
```

Without `--template` you get the API 1.0 hello plugin. Both stage templates declare `ui.stage` and `ui.app-menu`, an Apps-menu item that starts the activity, and (for `stage-web`) `ui.webview` with the place `stage`. See also `crates/plugin-sdk/examples/stage_board.rs`.

Users on Mumcord 0.6.x cannot run a 1.1 plugin. The registry shows them "Needs Mumcord 0.7.0 or newer" instead, and a local package is refused before anything is installed. See [plugin registry format](../plugin-registry-format.md) for how versions are listed.

## Surfaces

A surface is a place where your plugin shows something. Mumcord draws the frame around it (plugin icon and name, an "Unofficial" chip for unofficial plugins, a close button) so people always know whose content they are looking at.

| Surface       | Permission                       | Content                     | Where                                                           |
| ------------- | -------------------------------- | --------------------------- | --------------------------------------------------------------- |
| Stage         | `ui.stage`                       | building blocks or web page | the media area, as a tab next to screen shares and cameras      |
| Side panel    | `ui.side-panel` and `ui.webview` | web page                    | a docked column right of the chat, 18 to 32 rem wide, resizable |
| Dialog        | `ui.webview` (place `dialog`)    | web page                    | a modal dialog up to 46 × 34 rem                                |
| Panel popover | `ui.panel`                       | building blocks or web page | the existing popover, 420 × 560                                 |
| Apps menu     | `ui.app-menu`                    | menu items                  | the Apps menu and the command palette                           |

Web content comes only from your package (`web/`). Declare where a page may appear in the manifest: `"webview": { "places": ["stage", "side-panel", "dialog", "panel"] }`. At most three web pages are loaded at once; a fourth call returns `limit`.

**Stage.** `stage.start(title, content)` starts one activity in the user's current voice channel; `content` is a building-block tree or `web(entry)`. A plugin has at most one activity per channel, a channel at most three plugin activities, and `stage: { "title", "max_participants" }` caps the players (16 at most). Others join with a click on "Join", never automatically. Use `join`, `leave`, `end` and `set-tree`; you receive `stage-activity-started`, `stage-activity-ended`, `stage-joined`, `stage-left` and `stage-visibility-changed`. Only channel members who have the same plugin are told about an activity.

**Side panel.** Web content only. `set-size-hint(min-width, min-height)` tells Mumcord how much room the page needs. `set_status` (SDK `webview::set_status`) shows a plain-text status line under the panel: at most 80 characters, at most 4 updates per second, the last one wins. One side panel is open at a time; the old one gets `surface-closed` when another opens.

**Dialog.** A modal Mumcord dialog with your page in it. Esc closes it.

**Apps menu.** Declare up to two items in the manifest: `"app_menu": [{ "id", "label", "icon" }]`. Labels are at most 32 characters, translated, and may not equal a built-in label or start with "Mumcord". Items always show your plugin's name. You can only switch an item on or off with a reason text (`app-menu.set-state(item, enabled, reason)`); choosing one sends `app-menu-activated` and counts as a user gesture. The command palette lists the same items as "<plugin>: <label>".

## The gesture rule

A plugin can open the media area, a side panel or a dialog, save a file, open a file picker or post an image only **within 8 seconds after a user gesture for that plugin**. A gesture is:

- choosing one of your Apps-menu or command-palette items, pressing a button in your panel, or running your slash command; or
- a genuine mouse click or key press by the user inside your plugin's own web page (stage, side panel, dialog or panel content).

Mumcord detects the second kind itself from trusted input events. A click your page fakes with script (synthetic or dispatched events) never counts. Joining someone else's activity is always the user's own click on Join. Without a gesture the call returns `not-permitted` and nothing opens. Plan your flows so that the call happens as the direct result of a click.

## Websites: `net.origins`

An unofficial plugin can talk to the internet only through websites it declares:

```json
"net.origins": [{ "origin": "https://api.example", "purpose": "origins.api" }]
```

`purpose` is a translation key; its text is shown to the user next to the origin. The rules:

- exact origins `https://host[:port]` or `wss://host[:port]`; no path, query, fragment, user info or wildcard;
- at most 8; no IP addresses; no `localhost`, private, link-local or `.local` names (a plugin in Developer mode may use `http://localhost:<port>`, a packaged plugin may not);
- anything breaking a rule makes the package impossible to install.

The same list governs your WASM core (`net.http`) and your web page (the frame's content policy). Anything not listed is blocked. For `net.http` the address must resolve to a public address, redirects to other origins fail, and the certificate must match the declared name. A refused request returns `origin-not-allowed`.

Websites can serve data, images, fonts and media, never scripts. Users see every website with its purpose before installing, and an update that adds one waits for their approval. The official registry reviews every website a plugin declares. The more websites you declare, the higher your [security score](../plugin-score.md).

## Relay data: `peers.relay` and Bovnar

`peers.relay` (optional) lets your plugin send game data through the Mumble server's relay instead of Mumble's small plugin-data messages. Relay data is end-to-end encrypted: the relay sees only the size and a sequence number, never your plugin id or content. Check `relay.status()` first: `available`, `not-allowed`, `no-relay` or `switched-off`. Then `send-channel(data, fallback)` or `send-direct(peer, data, fallback)`; incoming data arrives as `relay-message(peer, data)`, once and in order per sender, and `relay-status-changed` tells you when availability changes.

Limits: 64 KiB per message, 256 kbit/s sustained with a 1 MiB burst. With `fallback = true`, a message of at most 997 bytes goes through Mumble plugin data when no relay is available; anything else returns `relay-unavailable`. Relay data has the lowest priority, behind voice, screen sharing, camera and files. Users can switch it off for all plugins or per plugin (by turning off `peers.relay`); then `relay.status()` is `switched-off`, you get `relay-status-changed`, and your sends behave as without a relay: small ones with `fallback = true` still go through Mumble plugin data.

Do not invent your own wire format. [Bovnar documents](peer-data-bovnar.md) are the recommended way to encode typed messages, with a versioning rule and size checks built in.

## Sounds: `audio.effects`

Declare sound files in `"audio": { "sounds": ["sounds/ding.ogg"] }` and play them with `audio.play(sound, gain)` or `audio.stop-all()`. Rules: Ogg Vorbis or WAV, each at most 10 seconds and 1 MiB, at most 32 files, counted toward the package size. Sounds are played only on the user's own computer, never sent to anyone else, lowered by 12 dB while anybody talks, muted while the user is deafened, and at most four play at once across all plugins. Users control the volume in Settings → Plugins. A sound name not declared returns `not-found`.

## Files and pictures

Mumcord performs these actions itself and never gives your plugin a file path:

- `files.save(suggested-name, data)` (permission `files.save`) opens the system save dialog. It returns `true` when saved and `false` when the user cancelled. The name is cut down to a plain file name.
- `files.import-picture()` (permission `files.import-picture`) opens a picker for PNG, JPEG or WebP and returns the name and bytes, or nothing if cancelled. Pictures over 10 MB return `too-large`.
- `chat.post-image(png, alt)` (permission `chat.post-image`) posts a PNG into the user's current channel through the normal chat image path (or as a file offer when it is too big). Invalid PNG data returns `invalid`.

All three need a fresh user gesture ([the gesture rule](#the-gesture-rule)): one dialog or one image per gesture. Mumcord keeps no copy of the bytes and never logs file contents or names. The mock host of `mumcord-plugin test` always answers them with `not-permitted`; try them in Mumcord itself.

## The frame security model

Web content runs in a sealed frame:

- Content comes **only from your package**, served by Mumcord. The frame has no access to Mumcord, your files, the clipboard, microphone, camera, cookies or storage, and it cannot navigate, open popups, download files, show `alert` or submit forms.
- The content policy is sent by Mumcord as a header, so the page cannot weaken it: scripts only from your own package (plus WebAssembly), no `eval`; images, media, fonts and connections only from your package and your declared websites; no frames, plugins or `<base>`.
- **No remote code.** Scripts, WebAssembly and anything executable come from the package you published. The official registry rejects plugins that load code from a website.
- The only line to the outside is the message bridge to your WASM core: strings, 16 KiB per message (64 KiB on the stage), 60 messages per second each way.
- Sound from a page needs a click inside it first, and Mumcord shows a mute button. A page that stops answering for three seconds is removed.
- Keyboard: Esc or F6 always returns focus to Mumcord.
- Mumcord never asks for passwords in a frame. Neither should you.

## Security score and publishing

Unofficial plugins show a [security score](../plugin-score.md). Keep it low by asking only for what you use, making permissions optional where possible, linking your source, declaring an OSI licence, publishing reproducible builds and keeping websites to a minimum. A version that adds permissions or websites is not installed automatically.

Repository owners: publish API 1.1 versions only in the `v1.1` index ([format](../plugin-registry-format.md)). Link the release notes of a version with `notes_url`.
