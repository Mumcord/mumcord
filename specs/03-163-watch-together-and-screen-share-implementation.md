# Epic 3 · Issue #163 – Watch together and screen share · Implementation report

> Specification: `specs/03-comfort-polish-specification.md` (FR-3.119–FR-3.127) · Issue: #163
> Task list: `specs/03-163-watch-together-and-screen-share-tasks.md`
> Mockups: wiki `watch-together-{dark,light}.png`, `screen-share-viewer-{dark,light}.png`, `screen-share-sharer-{dark,light}.png`
> As of: 2026-11-01 · Branch: `degenhard/epic-3-redesign-the-watch-together-and-screen-share`

### Result in one sentence

Both views are rebuilt on one shared "media view" layer: the watch party and a watched screen share now open as a view in the main column with the chat below it, leave as a mini bar, share the same keyboard keys and the same picture frame, show a real sync state per person and the real figures of a stream, and everything is in all 24 languages; what is still missing is the run against the local test server and the dark/light screenshots, so the issue cannot be closed yet.

## What was built

**Protocol and engine**
- Watch party goes to protocol version 3 with a `pos` message: every participant reports its own position, so a sync state per person is possible (the old protocol only carried a position with the play/pause/seek action). Reports are throttled — at most one every 10 seconds while playing, immediately when someone starts buffering — so a party of 16 costs about 800 bytes a minute. Older clients ignore the unknown message type, so an unmodified murmur keeps working (client-only rule).
- Screen share signal goes to version 2 with `kind` (window or screen) and `elapsed_ms` (FR-3.127).
- Per viewer quality and path are readable (`fps`, `struggling`), and the live sound level of a share is exposed for the meter in the overlay (FR-3.126).
- The player page reports duration, buffered range and its own volume; volume changes are pushed to it (T7).

**Shared media layer** (`ui/src/features/media/`)
- `MediaFrame` (top overlay, control row, full-screen button, fade-in on hover/focus/pause), `MediaPanel` (tab panel, reduced-motion-safe), `MiniBar` (the strip above the chat), `SeekBar` (buffered range + position markers of the other players), `VolumeControl` (slider + optional live meter), `ShortcutHint` (first use, respecting the setting), `useMediaKeys` (Space, `←`, `→`, M, F, Z, 1, Esc), `useTick`, `format.ts` (`clockTime`, `bitrate`, `behindSeconds`, `secondsSince`).
- One `mediaView` store: `cinema` (`watch` | `share` | `none`), `mini`, `focus`, `fit`, `tab`. Focus hides the other columns (FR-3.123); the picture scales with the window; reduced motion keeps the overlays visible (FR-3.121).
- Only the top-level `App` renders the frame, so the watch party and a share can never both show a picture, and one Esc leaves whichever is open (FR-3.122).

**Watch together** (FR-3.122–FR-3.124)
- `WatchTogetherView` in the main column: poster, title, who is in control, the adder, the sync summary, seek bar, volume, time, and play/pause/next (no "previous": a party has no history — see deviations).
- `WatchingList` with the host crown and each person's sync state (`syncState.ts`: tolerance, buffering, ahead/behind, stale reports), `SyncPill`, `ReportPosition`, `PlaylistPanel`, `WatchMiniBar`, `useWatchPlayer`.
- The old `WatchPartyWidget` is gone; the party reaches the UI through the API (`watchPartyReport`, `ParticipantPosition`, `host`).

**Screen share** (FR-3.125–FR-3.127)
- `ScreenShareView` in the main column: header (sharer, LIVE, what was picked, since when), chips for resolution / frame rate / bitrate / path / sound, the *Stream details* popover (codec, delay against the fastest frame, dropped frames), overlay with volume + live level, Fit / Fill / 1:1, Focus, Stop watching.
- `ViewerPanel`: *Watching (n / 8)* with the frame rate each viewer gets and the hint when quality is lowered for the slowest one.
- `SharerBar`: the control bar at the top of the chat — LIVE, elapsed, source and quality, Pause, Sound, Change window, Viewers, Stop. It replaces the red sidebar bar; the viewers line lives on as its own component (`Viewers.tsx`), still used by the share row in a chat card.
- `streamStats.ts` counts frames, bytes, drops and the relative delay; the renderer reports them.

