# SSD write reduction — inventory, questions and options

Goal: make Mumcord cause as little SSD wear as possible, optionally behind a
user-facing setting ("reduce write cycles"). This document lists every place the
app writes to disk, questions each one, and maps possible reductions with pros
and cons. Nothing here is a decision yet — it is the basis for one.

SSD wear only cares about **how often** we write and **how large** the writes
are. Reads, CPU and RAM cost nothing for wear. One large write costs far less
than many small writes because the flash controller amplifies small random
writes.

---

## 1. Inventory: every place Mumcord writes to disk

| # | What | Where | Frequency | Size | Trigger |
|---|------|-------|-----------|------|---------|
| 1 | Log files | `<logs>/mumcord-*.log` (daily rotation, 7 kept) | **per log line, direct unbuffered write** | small, constant | every tracing event at Info level (connects, reconnects, device lists, plugins, state changes) |
| 2 | SQLite database | `<data>/mumcord.sqlite3` + `-wal` + `-shm` (WAL, synchronous NORMAL) | one transaction per change | small–medium | see table 2 below |
| 3 | Crash marker | `<logs>/crash/marker` | once per start, removed on clean exit | tiny | start/exit |
| 4 | Crash context | `<logs>/crash/context` | rewritten on every `set_context` call | tiny | camera on/off, screen share start/stop, watch mode, monitor list |
| 5 | Crash maps + reports | `<logs>/crash/*` | only on crash or "save diagnostic" | medium | crash / user action |
| 6 | Web engine cache | WebKitGTK / WebView2 / WKWebView cache + `localStorage` + `IndexedDB` | continuous while UI runs | large | UI use, HTTP cache, UI prefs |
| 7 | Link preview cache | `<data>/link-previews/*.json` (SHA-1 names) | one file per new preview, plus prune scans | small | first render of a link preview / inline image |
| 8 | Compiled plugin cache | `<cache>/plugin-modules/plugins/*.cwasm` | once per plugin version | medium | plugin install/upgrade |
| 9 | Plugin packages | `<data>/plugins/**` | on install | medium | plugin install |
| 10 | Plugin registry cache | `<cache>/plugin-registry/*` (+ signature + etag) | each registry fetch | small | registry refresh |
| 11 | Plugin peer key | `<data>/plugins/**/peer-key` | once per new peer | tiny | first P2P contact with a peer |
| 12 | Camera backgrounds | `<data>/backgrounds/*.jpg` | user adds image | medium | user action |
| 13 | Incoming file transfers | downloads dir (BufWriter, temp + rename) | per transfer | large | receiving a file |
| 14 | Outgoing file transfers | temp zip + source files | per transfer | large | sending a file |
| 15 | Identity export | user-chosen path (PKCS#12) | user action | medium | export |
| 16 | Diagnostic file | user-chosen path | user action | medium | "save diagnostic info" |
| 17 | Overlay IPC | Unix socket / named pipe | connection lifetime | tiny | overlay connect (usually tmpfs/pipe, negligible) |
| 18 | OS/toolkit caches | app cache dir (gstreamer registry, GLib, etc.) | occasional | small–medium | framework internals |

### Table 2 — what writes into the SQLite database (item 2)

| Trigger | How often | Question |
|---------|-----------|----------|
| Chat history append (one INSERT per message) | **per message** in active chat | needed? is every message worth a transaction? |
| Settings save (full JSON blob rewrite) | per settings change | needed at change time, or defer? |
| Own voice (mute/deafen) | per toggle | could be memory + flush later |
| Friend "last seen" | per friend presence transition | could be throttled/batched |
| Servers, identities, certificates, trust, favourites, follows, person settings, channel audio, channel passwords, ACLs, ban lists, plugin trust | user action | all needed, all infrequent |
| WAL checkpoints | background, periodic | tunable |

---

## 2. Questions asked about each writer

1. **Logs** — biggest continuous writer. Info level logs lots of events
   (reconnect attempts, device enumeration, plugin activity). No buffering:
   every line is one `write` syscall. *Question: do we need Info on disk by
   default? Can we buffer, or offer "off"?*
2. **SQLite** — correctly already WAL + `synchronous NORMAL` (good). But chat
   history is one transaction per message and settings rewrite the whole blob.
   *Question: can we batch or defer these two?*
3. **Crash marker** — negligible, keep.
4. **Crash context** — rewrites a file on each camera/screen-share state
   change. *Question: debounce, or write only what the native handler needs?*
5. **Crash reports** — crash-only, keep.
6. **Web engine cache** — biggest *size* writer, fully outside our code (the
   platform webview). *Question: can we point it at RAM or disable the HTTP
   cache?*
7. **Link previews** — already in-memory + disk. *Question: memory-only mode?*
8–11. **Plugins** — install-time only, mostly fine. Registry cache could be
   optional.
12–16. **User-initiated files** — intentional, keep.
17. **Overlay IPC** — tmpfs/pipe, ignore.
18. **OS caches** — mostly fixed; cache dir can be relocated.

---

## 3. Measures and options (pros / cons)

Effort: S = small, M = medium, L = large.

### 3.1 Log buffering + "Off" log level (small — DECIDED: variant A)

Today `diag.rs` writes every formatted line straight to the rolling file
(`RollingFileAppender` = `RwLock<File>`, no buffer).

**Decided approach (variant A):** keep log lines in memory and write them in
batches — flush every few seconds, on clean shutdown and on a Rust panic. A
hard crash (segfault) cannot safely flush the buffer inside the signal handler,
so it may lose the last few seconds; the crash report is still generated on the
next start from the file that was last flushed. Rust panics already read the
in-memory ring, so they lose nothing.

- Pro: hundreds of small writes become one write every few seconds; zero loss
  on panic; no feature loss.
- Con: last few seconds missing after a segfault (accepted).
- **Add an "Off" level** (Error/Warn/Info/Debug + Off): Off = memory ring only,
  no file writes at all.
  - Pro: zero log wear for users who want it.
  - Con: less diagnostics; user must re-enable to file a report.

### 3.2 Several independent switches (DECIDED: no master switch)

No single master switch. Each reduction gets its own setting. The user decides
item by item. Shape in section 5.

### 3.3 Chat history: batch/defer writes (medium — DECIDED: always on)

Keep the newest messages in memory, flush to SQLite on a timer (e.g. every 5 s
or on idle) or at shutdown, instead of one transaction per message. Always on,
not tied to any SSD setting.

- Pro: during active chat, write transactions drop from N per message to a few
  per flush; biggest SQLite writer tamed.
- Con: crash can lose a few seconds of history; ordering/flush logic is real
  complexity; needs care with the "save history" toggle and history sync.

### 3.4 SQLite pragma tuning (small)

- `synchronous = OFF` (already WAL + NORMAL). WAL mode is corruption-safe
  without fsync per commit; OFF only risks losing recent commits on power loss.
  - Pro: removes per-commit fsync, real wear reduction, one line.
  - Con: last seconds of settings/history may vanish on hard power loss.
- Raise `wal_autocheckpoint` / checkpoint less often: fewer big rewrites of the
  main file, but a larger `-wal` file that is still rewritten as pages move.
  - Pro: fewer main-file rewrites.
  - Con: `-wal` grows, slightly higher disk usage and slower reopen; marginal.

Recommend: `synchronous=OFF` behind its own switch; skip the checkpoint knobs
(marginal, risk of confusion).

### 3.5 Link previews memory-only (small)

Preview JSON files are already backed by an in-memory cache with limits. Add a
flag to skip the disk `put`.

- Pro: zero preview writes; previews still work during the session.
- Con: after restart, previews re-fetch (small network cost, brief placeholder).

### 3.6 Crash context debounce (small)

`set_context` currently rewrites `context` on every call. Debounce to once per
second (or write only on the transitions that matter).

- Pro: fewer tiny writes; still keeps the crash handler's info fresh enough.
- Con: at most 1 s stale context in a crash report (irrelevant).

### 3.7 Web engine cache off / to RAM (medium–large, platform-specific — DECIDED: out)

Not in scope for now. Kept here for reference if it comes back.

The webview (WebKitGTK/WebView2/WKWebView) writes HTTP cache, `CacheStorage`,
`localStorage` and `IndexedDB`. This is the largest *volume* writer and is not
our code.

- Disable the HTTP cache / use a private (incognito) webview session.
  - Pro: kills most webview writes.
  - Con: UI settings in `localStorage`/`IndexedDB` may be lost or must be moved
    into our SQLite settings; slower re-render of cached assets; each platform
    needs its own implementation (WebKitGTK network session, WebView2 profile,
    WKWebView website data store).
- Point the webview data directory at a RAM-backed path (tmpfs on Linux,
  `XDG_RUNTIME_DIR`; not universally available on Windows/macOS).
  - Pro: all webview writes go to RAM.
  - Con: lost on reboot; RAM usage; portable/Windows needs a fallback.

This is the most impactful but also the most invasive — likely its own epic.

### 3.8 Plugin registry cache optional (small)

Skip writing the offline copy of the registry (body + signature + etag).

- Pro: no registry writes, no stale-file handling.
- Con: every refresh needs the network; slower startup if offline.

### 3.9 Friend "last seen" throttling (small)

Batch presence-driven `friend_seen` updates with a short debounce.

- Pro: fewer tiny DB writes during busy servers.
- Con: marginal wear gain; added timer logic.

### 3.10 Relocate cache/log dirs to RAM (no code, or a config hint) (small)

Document/allow pointing the cache dir (and optionally logs) at a tmpfs/RAM
disk. On Linux this is trivial (`/dev/shm`, `XDG_RUNTIME_DIR`).

- Pro: zero wear for those dirs, no code risk.
- Con: lost on reboot; RAM usage; only a real option on Linux.

---

## 4. Recommended sequencing

1. **Do first (cheap, high value):** 3.1 (buffered logging, variant A, plus the
   "Off" level), 3.4 (`synchronous=OFF` switch), 3.5 (previews memory-only),
   3.6 (context debounce).
2. **Do second (more code):** 3.3 (history batching, always on) — biggest
   SQLite win.
3. **Out of scope for now:** 3.7 (webview cache).
4. **Optional niceties:** 3.8, 3.9, 3.10.

## 5. Settings shape (decided: several independent switches)

No master switch. Each reduction is its own setting, all off by default so the
current behaviour stays the default.

- **Log detail** — proposal C (DECIDED): an on/off switch "Write logs to disk"
  plus a dropdown for the level (Errors only / Warnings / Normal / Detailed).
  Buffered writes always on (variant A); switch off = memory only, no disk
  writes.
- **Chat history** — batched writes, always on (not a setting).
- **Link previews** — switch "keep on disk" (off = memory only).
- **Plugin registry cache** — switch (off = no offline copy).
- **SQLite** — switch "wait for disk on every save" (off = `synchronous=OFF`).
- **Crash context** — debounced, always on (not a setting).
- **Webview cache** — out of scope for now.

No open questions left: all decisions recorded above.
