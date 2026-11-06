# Mumcord · Epic 11 – Activity log (server event feed) · Specification

> Status: Draft for review · As of: 2026-11-06
> Sources: `specs/11-activity-log-brainstorming.md` (scope, the five proposals and the chosen one), the final mockup `docs/images/screenshots/activity-log-p2-twoline-dark.png` / `-light.png`, and the answers given in chat on 2026-11-06 (event scope, visibility, unread hint, cap, condensed two-line no-icon look, clickable events, plugin-pushed events).
> Builds on: `specs/01-foundation-specification.md` (stack, design system, accessibility, 24 locales), `specs/02-multi-server-themes-specification.md` (several servers at once, shown/voice server), `specs/03-163-watch-together-and-screen-share-implementation.md` (watch together), `specs/06-plugin-system-specification.md` and `specs/09-plugin-platform-1-1-specification.md` (plugin API, permissions, `ui-action`), `specs/07-file-sharing-screenshare-specification.md` (screen share), issue #165 (members column overlay). All decisions there stay valid unless this spec names a change.
> Numbering: file number 11 → requirement IDs `FR-11.x` and `AC-11.x`.

## 1. Goal & context

Today Mumcord shows "what is happening right now" in two places: the chat banners for a running
screen share or watch party, and the plugin activity lines at the top of the chat. What is missing
is a **log**: one place that lists what happened on the server — who connected or disconnected, who
moved channel, who started or stopped a screen share, watch party or plugin activity — so someone
who was away can see what they missed.

The activity log is a **per-server event feed** in the members column, behind a second tab next to
"Members". It is built client-side from data the client already holds; the only new server-visible
capability is a small plugin API that lets plugins push their own events with an action back to the
plugin. The log is **session-scoped and in memory only**: it starts when the client connects, never
back-fills anything from before the connection, and is discarded on disconnect. Nothing is ever
written to disk.

## 2. Scope

### 2.1 In scope

- A second tab in the members column: **Members | Activity**.
- The feed itself, per server, newest first, grouped by day, two-line rows with no icons.
- Events: you connected, someone connected, someone disconnected, someone moved channel, screen
  share started/stopped, watch party started/stopped, plugin activity started/stopped, and
  plugin-pushed events.
- Clickable live events: a running screen share opens the watch flow, a running watch party and a
  running plugin activity open the join flow, **only while they still exist**.
- Plugin-pushed events with a plugin-defined action that is routed back to the plugin.
- In-memory only, capped at 200 events, discarded on disconnect, never persisted.
- Dark and light theme, keyboard operation, screen-reader labels, system scaling.

### 2.2 Out of scope / later

- Camera on/off, mute/deafen or message-related events: decided "never" (chat 2026-11-06).
- Filter chips and a "session started" notice: removed by decision (chat 2026-11-06).
- A per-channel scope: later, not in this version.
- A dedicated full-width page (proposal 1) or an overlay (proposal 5): rejected.
- Search, export, persistence to disk, or a server-side history.

## 3. Roles & permissions

The log is local to the client and shows only what the client already knows. Plugin-pushed events
use a new plugin permission (see section 5); the plugin's own permission settings decide whether its
events may appear.

## 4. Functional requirements

### 4.1 Placement

- **FR-11.1** The members column gets a two-part tab strip at its top: **Members** and **Activity**.
  **Activity** shows the feed; **Members** shows the existing member list. The strip behaves like a
  `tablist`: `tab` buttons with `aria-selected` and `aria-controls`, panels with `aria-labelledby`,
  Left/Right arrows switch and Tab moves into the panel.
- **FR-11.2** The tab choice is remembered for the session but resets to **Members** for a new
  connection. The Activity tab shows no unread indicator (decided: no unread dot).
