# Issue #163 – redesign the watch together and screen share views

The contract is issue #163 plus the mockups (`docs` wiki, `epic3/media/*`), read together with
Spec 03 §4.6 (FR-3.124 – FR-3.128). Pop out (#164) and the relay (#62) stay out of scope.

Decisions taken with the product owner (2026-11-01):

- **Control:** everyone may play, pause and seek, as today (FR-3.128). "Request control" is dropped;
  the header shows who acted last ("Maya is in control").
- **Sync states:** the party protocol only carries a position on play, pause and seek, so a live
  "Buffering" or "12 s behind" needs each player to report where it is. Amendment to FR-3.125: a
  small version-3 `pos` message, sent on a buffering change at once and otherwise at most every
  10 s. Standard Mumble servers still see only ordinary plugin data; older Mumcord clients ignore
  the new message.
- **Picture-in-picture:** stays with #164 (the pop-out window). #163 ships the mini player docked at
  the top of the chat as the small view.
- **Twitch in the party playlist:** the party still takes YouTube only (engine validates YouTube ids,
  and Twitch's embed has no control API to sync play/pause/seek). The paste field says "YouTube link".

Acceptance criteria (from the issue, verified in the running client against the local test server):

- AC-1 Starting or joining a party opens the cinema view in the main column; the chat stays usable
  below it; leaving the view keeps the mini player.
- AC-2 Play, pause and seek from the overlay reach every member; a member who is buffering shows
  "Buffering" within 2 s.
- AC-3 Playlist reorder by mouse and keyboard is seen by all members; every entry shows who added it,
  its duration and a thumbnail.
- AC-4 The screen share viewer shows resolution, fps, bitrate, path and sound; Fit / Fill / 1:1 and
  Focus work; the viewer list matches the sharer's list.
- AC-5 The sharer can pause, change the window and see each viewer's quality from the control bar.
- AC-6 All controls work with the keyboard (Space, ←/→, M, F, Z, 1, Esc) and a screen reader; light
  and dark themes and reduced motion are correct.
- AC-7 Theme tokens only; all 24 locales translated.
- AC-8 Client only: everything works against an unmodified standard Mumble server.

| ID | Task | Status | Owner | Depends on | Complexity | Covers | Files |
|---|---|---|---|---|---|---|---|
| T1 | Analysis: widget architecture, shell slots, mockups, party protocol | done | lead | – | complex | – | see notes |
| T2 | Engine: per-player position reports and the sync state of each participant | done | lead | T1 | complex | AC-2 | `crates/engine/src/watch_party.rs` |
| T3 | Engine: what a share shows (source, since, own stream stats) and per-viewer quality | done | lead | T1 | complex | AC-4, AC-5 | `crates/engine/src/screen_share/*` |
| T4 | Player page: report duration and buffered range, own volume, state for buffering | done | lead | T2 | complex | AC-2, AC-6 | `src-tauri/src/watch_player/player.html`, `ui/src/features/watchparty/playerBridge.ts` |
| T5 | App shell: media stage slot, focus mode, right column panel, mini bar mount | done | lead | T1 | complex | AC-1, AC-4, AC-6 | `ui/src/app/AppShell.tsx`, `ui/src/app/mounts.tsx`, `ui/src/features/media/*` |
| T6 | UI: shared media frame (overlay, control order, reduced motion, keyboard) | done | lead | T5 | complex | AC-6, AC-7 | `ui/src/features/media/MediaFrame.tsx`, `media.css` |
| T7 | UI: watch together cinema view, seek bar with markers, volume, last action | done | lead | T4, T6 | complex | AC-1, AC-2, AC-6 | `ui/src/features/watchparty/WatchTogetherView.tsx` |
| T8 | UI: playlist panel with tabs, thumbnails, duration, drag line, ⋯ menu, paste field | done | lead | T6 | complex | AC-3 | `ui/src/features/watchparty/PlaylistPanel.tsx` |
| T9 | UI: who's watching list with per-person sync state and host crown | done | lead | T2, T6 | complex | AC-2 | `ui/src/features/watchparty/WatchingList.tsx` |
| T10 | UI: mini player bar at the top of the chat | done | lead | T6 | simple | AC-1 | `ui/src/features/media/MiniBar.tsx` |
| T11 | UI: screen share viewer view (header chips, stream details, bottom overlay, panel) | partly: no Chat tab | lead | T3, T6 | complex | AC-4 | `ui/src/features/screenshare/ScreenShareView.tsx` |
| T12 | UI: sharer control bar | done | lead | T3, T6 | complex | AC-5 | `ui/src/features/screenshare/SharerBar.tsx` |
| T13 | Translations: 24 locales | done (needs a human pass) | lead | T7–T12 | simple | AC-7 | `ui/src/locales/*/watchparty.json`, `*/screenviewer.json`, `*/screenshare.json` |
| T14 | Tests: engine, player bridge, UI units | done (units) | lead | T2–T12 | complex | AC-2, AC-3, AC-4 | `crates/engine/src/*/tests`, `ui/src/features/**` |
| T15 | Verification: build, tests, type checks, linters | done | lead | T14 | simple | – | – |
| T16 | Verification in the running client against the local test server + dark/light screenshots | open | lead | T15 | complex | AC-1–AC-6 | `e2e/specs/media/`, `docs/images/screenshots/` |
| T17 | Report `specs/03-163-watch-together-and-screen-share-implementation.md` | done | lead | T16 | simple | – | – |

## Notes

- The floating widgets (FR-2.70) go away for these two features; nothing else in the app uses the
  `FloatingWidget` component, so it, its geometry, store, CSS and tests are removed. Spec 02 AC-2.51
  then applies to the mini bar instead of a draggable widget.
- Both views share one frame: header (who, what, live, chips), the picture, an overlay control row,
  and the right column panel. The mini bar is the same shape for both.
- The path chip shows the path the engine actually used. Today screen share is direct only, so it
  reads "Direct"; the relay (#62) can put "Relay" there without a UI change.
- The buffered range and duration come from the player page; the YouTube control protocol reports
  duration, and buffered is only shown when the page can report it.
