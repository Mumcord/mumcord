# Mumcord · Epic 12 – Reduce SSD writes · Specification

> Status: Draft for review · As of: 2026-11-08
> Sources: `docs/ssd-write-reduction.md` (inventory of every disk writer, questions, options and pros/cons), the screenshot `docs/images/screenshots/ssd-logloglevel-proposal-c.png`, and the answers given in chat on 2026-11-08 (several independent switches; chat-history batching always on; log buffering variant A; log UI proposal C; `synchronous=OFF` behind a switch; webview cache out of scope).
> Builds on: `specs/01-foundation-specification.md` (stack, design system, accessibility, 24 locales), `specs/02-multi-server-themes-specification.md` (several servers, settings screen), `specs/03-comfort-polish-specification.md` (settings conventions), `specs/06-plugin-system-specification.md` (plugin registry cache). All decisions there stay valid unless this spec names a change.
> Numbering: file number 12 → requirement IDs `FR-12.x` and `AC-12.x`.

## 1. Goal & context

Solid-state drives wear down from write cycles. Mumcord writes to disk in several places:
a log file per log line, the SQLite database per message and per settings change, link-preview
and plugin caches, and the crash reporter. Most of these writes are not wrong, but several happen
far more often than needed. This epic reduces the number and frequency of writes, without
changing what the app does for the user, and exposes the optional trade-offs as a few small
settings so everyone can pick their own balance between durability and wear.

The work is deliberately small and conservative: it changes *when* and *how often* data is
written, never *what* is kept. All settings keep today's behaviour as their default.

## 2. Scope

### 2.1 In scope

- Buffered log writing (variant A): lines collect in memory and are written in batches.
- A "Write logs to disk" switch plus the existing log-detail dropdown (proposal C).
- Chat-history batching: messages are written in batches instead of one transaction per message.
- A "Wait for disk on every save" switch (`synchronous` NORMAL vs OFF).
- A "Keep link previews on disk" switch.
- A "Keep plugin registry copy on disk" switch.
- Crash-context debounce.
- Settings UI for all of the above, dark and light, keyboard and screen-reader friendly, all 24
  locales.

### 2.2 Out of scope / later

- The web engine cache (WebKitGTK/WebView2/WKWebView): out of scope, decided 2026-11-08. The
  largest-volume writer, but platform-specific and invasive — its own epic if it returns.
- A single "master" switch: rejected; each reduction is its own switch.
- Relocating caches/logs to a RAM disk: not built in this epic (documented in the plan doc as a
  possible manual tip, not a feature).
- Throttling the friend "last seen" writes and other marginal writers: later, only if needed.
- File transfers and identity export: intentional user file writes, untouched.

## 3. Roles & permissions

None. All settings are per user, local, and available to everyone. No server or plugin changes.

## 4. Functional requirements

### 4.1 Log writing (always buffered + disk switch)

- **FR-12.1** Log lines are collected in memory and written to the log file in **batches**, not
  line by line: the buffer is flushed every 5 seconds, on clean shutdown, and on a Rust panic.
  A hard crash (segfault) cannot flush safely and may lose the last few seconds; the crash report
  is still generated on the next start from the last flushed file.
- **FR-12.2** New setting **"Write logs to disk"** (default: on). On = buffered writes per
  FR-12.1. Off = no log file is created or written at all; the in-memory log still records so
  "Copy whole log", the diagnostic file and crash reports keep working.
- **FR-12.3** The existing **"Log detail"** dropdown keeps its four levels (Errors only,
  Warnings, Normal, Detailed) and filters what is recorded, both in memory and on disk. It no
  longer needs an "Off" entry — the switch carries that meaning.
- **FR-12.4** The memory log keeps its current cap (last 500 lines) and redaction rules. Nothing
  personal, no chat contents or audio is logged (unchanged).

### 4.2 Chat-history batching (always on)

- **FR-12.5** New chat messages are held in memory and written to the database in **batches**
  instead of one transaction per message: flushed every second, on idle, and at shutdown.
  Always on; no setting.
- **FR-12.6** The per-server "Save history" switch and history sync stay exactly as they are.
  Batching changes only *when* the same messages reach the database, not which messages are kept.
- **FR-12.7** The last batch is flushed on clean shutdown so a normal close never loses messages.

### 4.3 SQLite durability switch

