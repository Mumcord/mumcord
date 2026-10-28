# Packet inspector

A hidden debugging screen that shows the packets Mumcord sends and receives, mainly its own
plugin-data messages (issue #60). It is meant for checking the plugin-data protocol in the field,
for example why a late joiner did not see a running screen share.

## Unlocking and hiding

1. Open Settings → About.
2. Click the version number seven times within a few seconds.
3. "Developer tools unlocked" appears, and a **Packet inspector** section is added at the end of
   the settings list.

The section has a **Developer tools** switch; turning it off hides the section again, stops the
capture and closes the inspector window. The choice is stored on this device only (in the app's
local web storage), not in the settings file.

## The inspector window

**Open packet inspector** in that section opens the inspector in a window of its own, so it can
stay next to the main window while you reproduce a problem. Clicking it again while the window is
open brings that window to the front; there is never more than one.

- The window remembers its size and position on this device (`packet-inspector-window.json` in
  the app's data folder). If the screen it was on is gone, the system places it.
- It follows the main window's theme, accessibility hints and language as they change, and shows
  the names the main window knows.
- **Ctrl+W** (⌘W on macOS) or the window's close button closes it; Esc does nothing, so it cannot
  be closed by accident. Closing it does not quit Mumcord; quitting Mumcord closes it.
- Capture does not depend on the window: it runs while the developer tools are unlocked, so
  packets sent while the window was closed are there when it opens (within the ring buffer).
- The window can only read the capture. It may call the inspector commands (fetch, clear, export)
  and the app version for exports, listen to events and show the save dialog for exports; every
  other command (connecting, sending, settings, deep links) is refused for it.

## What is captured

Capture runs only while the developer tools are unlocked. When they are locked, each connection
checks one flag per message and does nothing else.

| What                                              | Kept                                                            |
| ------------------------------------------------- | --------------------------------------------------------------- |
| Plugin data (`PluginDataTransmission`), both ways | Type (data id), size, sender or receivers, pacer state, payload |
| Other Mumble control messages, both ways          | Type and size only                                              |
| Voice (UDP and tunnelled through TCP)             | Counters only (sent and received per server)                    |

All plugin data is captured, Mumcord's own (`mumcord.hello`, `mumcord.history`, `mumcord.image`,
`mumcord.nowgaming`, `mumcord.nowplaying`, `mumcord.p2p`, `mumcord.screen`, `mumcord.watch`) and
that of other clients or plugins. Standard control messages such as `TextMessage` or `UserState`
are listed with their type and size so the order and timing around plugin data stays visible, but
their content (chat text, names, comments) is not kept. The login messages `Version` and
`Authenticate` are written before capture starts and do not appear.

Sent packets are captured when they actually leave the client, after the client pacer: the
**Pacer** column shows how long a plugin-data message waited (`delayed 340 ms` or `not delayed`)
and how many messages the pacer still lets through at once (`tokens left`).

**Size** is the size of the plugin data itself (the part the server's 1,000-byte limit applies
to), or of the whole message for other types.

## Privacy

- Captured packets live in memory only, in a ring buffer of the last **2,000** packets for all
  servers together. Nothing is logged or sent anywhere.
- Hiding the developer tools (or quitting the app) drops everything captured.
- Content leaves the device only when the user exports it. Exports start with a warning that they
  can contain personal data such as names and chat text.
- Everything shown is treated as untrusted input and rendered as plain text.

## Reading the list

One row per packet, oldest first: time (`HH:MM:SS.mmm`, local time), direction (↑ sent / ↓
received, also told apart by colour), type (plain name plus the internal id such as
`mumcord.watch`), size, from / to (user name and session id; sends to several people show
"→ N users", which expands), and pacer state.

Names are the ones the app knows right now; someone who has left shows as `session 7`.

Click a row (or press Enter on its time) to expand it:

- the gap to the previous packet in the list (`+340 ms`), to spot bursts or resends,
- the decoded content as highlighted JSON. Mumcord's messages are JSON; history sync shows its
  JSON header and the size of the binary body; full-image transfers show their decoded fields;
  any other UTF-8 payload shows as a string,
- **Show raw hex** for the exact bytes of the plugin data,
- **Copy packet** copies the packet as Markdown.

Filters (server, direction, type), **Pause** / **Resume** and **Clear** act on the list; the
counters show packets per type and direction since capture started or was cleared, including
those the ring buffer already dropped, plus the voice packet counts of each connected server.

## Export

- **Export JSON**: complete and machine-readable, every field including decoded content and raw
  hex.
- **Export Markdown**: readable, for pasting into an issue: a table with the same columns, then
  the decoded content per packet.

Both export what the current filters show and include the app version, the OS, the export time and
the time range of the packets.

## Decisions

The issue left these open; this is what was built:

- **Scope**: all plugin data both ways, standard control messages as type and size only (cheap to
  take at the protocol layer), voice as counters only.
- **Unlock**: seven clicks on the version in Settings → About; hidden again with a switch in the
  section. Stored locally.
- **Buffer**: 2,000 packets in memory; capture only while unlocked.
- **Updates**: the UI fetches new packets every 250 ms while the section is open, so a burst never
  floods it. Nothing is pushed while the section is closed.
- **Export**: JSON and Markdown of the filtered list; copy one packet as Markdown.
