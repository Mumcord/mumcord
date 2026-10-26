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
