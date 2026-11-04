# Epic 4.1 – File sharing and screen-sharing upgrade · Brainstorming

> The specification `specs/07-file-sharing-screenshare-specification.md` was first written directly from the agreed issues (#348, #355, #386, #400) without a brainstorming file. This file starts with the first addendum and records later decisions. Append only.

## Addendum: application and desktop audio (#435)

> Date: 2026-11-04 · Source: issue #435 (application audio while screen sharing on Linux with PipeWire, milestone 0.7.0) and the user's answers in chat on 2026-11-04. The questions were asked and answered in chat; they are recorded here so the specification can reference them.

**Today (before this addendum):** Windows shares only the shared app's sound (Windows 10 version 2004 or newer) and no sound for a whole screen. macOS shares only the shared content's sound, never Mumcord's own (macOS 13 or newer). Linux shares no sound at all; the Sound switch is hidden there. The specification listed "Share sound on Linux (PipeWire)" as later.

### QA.1 – How does the user choose which sound is shared?

- [x] ⭐ **Automatic from the system picker:** sharing a whole screen shares the whole desktop sound; sharing one window shares only that app's sound. No separate sound picker in the normal case.
- [ ] A separate sound picker on every share (desktop, one app, or no sound).
- [ ] Desktop sound only, regardless of the picker.

✍️ Note: Answered in chat 2026-11-04: automatic from the picker; no extra sound picker in the normal case.

### QA.2 – Is Mumcord's own sound part of the shared sound?

- [x] ⭐ **Always excluded:** voices, notification sounds and every other sound Mumcord plays are never shared, so viewers hear no echo. Not switchable.
- [ ] Excluded by default, with a switch to include it.

✍️ Note: Answered in chat 2026-11-04: always excluded, not switchable.

### QA.3 – Which systems get the new sound in 0.7.0?

- [x] ⭐ **All three systems in 0.7.0:**
  - macOS 13 or newer: the system's screen capture with audio for the chosen window or display, excluding Mumcord.
  - Windows 10 version 2004 or newer: per-process loopback for the chosen window's app; for a whole screen, whole-system loopback that excludes Mumcord's own process.
  - Linux with PipeWire: the whole desktop sound minus Mumcord's own streams. For a window the desktop portal doesn't reveal which app owns it, so Mumcord matches the PipeWire audio app by name or process. When the match is unclear, a small "Which app's sound?" list asks the user. Without PipeWire there is no sound, as today.
- [ ] Linux only in 0.7.0; Windows whole-screen sound later.

✍️ Note: Answered in chat 2026-11-04: all three systems in 0.7.0, as described.

### QA.4 – What happens to the existing Sound switch in the share start dialog?

- [x] ⭐ **It stays** as an on/off switch on top of the automatic choice: on = share the automatically chosen sound, off = share no sound.
- [ ] Remove it; sound is always shared when available.

✍️ Note: Answered in chat 2026-11-04: the switch stays as on/off on top of the automatic choice.

### Follow-up for the specification

- Minimum system versions and the behaviour below them (no sound, with a clear note in the dialog) are written into the specification from QA.3.
- Privacy: only the chosen source is captured, and nothing about it (app names, window titles, process ids) is logged.
- Mockups needed before building: the "Which app's sound?" list, the sound line in the share start dialog and the sound line in the live share bar.