- **FR-12.8** New setting **"Wait for disk on every save"** (default: on). On = `synchronous =
  NORMAL` (today). Off = `synchronous = OFF`: commits are not fsynced, which removes a forced
  write per transaction; a power cut can lose the last few seconds of history and settings, but
  the database cannot be corrupted (WAL mode stays on).
- **FR-12.9** The pragma is applied after opening and migrating the database, before the engine
  serves any request, from the stored setting. Migrations always run under NORMAL so a schema
  change is never written with OFF.

### 4.4 Link-preview disk cache switch

- **FR-12.10** New setting **"Keep link previews on disk"** (default: on). On = today's behaviour.
  Off = previews are kept in memory only for the session; nothing is written to the
  `link-previews/` folder and existing files are not touched. After a restart the previews are
  re-fetched.

### 4.5 Plugin registry cache switch

- **FR-12.11** New setting **"Keep plugin registry copy on disk"** (default: on). On = today's
  behaviour (body + signature + ETag offline copy). Off = the registry is always fetched fresh
  and no offline copy is written; a registry refresh therefore always needs the network.

### 4.6 Crash-context debounce (always on)

- **FR-12.12** The crash `context` file is written at most **once per second** (debounced, always
  on, no setting). The in-memory context stays immediate for the panic hook, so a Rust panic still
  sees the exact last state.

### 4.7 Settings

- **FR-12.13** Four new settings, all defaulting to on so existing profiles behave exactly as
  today: `logToDisk`, `durableSaves`, `linkPreviewsOnDisk`, `pluginRegistryCache`.
- **FR-12.14** The new settings are stored in the existing single settings JSON record with serde
  defaults. There is no database schema migration; old records load with the defaults.

## 5. Data model & code map

| Setting (TS / Rust) | Default | Effect |
|---|---|---|
| `logToDisk` / `log_to_disk: bool` | `true` | FR-12.2; `diag::attach_file` skipped when off |
| `durableSaves` / `durable_saves: bool` | `true` | FR-12.8; `PRAGMA synchronous` NORMAL vs OFF in `store::db::from_connection` |
| `linkPreviewsOnDisk` / `link_previews_on_disk: bool` | `true` | FR-12.10; skips the disk `put` in `engine::web` preview cache |
| `pluginRegistryCache` / `plugin_registry_cache: bool` | `true` | FR-12.11; skips `registry::cache::store` |

Always-on changes (no settings):

- `engine::diag` — replace the per-line `write_all` with a buffered writer and a 5 s flush timer;
  flush on shutdown and on panic (the panic hook already reads the memory ring).
- `store::history` / `engine::session` — collect messages and flush in batches (1 s / idle /
  shutdown).
- `engine::crash::persist_context` — debounce file writes to once per second.

## 6. UI

### 6.1 Diagnostics card (proposal C)

```
┌ Diagnostics ──────────────────────────────────────────────┐
│  Write logs to disk                                ( on ) │
│  Log detail                                  [ Normal ▾ ] │
│                                                           │
│  Current log                                              │
│  Logs contain no chat messages, names, passwords or audio.│
│  [ Copy last 200 lines ] [ Copy whole log ] [ Open folder ]│
│  Diagnostic file                                          │
│  [ Save diagnostic file… ]                                │
└───────────────────────────────────────────────────────────┘
```

- The switch uses the existing `Switch` component; the dropdown uses the existing `Select`
  component. The hint under the switch says: "Off keeps the log in memory for this session only —
  nothing is written to disk."

### 6.2 New "Reduce disk writes" card (Advanced)

```
┌ Reduce disk writes ───────────────────────────────────────┐
│  Wait for disk on every save                        ( on )│
│    Off writes less, but a power cut can lose the last few │
│    seconds of history and settings.                       │
│  Keep link previews on disk                        ( on ) │
│    Off keeps previews in memory for this session only.    │
│  Keep plugin registry copy on disk                 ( on ) │
│    Off always fetches the registry fresh.                 │
└───────────────────────────────────────────────────────────┘
```

- The card sits in the **Advanced** section near the existing "Storage" card, uses the same
  `Group` + `Switch` components, and each switch shows its explanation as its status/hint line.

## 7. Accessibility

- All new controls are the existing `Switch` and `Select` components, so they already expose
  proper roles (`role="switch"`, labelled combobox), keyboard operation and screen-reader names.
