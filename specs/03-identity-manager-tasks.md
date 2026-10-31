# Mumcord · Epic 3 – Identity manager · Tasks

> Spec: `specs/03-identity-manager-specification.md` · Issue: #148
> Each task is small, self-contained and verifiable on its own. Order matters; later tasks build on earlier ones.
>
> Status 2026-10-30: T1–T11 done except the parts marked open below.
> - Live check done against `testenv` v15 + v15b: `crates/engine/tests/live_identity.rs` (two servers with different identities, switching one server's identity, identity list bookkeeping) passes with `MUMCORD_LIVE=1`.
> - **Open:** a Playwright/WDIO end-to-end spec for the identity manager (`e2e/specs/servers/identities.e2e.ts`) and the manual walk through AC-3.1 – AC-3.14 in the running client (needs the built app; not done in this worktree).

## T1 – Store: identity table and per-server choice

- Migration v13: `identity` table, unique default index, `saved_server.identity_id` (section 5.1).
- `IdentityRecord` (id, label, cert name, hash, created_at, is_default) and store methods: `add_identity`, `identities`, `identity`, `rename_identity`, `set_default_identity`, `delete_identity`, `identity_servers`, `set_server_identity`.
- Rules: exactly one default, first identity becomes default, delete promotes the oldest remaining, last identity cannot be deleted, duplicate hash refused, label validation.
- New errors: `LastIdentity`, `UnknownIdentity`, `DuplicateIdentity`, `EmptyIdentityLabel`.
- Tests for every rule, including "deleting an identity clears its servers to NULL".

## T2 – Secrets: one keychain item group per identity

- Layout `identity/<id>` (chunk count) and `identity/<id>/<n>` (chunks); per-id cache.
- `save_identity(id, &Identity)`, `load_identity(id)`, `delete_identity(id)`, `identity_ids()`.
- Legacy layout (`identity`, `identity/<n>`) read once, returned for migration, deleted afterwards.
- Tests: round trip per id, two identities side by side, legacy read, failed migration keeps legacy items.

## T3 – Store: bootstrap the identity registry from the old keychain item

- On first use after the upgrade: read the legacy identity, insert it as "Default" (label "Default"), write it under the new layout, delete the legacy items.
- Idempotent and safe to fail (old layout survives a failure; the app still connects).
- Tests: fresh store without legacy identity, store with legacy identity, migration runs once, failure path.

## T4 – Engine: identity commands

- `identity_list`, `identity_create(name, label?)`, `identity_import(path, password, label?)` (adds, refuses duplicates), `identity_rename`, `identity_export(id, …)`, `identity_delete`, `identity_set_default`.
- `identity_get` keeps returning the default identity.
- Per-id identity cache in the engine; `IdentityView` extended (id, label, cert name, createdAt, isDefault, serverIds).
- Tests at engine level with the in-memory secret store.

## T5 – Engine: connect with the server's identity

- `SavedServer`/`ServerView` carry `identityId`; `server_set_identity(serverId, identityId | null)`.
- Connect resolves the server's identity, falls back to the default and emits a notice when it is missing.
- Peer-to-peer always uses the default identity.
- Tests: two servers with different identities; fallback on a missing identity.

## T6 – Tauri commands, bindings, engine API docs

- New commands wired in `src-tauri`, `export_bindings` regenerated, `docs/engine-api.md` updated.
- `ui/src/api/mock.ts` supports the new commands (and `identity_get` for the default).

## T7 – UI: Settings → Identity (identity manager)

- List with avatar, label, default badge, "Name · Created", "Fingerprint · Used by", actions.
- Dialogs: new identity, import, rename, export, details, delete (typed confirmation, "Back up first", last-identity hint).
- Keyboard, focus return, `role="alert"` validation, empty state.
- Tests for the list, every dialog and the delete rules.

## T8 – UI: identity per server

- Shared themed dropdown component (listbox with keyboard support).
- `ServerIdentityDialog` from the server context menu ("Identity…"), with "Manage identities…" and "Save and reconnect"/"Save".
- Same dropdown in the current edit-server dialog.
- Confirmation before reconnecting a connected server; "Later" stores without reconnecting.
- Tests: store call, confirmation, focus return, keyboard.

## T9 – UI: identity in use visible

- Server header badge for non-default identities (channel tree header and chat header).
- Connection info panel row "Identity".
- User panel fallback name and "Register on this server" export use the shown server's identity.
- Tests for all three.

## T10 – Languages

- All new texts in the 24 locales (`settings.json`, `servers.json`), dates and fingerprints locale-formatted.
- Test that every locale has the same keys.

## T11 – Verification

- `cargo test -p mumcord-store`, `cargo clippy`, `npm run typecheck`, `npm run lint`, `npm run test`.
- Live test against `testenv` (`v15` + `v15b`): two servers, two identities (AC-3.9), identity switch reconnects one server (AC-3.10).
- Manual walk through AC-3.1 – AC-3.14 and note the result in the PR.
