# Mumcord · Epic 12 – Reduce SSD writes · Tasks

> Spec: `specs/12-ssd-write-reduction-specification.md`
> Each task is small, self-contained and verifiable on its own. One fresh agent per task, in order.
> Commit only on request. Build cache in `~/.cache/mumcord-build/<agent>` is deleted when done.

## T1 – Store: settings fields + SQLite `synchronous` switch

- Add `log_to_disk`, `durable_saves`, `link_previews_on_disk`, `plugin_registry_cache` to
  `store::Settings`, all defaulting to `true` (FR-12.13).
- Apply `PRAGMA synchronous = OFF` after open + migrate when `durable_saves` is off; migrations
  always run under NORMAL (FR-12.8, FR-12.9).
- Tests: round-trip of the four fields, default-on for old records, and the pragma change.

## T2 – Engine: buffered log writing + `log_to_disk` switch

- Replace the per-line `write_all` in `engine::diag` with a buffered writer flushed every 5 s, on
  clean shutdown and on panic (variant A, FR-12.1). The panic hook already reads the memory ring.
- Skip opening/writing the log file entirely when `log_to_disk` is off; keep the in-memory ring
  and `copy log` / diagnostic file working (FR-12.2, FR-12.4).
- Tests: lines land in batches; nothing is written when off; panic flush keeps lines.

## T3 – Engine: chat-history batching

- Collect new messages in memory and flush to SQLite in batches (5 s / idle / shutdown) instead
  of one transaction per message, always on (FR-12.5, FR-12.7).
- Respect the per-server "Save history" switch unchanged (FR-12.6).
- Tests: batching flushes on timer and shutdown, respects the switch, loses nothing on a clean
  close.

## T4 – Engine: crash-context debounce

- Debounce the `context` file writes in `engine::crash` to once per second; keep the in-memory
  context immediate for the panic hook (FR-12.12).
- Tests: rapid `set_context` calls produce at most one file write per second, memory stays exact.

## T5 – Engine: link-preview and plugin-registry disk-off wiring

- Link previews: when `link_previews_on_disk` is off, skip the disk `put` and keep previews in
  memory only (FR-12.10).
- Plugin registry: when `plugin_registry_cache` is off, skip writing the offline copy and always
  fetch fresh (FR-12.11).
- Tests: both switches skip their writes; on/off behave as documented.

## T6 – UI: Diagnostics card + "Reduce disk writes" card

- Diagnostics card: replace the single dropdown with the "Write logs to disk" switch plus the
  existing "Log detail" dropdown (proposal C, FR-12.2, FR-12.3).
- New "Reduce disk writes" card in Advanced with the three switches and their hints
  (FR-12.8, FR-12.10, FR-12.11).
- Wire the four settings through the settings screen (`SettingsSectionProps`), search index and
  change tracking.
- Component tests: switch + dropdown render; the three switches update their settings.

## T7 – Bindings + 24 locales

- Regenerate the Tauri/Specta bindings for the four new settings fields.
- New `settings` strings under `diskWrites` (card title, three switch labels + hints) and the
  `diagnostics` strings ("Write logs to disk" + hint) in all 24 locales.
- Locale parity test passes; spot-check two languages.

## T8 – Verification

- `cargo test`, `clippy`, `npm run typecheck`, `npm run lint`, `npm run test`.
- Live walk through AC-12.1 – AC-12.8 in the running client against the local test server
  (a second client for chat, a plugin for the registry-cache check).
