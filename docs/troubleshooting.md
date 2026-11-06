# Troubleshooting

## OpenGL games need a launch option

Applies to: Linux, in-game overlay.

Mumcord's in-game overlay loads into games through a Vulkan layer. Native games that draw with OpenGL (for example Unity games that force OpenGL) do not use Vulkan, so the overlay does not appear in them by default.

Fix, once per game:

1. In Mumcord open **Settings → Overlay** and turn the **In-game overlay** on.
2. Under **OpenGL games** press **Copy**. The text looks like `LD_PRELOAD=<data folder>/app.mumcord.client/overlay-layer/current/libmumcord_overlay_layer.so %command%`.
3. In Steam open the game's **Properties → General → Launch Options** and paste the text.
4. Start the game. Mumcord's log shows `a game connected api=OpenGL` and the overlay appears.

Notes:

- The path ends in `current`, a link Mumcord keeps pointed at the installed version. It stays the same after Mumcord updates and after restarting the AppImage, so the launch option does not need to be changed again.
- Add it only to games that need it. Mumcord never sets `LD_PRELOAD` globally.
- Vulkan games need no launch option.
- If the game already has launch options, put the `LD_PRELOAD=...` part first and keep the rest, for example `LD_PRELOAD=... -windowed %command%`.
- Flatpak Steam keeps its own copy of the library in its private data folder; the pasted path is the host path, so check that the game can read it (OpenGL support for Flatpak Steam is not covered here).

## Camera problems

Applies to: camera sharing (0.7.0 and later), all systems.

### The camera button is disabled

Camera sharing always goes through the server's relay (mumpeer), end-to-end encrypted. The button and the "Turn on camera" entry in the Apps menu say why they are disabled:

| Message                                                               | What to do                                                                                   |
| --------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| "This server has no relay, so camera sharing isn't available."        | Ask the server owner to add a mumpeer relay. Mumble servers without one can't carry cameras. |
| "This server's relay is too old for camera sharing."                  | The relay needs an update by its owner.                                                      |
| "This server's relay doesn't allow camera sharing."                   | The relay owner turned cameras off.                                                          |
| "Camera sharing needs the relay. Change it in Privacy & Connections." | Your peer-to-peer setting keeps traffic off the relay. Change it in Settings → Privacy.      |
| "Camera sharing needs a client certificate."                          | Connect with an identity that has a certificate.                                             |

### No camera found

"No camera found. Connect a camera and it appears here." means Mumcord sees no camera. Plug the camera in; the list updates by itself. On Linux, Mumcord asks the desktop's camera portal first and falls back to `/dev/video*` devices.

"Camera stopped: another app is using your camera." means another program holds the camera. Close it and turn the camera on again. "Camera stopped: the camera was disconnected." appears when the camera is unplugged while it is on.

### Camera access denied

| System  | Message                                         | Fix                                                                                                  |
| ------- | ----------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| Windows | "Windows blocks camera access for desktop apps" | Press **Open camera settings** and allow camera access for desktop apps.                             |
| macOS   | "macOS blocks camera access for Mumcord"        | Press **Open Privacy settings** and allow Mumcord under Camera.                                      |
| Linux   | "Camera access was denied"                      | The desktop's camera portal refused access. Press **Try again** and allow it in the portal's dialog. |

### Blur and backgrounds are not available

Background effects (blur Light or Strong, built-in and own images) run on your own computer with ONNX Runtime, which ships with Mumcord. If it can't be used, the preview says "Background effects are not available on this PC: …" with the reason, and the camera still works without a background:

- "the ONNX Runtime library is missing." The installation is incomplete. Reinstall Mumcord from the official package; the library sits next to `Mumcord.exe` on Windows, in the app bundle on macOS and in `lib/mumcord` on Linux.
- "the ONNX Runtime library failed to load (…)." The library is there but could not be loaded. The log names the reason (`ONNX Runtime failed to load`).

"Your PC is too busy for this background effect. Try None." means the effect can't keep up. Pick None, or set **Settings → Camera → Effect quality** to **Light (saves battery)**.

### Camera stops or doesn't start

- "Camera couldn't start: this server's relay is full right now." or "The camera room is full (9 of 9 cameras)." The relay or the channel has reached its camera limit. Try again later.
- "Camera stopped: the relay isn't reachable" The connection to the relay was lost. Mumcord turns the camera off; turn it on again once the relay is back.
- "Camera couldn't start: too many attempts. Try again in a minute."