- **FR-11.3** On narrow windows the members column already becomes an overlay (issue #165); the tab
  strip appears there too and behaves the same.

### 4.2 Event collection

- **FR-11.4** The log is per server, held in memory only, and never written to disk. It is empty
  before the first sync of a connection and is discarded when the connection ends (disconnect or
  client close). Nothing survives a reconnect.
- **FR-11.5** The state delivered by `serverSynced` is the baseline and produces **no** events,
  except a single **"You connected"** entry that marks the start of the session. Events that
  happened before the client connected are therefore never collected — there is no back-fill.
- **FR-11.6** After the baseline, the following changes become events:
  - **You connected** — once, at the first sync of the connection.
  - **‹name› connected** — a session id appears that was not known before.
  - **‹name› disconnected** — a known session id disappears (kicks and bans look the same from the
    client; it is recorded as "disconnected").
  - **‹name› moved channel** — a known session's channel changes.
  - **‹name› started / stopped a screen share** — the session appears in / disappears from the
    screen-share list.
  - **‹name› started / stopped a watch party** — the party appears in / disappears from the
    watch-party state.
  - **‹name› started / stopped ‹plugin›** — a plugin activity appears in / disappears from the
    stage list.
  - **Plugin-pushed event** — a plugin pushes text and an optional action (FR-11.9).
- **FR-11.7** Each event records: the server id, the event kind, the actor's display name (or "you"
  for the own session), the channel name where applicable, the local time it was observed, and —
  for live share/party/activity events — a reference to the live thing, and — for plugin events —
  the plugin id, action label and action id. A disconnect uses the last known name of that session.
- **FR-11.8** The log keeps at most **200** events per server. When a 201st arrives, the oldest
  event is dropped.
- **FR-11.9** Plugins can push events through a new `activity` plugin API: `push(text, action?)`.
  `text` is untrusted and sanitised (control characters removed) and capped at 128 characters;
  `action` is an optional `{ label, id }` with the label capped at 24 characters. The push is
  rate-limited per plugin. The action, when clicked, is delivered back to the plugin through the
  existing `ui-action` path with the plugin's action id.

### 4.3 Feed UI

- **FR-11.10** The feed lists events **newest first**, grouped by day ("Today", "Yesterday", then a
  date). There is no filter bar and no "session started" notice.
- **FR-11.11** An event is a **two-line row with no icon**:
  - Line one: the title, with the actor's name in bold — "‹name› connected", "‹name› disconnected",
    "‹name› moved channel", "‹name› started a screen share", "‹name› started a watch party",
    "‹name› started ‹plugin›", or for a plugin event "‹plugin› “‹text›”".
  - Line two: the meta — the time, then the channel and a **live** marker where applicable
    ("14:32 · # lobby · live") — and, right-aligned, the action when there is one.
- **FR-11.12** A live screen-share, watch-party or plugin-activity event is a clickable row: the
  whole row opens the existing flow (**Watch** / **Join**), and line two carries the action label
  ("Watch ›", "Join ›"). Once the thing ends, the row becomes a plain, non-clickable row and the
  action label disappears. The click never re-opens a stopped share or party.
- **FR-11.13** A plugin event shows the plugin's action label on line two ("Notify me ›"). Clicking
  routes the action back to the plugin (FR-11.9). A plugin event without an action is a plain row.
- **FR-11.14** With no events yet, the feed shows a muted **"No activity yet."** line.
- **FR-11.15** New events are **not** announced to screen readers automatically (the feed would be
  too chatty); they simply appear in the list.

## 5. Data sources & plugin API

The store subscribes to data that already exists, plus one new plugin API:

| Source | What it provides | Mapped to |
|---|---|---|
| `subscribeAllServerChanges` (`serverStore.ts`) — `synced` | first sync of a connection | reset the log, record "You connected" |
| `subscribeAllServerChanges` — `userUpserted` (`previous === undefined`) | a new session | "‹name› connected" |
| `subscribeAllServerChanges` — `userUpserted` (channel changed) | a channel move | "‹name› moved channel" |
| `subscribeAllServerChanges` — `userRemoved` | a session gone | "‹name› disconnected" |
| `screenSharesChanged` (`sharesStore.ts`) | share list diff | "‹name› started / stopped a screen share" |
| `watchPartyChanged` (`watchPartyStore.ts`) | party appeared / disappeared | "‹name› started / stopped a watch party" |
| `pluginsStageChanged` (`activityStore.ts`) | activity list diff | "‹name› started / stopped ‹plugin›" |
| `connectionStateChanged` | connection ended | discard the log for that server |
| new plugin API `activity.push` | plugin event + action | "‹plugin› “‹text›”" + action back to plugin |

