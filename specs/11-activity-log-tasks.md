# Mumcord · Epic 11 – Activity log · Tasks

> Spec: `specs/11-activity-log-specification.md`
> Each task is small, self-contained and verifiable on its own. One fresh agent per task, in order.
> Commit only on request. Build cache in `~/.cache/mumcord-build/<agent>` is deleted when done.

## T1 – Plugin API: `activity.push`

- New plugin permission `activity` and host function `push(text: string, action?: { label: string, id: string })`.
- Sanitise `text` (control characters become spaces) and cap lengths: text ≤ 128 chars, action label ≤ 24 chars.
- Per-plugin rate limit (at most 1 per 10 s and 50 per hour; final numbers in code + tests).
- A push emits `HostEvent::Activity { server_id, plugin_id, text, action }` so the UI can render it; the action is later routed back to the plugin through the existing `ui-action` path (no new callback).
- Plugin without the permission gets an error; too-large or rate-limited pushes error cleanly.
- Tests in the plugins crate (sanitise, caps, rate limit, permission, emitted event).

## T2 – Engine → UI event and bindings

- New engine event `activityEvent` (`server_id`, `plugin_id`, `text`, `action_label`, `action_id`) delivered to the UI when a plugin pushes.
- Bindings regenerated; mock engine support for the event.
- Tests: the event reaches the UI listener; mock drives it.

## T3 – UI store refactor: non-React subscriptions

- Export a `subscribe` (and getter) from `sharesStore.ts`, `watchPartyStore.ts` and `activityStore.ts` (or an equivalent change callback) so the activity store can observe them while the Activity tab is closed.
- No behaviour change; the existing hooks keep working.
- Tests: existing store tests still pass; the new subscribe fires on change.

## T4 – UI: activity store

- New `ui/src/features/activity/activityStore.ts`: per-server event history, in-memory only, cap 200, reset on `synced` and discarded on disconnect.
- Baseline rule (FR-11.5): `synced` records only "You connected"; users already in the snapshot produce no events.
- Derive events (FR-11.6): connected / disconnected / moved channel from `subscribeAllServerChanges`; screen-share, watch-party and plugin-activity start/stop from the store diffs; plugin events from `activityEvent`.
- Record the fields of FR-11.7; expose `useActivityLog(serverId)`.
- Tests: baseline no-backfill, each derivation, cap drops oldest, discard on disconnect.

## T5 – UI: members column tabs + feed

- Tab strip **Members | Activity** (`tablist`/`tab`/`tabpanel`, arrow keys) in the members column (FR-11.1–11.3); tab resets to Members on a new connection, no unread dot.
- Feed component: newest first, day groups, two-line no-icon rows (FR-11.11), green **live** marker, clickable live rows (Watch/Join) vs plain ended rows (FR-11.12), plugin action routing via `pluginUiAction` (FR-11.13), empty state "No activity yet." (FR-11.14).
- Tests: tab switching, ordering, day grouping, clickable vs plain, plugin action, empty state.

## T6 – Languages & permission mirror

- New `activity` namespace with every string in all 24 locales; locale parity test passes; spot-check two languages.
- Permission mirror for `activity` (deferred from T1): entry in `ui/src/features/plugins/permissions/permissionIcons.ts`, the count in `permissions.test.tsx`, and the name + explanation in `pluginPermissions.json` for all 24 locales (risk tier low, same as `notify`).

## T7 – Verification

- `cargo test`, `clippy`, `npm run typecheck`, `npm run lint`, `npm run test`.
- Live walk through AC-11.1 – AC-11.9 in the running client against the local test server (a second client for connect/disconnect, a plugin for the plugin event).
