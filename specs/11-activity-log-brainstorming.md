# Epic 11 — Activity log (server event feed) · Brainstorming draft

> Status: decisions recorded — the specification is `specs/11-activity-log-specification.md`.
> Nothing committed. The exploratory proposal mockups were removed; only the final mockup is kept
> (`docs/images/screenshots/activity-log-p2-twoline-dark.png` / `-light.png`).

## 1. What this is

The activity log is a **server event feed**: a chronological list of what happened on the current
server — who connected or disconnected, who moved channel, who started or stopped a screen share,
a watch party or a plugin activity, plus events pushed by plugins with an action back to the plugin.

It does **not** include:

- chat messages (those already live in the channel chat),
- the user's personal connection history,
- or technical/debug output.

This scope was agreed in chat on 2026-11-06 (option A of three).

## 2. Event types

| # | Event | Action offered |
|---|-------|----------------|
| 1 | You connected (session start) | none |
| 2 | Someone connected / disconnected (you and others) | none |
| 3 | Someone moved channel | none |
| 4 | Screen share started / stopped | Watch (only while still live) |
| 5 | Watch party started / stopped | Join (only while still live) |
| 6 | Plugin activity started / stopped | Join (only while still live) |
| 7 | Plugin-pushed event | plugin-defined action, routed back to the plugin |

## 3. UI proposals considered

Five placements were explored as mockups (since removed): a dedicated page, the members-column tab,
inline in the chat, a compact strip, and an overlay log. **The members-column tab was chosen**, and
after two refinement rounds the final look settled as:

- a second tab **Members | Activity** in the right column,
- **two-line rows with no icons**: line one is the title ("Maya started a screen share"), line two
  is the meta and action ("14:32 · # lobby · live  Watch ›"),
- no filter chips and no "session started" notice,
- live events are clickable rows; ended events become plain rows,
- plugin events show a plugin-provided action that is routed back to the plugin.

Final mockup: `docs/images/screenshots/activity-log-p2-twoline-dark.png` and
`-light.png`.

## 4. Recommendation

⭐ **Members-column tab, two-line no-icon rows** — chosen for implementation.

Why:

1. Reuses the column that already swaps between members / watch party / screen-share viewers.
2. One click away, chat stays visible, no new top-level surface.
3. Two-line rows stay readable in a narrow column without the noise of icons.

## 5. Decisions (all answered in chat on 2026-11-06)

- **Placement:** members-column tab.
- **Events:** connected/disconnected (all users), channel moves, screen share, watch party, plugin
  activity, plus plugin-pushed events.
- **Camera / mute / message events:** never.
- **Visibility:** whole server (no per-channel scope for now).
- **Unread indicator:** no.
- **Filters / chips:** none.
- **"Session started" notice:** none.
- **Cap:** 200 events per server.
- **Persistence:** none — in-memory only, discarded on disconnect.
- **Back-fill:** never — the sync snapshot is the baseline and produces no events except "You
  connected".
- **Order:** newest first, grouped by day.
- **Channel wording:** "moved to # ‹channel›"; "left" means disconnected.

## 6. Not part of this epic

- No server-side changes; the feed is assembled client-side from events Mumcord already receives.
- No persistence to disk.
- No search, export, or per-channel scope yet.