The screen-share, watch-party and plugin-stage stores currently expose only React hooks; the
activity store needs a small non-React `subscribe`/getter on each (or an equivalent change callback)
so it can observe them while the Activity tab is closed. This is a refactor of the existing stores.

The new `activity` plugin API mirrors the existing `notify` API: a `push` host function with the
permission `activity`, a length-capped and sanitised text, and a per-plugin rate limit (numbers go
into the tasks). The optional action is delivered back to the plugin as a `ui-action` so no new
callback machinery is needed.

## 6. UI

```
┌ Members column ──────────────────────────────────────────────┐
│  [ Members ]   [ Activity ]                                  │
│                                                              │
│  RaidAlert “Raid starts in 5 min”                            │
│  14:35 · plugin                                Notify me ›   │
│                                                              │
│  Nia connected                                               │
│  14:33                                                       │
│                                                              │
│  Maya started a screen share                                 │
│  14:32 · # lobby · live                          Watch ›     │
│                                                              │
│  Alex started VRChat                                         │
│  14:26 · # gaming                                 Join ›     │
│                                                              │
│  Leo disconnected                                            │
│  14:18                                                       │
│                                                              │
│  Maya moved channel                                          │
│  14:12 · # gaming                                            │
│                                                              │
│  Jordan started a watch party                                │
│  14:10 · # movie-night · live                    Join ›      │
│                                                              │
│  MusicBot “Queue is open”                                    │
│  14:06 · plugin                                 Open ›       │
│                                                              │
│  Maya connected                                              │
│  14:04                                                       │
│                                                              │
│  You connected                                               │
│  14:02                                                       │
└──────────────────────────────────────────────────────────────┘
```

- The tab strip uses the same quiet underline style as the settings navigation.
- Line one is the title with the actor's name in bold; line two is muted and smaller, with the time
  first, then the channel, then a green **live** marker for still-running shares/parties.
