# Mumcord · Epic 3 – Redesigned add/edit server dialog · Tasks

> Spec: `specs/03-server-dialog-specification.md` · Issue: #151
> Each task is small, self-contained and verifiable on its own.
>
> Status 2026-10-30: T1–T8 done except the manual walk through AC-3.20 – AC-3.29 in the running client (needs the built app) and a WDIO end-to-end spec. Live check of the probe against `testenv` v15 passes (`crates/engine/tests/live_server_test.rs`).

## T1 – Protocol: a real ping

- `Handle::ping() -> Option<Duration>`: queue a Mumble `Ping`, wait for the server's answer, return the round trip; `None` when the connection ended first.
- Test against the in-repo fake server; test that a dropped connection ends the wait.

## T2 – Engine: `server_test_connection`

- Input `ServerTestInput { address, port, username, password: PasswordUpdate, server_id, identity_id }`.
- Resolve the password (typed, saved, or none) and the identity (own choice or default), connect with `connect_with_tap`, wait for the sync, take the server version, ping once, close the connection.
- Certificate state from the saved trust decision of `server_id`: trusted / unknown / changed; the fingerprint is reported; nothing is trusted or stored.
- No slot, no events, no voice: the probe never becomes a server connection.
- Errors mapped to the existing codes plus `timeout`; 15 s limit.
- Tests against the local test server (version, ping, wrong password, unreachable port, changed certificate).

## T3 – Tauri command, bindings, docs

- `server_test_connection` wired up, bindings regenerated, `docs/engine-api.md` updated, mock engine support (success and the error cases).

## T4 – UI: tabs in the server dialog

- Tab list (`tablist`/`tab`/`tabpanel`) with arrow keys, Home/End, `aria-selected`/`aria-controls`, warning dots per tab.
- Four panels: Connection, Identity (existing `IdentitySelect`), Passwords, Behaviour.
- New server opens on Connection, editing remembers the last tab of the session.
- Tests: mouse and keyboard switching, warning dots, panel content per mode (new vs edit).

## T5 – UI: inline validation

- Per-field rules and messages (FR-3.36), validation on blur and submit, `role="alert"`, `aria-describedby`.
- Submit switches to the first tab with an invalid field and focuses it.
- mumble:// link fill keeps working and switches to Connection.
- Tests for every rule and for the cross-tab focus jump.

## T6 – UI: test connection

- Button with busy state, result line with ping, server version and certificate state (tones + icon), `role="status"`.
- Uses the form values; disabled while running; never saves.
- Tests: success, wrong password, unreachable, changed certificate, busy state.

## T7 – Languages

- All new texts in the 24 locales; locale parity test passes; spot check two languages.

## T8 – Verification

- `cargo test`, `clippy`, `npm run typecheck`, `npm run lint`, `npm run test`.
- Live check against `testenv` v15 for AC-3.22 – AC-3.24; manual walk through AC-3.20 – AC-3.29 in the running client.