- Each switch's hint text is associated with the control and announced with it.
- The four settings appear in the settings search index with their titles and hint terms.
- Both themes, system scaling and 200 % text size must not clip the card content.

## 8. Languages

New strings live in the existing `settings` namespace under a new `diskWrites` key, in all 24
locales, plus the two new `diagnostics` strings ("Write logs to disk" and its hint). Placeholders
stay untranslated; there are none beyond those already used.

## 9. Tests & acceptance criteria

Automated:

- Store: `durable_saves=false` sets `PRAGMA synchronous` to 0 after open and leaves the schema
  migrated; `link_previews_on_disk=false` never calls the preview disk `put`;
  `plugin_registry_cache=false` never writes the offline copy; all four settings round-trip and
  default to true for old records.
- Engine: log lines are buffered and flushed on the timer, on shutdown and on panic; with
  `log_to_disk=false` no file is created. Chat-history batching flushes messages on the timer and
  at shutdown, respects the per-server "Save history" switch, and loses nothing on a clean close.
- Crash context: `set_context` debounces file writes to once per second while the in-memory value
  updates immediately.
- UI: the Diagnostics card renders the switch + dropdown; the new card renders the three switches;
  search finds them; keyboard and screen-reader tests pass.

Acceptance criteria (running client, local test server):

- **AC-12.1** Watch the log file while the app is used: new lines appear in bursts (every few
  seconds), not continuously per event. Turn "Write logs to disk" off → the file stops growing and
  "Copy whole log" still returns the in-memory log.
- **AC-12.2** Trigger a panic in a development build → the crash report contains the buffered
  lines up to the panic (nothing lost).
- **AC-12.3** Send many chat messages in quick succession, then close normally → on the next start
  all messages are present; the database was written in batches, not per message.
- **AC-12.4** Turn "Wait for disk on every save" off, change a setting, close and reopen → the
  change is kept. (The power-cut trade-off is documented, not easily testable.)
- **AC-12.5** Turn "Keep link previews on disk" off → a preview still renders in the session, the
  `link-previews` folder is not written, and after a restart the preview is re-fetched.
- **AC-12.6** Turn "Keep plugin registry copy on disk" off → a registry refresh works and no
  offline copy appears in the registry cache folder.
- **AC-12.7** Start/stop the camera and a screen share in quick succession → the crash context file
  is written at most once per second.
- **AC-12.8** Dark and light theme, 200 % scaling, keyboard-only: all four settings are reachable
  and changeable; the locale test passes and a spot-check of two languages shows no English
  leftovers.

## 10. Decision log

| Question | Decision | Note |
|---|---|---|
| D1 One master switch or several? | Several independent switches | FR-12.13; no master switch |
| D2 Chat-history batching optional? | Always on, no setting | FR-12.5 |
| D3 How to buffer logs? | Variant A: timer + shutdown + panic; segfault may lose last seconds | FR-12.1 |
| D4 Log "off" UI | Proposal C: on/off switch + level dropdown | FR-12.2, FR-12.3 |
| D5 `synchronous=OFF` allowed? | Yes, behind "Wait for disk on every save" | FR-12.8 |
| D6 Webview cache | Out of scope | section 2.2 |
| D7 Link previews / registry cache | Own switches, default on | FR-12.10, FR-12.11 |
| D8 Crash-context debounce | Always on, once per second | FR-12.12 |

## 11. Open points & risks

- **Flush interval (1 s)** is a starting point; the tasks may tune it after measuring.
- **Hard crash loses the last batch of chat history** even with "Wait for disk" on, because
  batching holds the newest messages in memory. Accepted by decision (D2); a normal close never
  loses anything.
- **`durableSaves` off** means a power cut can lose the last few seconds of history/settings —
  by design; the WAL keeps the file itself uncorrupted.
- **Migrations run under NORMAL** regardless of the switch, so the setting cannot put a schema
  change at risk.
- **Log buffering changes the diagnostic-file timing slightly**: the file may be up to 5 s behind
  the memory ring. The panic hook and "Copy whole log" read the memory ring, so they are exact.

## 12. Impact on later issues

- If the webview-cache reduction returns, it becomes another switch in the same "Reduce disk
  writes" card.
- The friend "last seen" throttling and any other marginal writer can be added later without
  changing this spec's model.
- No server-side or plugin changes; nothing here affects other clients or the server.