- The action label ("Watch ›", "Join ›", or a plugin's own label) sits right-aligned on line two in
  the accent colour; the whole row is clickable.
- Rows are separated by quiet spacing; hover highlights the clickable rows.

## 7. Accessibility

- Tab strip: `role="tablist"`, `role="tab"` with `aria-selected`/`aria-controls`, `role="tabpanel"`
  with `aria-labelledby`; Left/Right arrows switch, Tab moves into the panel.
- The feed is a list; a clickable event is a button whose accessible name combines the title and the
  action ("Watch Maya's screen share"), a plain event is a list item with the title as text.
- There are no icons, so nothing decorative needs hiding; the green **live** marker is real text.
- The feed is **not** a live region.
- Works in both themes, with system scaling and at 200 % text size without clipping; every control
  is keyboard operable.

## 8. Languages

A new `activity` i18n namespace holds every new string, in all 24 locales. Placeholders stay
untranslated (`{{name}}`, `{{channel}}`, `{{plugin}}`, `{{time}}`). Plugin event text and action
labels are plugin-provided and are shown as-is (sanitised, not translated).

## 9. Tests & acceptance criteria

Automated:

- Store: `synced` resets and records only "You connected"; users already in the snapshot produce no
  events; `userUpserted` without previous → connected; with a channel change → moved; `userRemoved`
  → disconnected; share/party/stage list diffs → started/stopped; cap 200 drops the oldest; a
  disconnect discards the log; nothing is written anywhere.
- Plugin API: `activity.push` sanitises and length-caps the text, rate-limits per plugin, drops the
  event when the plugin lacks the permission, and delivers the action back as a `ui-action` with the
  right id.
- UI: tab switching by mouse and keyboard, newest-first ordering, day grouping, empty state,
  clickable live rows vs plain ended rows, the live marker, plugin action routing.

Acceptance criteria (running client, local test server):

- **AC-11.1** Connect to a server where two people are already online: the Activity tab shows
  "You connected", and does **not** list the two already-online people as connected.
- **AC-11.2** A second client connects → "‹name› connected" appears at the top; it disconnects →
  "‹name› disconnected" appears. Nothing that happened before the first client connected ever shows
  up.
- **AC-11.3** Someone starts a screen share, a watch party and a plugin activity → three "started"
  events appear, each with a live/action row; clicking opens the same flows as the chat
  banners/activity lines. Stopping each turns its row into a plain row with no action.
- **AC-11.4** Someone moves to another channel → "‹name› moved channel" appears once, with the new
  channel in the meta line.
- **AC-11.5** Disconnect and reconnect → the feed is empty again (nothing persisted); the tab shows
  "No activity yet." until the next event.
- **AC-11.6** Generate more than 200 events (a loop of connects/moves) → the feed stays at 200 and
  the oldest entries disappear.
- **AC-11.7** A plugin pushes an event with an action → the event appears with the plugin's text and
  action label; clicking it delivers the action id back to the plugin. A plugin without the
  permission cannot push.
- **AC-11.8** Keyboard only: Tab reaches the tab strip, arrows switch Members/Activity, clickable
  rows are reachable and activated with Enter/Space. Screen reader: tab names and selected state are
  announced, and a clickable row is read as one action ("Watch Maya's screen share").
- **AC-11.9** Dark and light theme, 200 % text scaling: nothing is clipped. All new texts exist in
  all 24 locales (locale test passes); a spot-check of two languages shows no English leftovers.

## 10. Decision log

| Question | Decision | Note |
|---|---|---|
| D1 Where does the feed live? | Second tab in the members column | FR-11.1 |
| D2 Which events? | connected/disconnected (all users), channel moves, screen share, watch party, plugin activity, plugin-pushed events | FR-11.6 |
| D3 Camera / mute / message events? | Never | section 2.2 |
| D4 Server-wide or per-channel? | Whole server | section 2.2 |
| D5 Unread indicator on the tab? | No | FR-11.2 |
| D6 Filter chips? | None | FR-11.10 |
| D7 "Session started" notice? | None | FR-11.10 |
| D8 Icons? | None; two-line text rows | FR-11.11 |
| D9 Cap? | 200 events per server | FR-11.8 |
| D10 Persistence? | None; in-memory only, discarded on disconnect | FR-11.4 |
| D11 Back-fill? | Never; baseline produces no events except "You connected" | FR-11.5 |
| D12 Order? | Newest first, grouped by day | FR-11.10 |
| D13 Live event click | Whole row opens Watch/Join, only while still live | FR-11.12 |
| D14 Plugin events | New `activity` plugin API; action routes back via `ui-action` | FR-11.9, FR-11.13 |
| D15 Channel wording | "moved channel"; "left" means disconnected | FR-11.6 |

## 11. Open points & risks

- **Your own disconnect is never shown**: the log is discarded together with the connection, so
  there is no "You disconnected" entry.
- **Kicks and bans read as "disconnected"**: the client cannot always tell them apart.
- **Bursts on busy servers**: dozens of connects/moves arrive right after sync; the 200 cap keeps
  the list bounded, and events are batched per frame as the server store already does.
- **Timestamps are local observation time**, which can lag the real event by a frame or two.
- **Reconnect keeps the tree** (FR-2.8): the log is only reset on a real disconnect or on the next
  `serverSynced`.
- **Plugin event spam**: the `activity` permission plus a per-plugin rate limit keeps it bounded;
  exact limits go into the tasks.

## 12. Impact on later issues

- A per-channel scope can be added later without changing the data model.
- If the feed ever outgrows the column, proposal 1 (dedicated page) can reuse the same store.
- The new `activity` plugin API is additive; existing plugins are unaffected.
- No server-side feature is used; nothing here affects other clients or the server.