**Accessibility and i18n**
- New keys in `common` for all 24 locales (view, mini bar, controls, sync, quality, focus, leave, fullscreen, shortcuts, watching, control, live, volume, time, details), plus the new `screenviewer` strings; `ui/tests/i18n.test.ts` and `languageCoverage.test.ts` pass. The `de`/`en` values follow the mockup wording; the other languages are machine-level translations and need a human pass (especially ga, mt, hu, hr, et, lv).

## Decisions taken with the product owner

1. Control stays with everyone ("everyone may control"); the header shows who acted last. "Request control" is dropped.
2. Per-person sync states need the protocol addition above — agreed, because otherwise the feature cannot be built as the issue describes.
3. "Picture-in-picture" stays the in-app mini player for #163; a real system PiP / always-on-top window is #164.
4. Watch party stays YouTube-only; the UI says "YouTube link" (Twitch cannot be synchronised through the party protocol).
5. Verification depth: unit tests + two e2e specs + dark/light screenshots.

## Deviations from the issue (need a reason in the PR)

- **No "Previous" button.** The party protocol has no history, so there is nothing to go back to. The playlist keeps the entries; the issue's "play/pause, previous, next" cannot be met in full. Needs a product decision.
- **Playlist total duration** is only known for the entry that plays — the engine's link preview does not return durations for the other entries. The list shows duration where it is known, nothing elsewhere.
- **PiP** as described in the issue (picture over the other columns) is the mini player + focus mode, not a system window (#164).
- The issue's "Chat tab" in the right panel: the panel shows the *Watching* list; chat remains below the view. Not implemented as a tab yet.

## Checks (as of this report)

| Check | Result |
|---|---|
| `npm run typecheck` | clean |
| `npm test` | 157 files, 1945 tests pass |
| `npm run lint` | 0 errors, 4 fast-refresh warnings (same class as existing files) |
| `cargo test -p mumcord-engine --lib` | 402 pass |
| i18n tests | 102 pass |
| Live tests against the local test server (`testenv/up.sh v15`) | ✅ 2026-11-01: `live_watch_party` 4 pass (new: each player reports where it is, host/crown), `live_screen_share` 2 pass (new: source + elapsed reach the viewer), `live_screen_share_media` 1 pass |
| E2E specs for the two views (UI) | **not written yet** |
| Dark/light screenshots | **not produced** |

## Acceptance criteria status

| AC | Status | Note |
|---|---|---|
| AC-3.119 (media views exist) | code done | not verified in the running client |
| AC-3.120 (single media store) | code done | unit-tested |
| AC-3.121 (reduced motion, overlays) | code done | needs a real client run |
| AC-3.122 (watch view, sync state) | code done; protocol part verified live | UI part needs a running client |
| AC-3.123 (focus mode) | code done | needs a real client run |
| AC-3.124 (mini bar + shortcuts) | code done | needs a real client run |
| AC-3.125 (share view) | code done | needs a real client run |
| AC-3.126 (quality figures live) | code done; media path verified live (`live_screen_share_media`) | figures unit-tested |
| AC-3.127 (sharer control bar) | code done | needs a real client run |

## Next steps

1. Write and run the two e2e specs (UI) against the local test server (watch together: second client joins, control moves, sync within 2 s; screen share: figures change, viewer count, quality for one viewer).
2. Dark and light screenshots of all three views in the running client → `docs/images/screenshots/`, embedded in the PR description.
3. Human pass on the new translations (ga, mt, hu, hr, et, lv especially).
4. Decide: "Previous" button, playlist durations, Chat tab in the panel.
5. Push and open the PR only on request.

## Commits

`ea83ba71` docs(spec): task list · `d640d612` engine positions · `88ad9e8f` engine share figures · `36696302` API commands · `f2a470c5` player page reports · `2b1134c8` shared media layer · `30c18802` watch cinema view · `b6a636c8` share viewer view · `4c6a07e8` 24 languages · `e489831c` sharer control bar · `82c901c4` + `894129e9` changelog.
