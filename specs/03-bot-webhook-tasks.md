# Epic 3 – Bot: webhook messages into a channel · Task list

> Mode: single session (`/dev 3 --no-team`). Owner column names who ran the task.
> Spec: `specs/03-bot-webhook-specification.md` · Report: `specs/03-bot-webhook-implementation.md`

| ID | Title | Status | Owner | Depends on | Complexity | Files / covered AC |
|---|---|---|---|---|---|---|
| A1 | Analysis & breakdown | done | lead | – | complex | spec + `crates/bot`, contracts below |
| B1 | TOML config + API key model | done | lead | A1 | complex | `crates/bot/src/webhook/config.rs`, `crates/bot/Cargo.toml` (new `toml` dep), tests |
| B2 | Markdown subset → safe HTML | done | lead | A1 | complex | `crates/bot/src/webhook/markdown.rs`, tests (AC-3.5) |
| B3 | HTTP server: routes, key auth, rate limit, status codes | done | lead | B1, B2 | complex | `crates/bot/src/webhook/http.rs`, tests (AC-3.3, AC-3.4, AC-3.7) |
| B4 | Bot bridge: live channel tree, delivery, reconnect, `webhook` subcommand | done | lead | B1, B3 | complex | `crates/bot/src/webhook/state.rs`, `crates/bot/src/lib.rs`, `crates/bot/src/main.rs`, tests (AC-3.1) |
| B5 | Docs, example config, README quick-start | done | lead | B1–B4 | simple | `docs/bot-webhook.md`, `crates/bot/examples/bot.example.toml`, `README.md` (AC-3.6 warning wording) |
| C1 | Verification against the local test server | done | lead | B1–B4 | complex | `crates/bot/tests/webhook_live.rs` (AC-3.1–3.8) |
| B6 | Close two spec gaps found in review: server-reported message cap (FR-3.6) and key label + channel id in the request log (FR-3.20) | done | lead | B3 | simple | `crates/bot/src/webhook/http.rs`, `crates/bot/src/webhook/state.rs`, `docs/bot-webhook.md`, tests |
| B7 | Channel passwords (`[[token]]` list, presented at login) per Addendum FR-3.22 | done | lead | B4 | medium | `crates/bot/src/webhook/config.rs`, `crates/bot/src/bot.rs`, `docs/bot-webhook.md`, `crates/bot/examples/bot.example.toml`, tests (AC-3.9) |
| D1 | Implementation report | done | lead | C1 | simple | `specs/03-bot-webhook-implementation.md` |

## Contracts between tasks (decisions that must not drift)

- **Modules:** `crates/bot/src/webhook/{config.rs, markdown.rs, http.rs, state.rs}`, exposed as `pub mod webhook` from `crates/bot/src/lib.rs`.
- **Config (TOML, git-ignored):** `[server]` host/port/name/cert/channel, `[http]` listen (`127.0.0.1:8080` default), `[[key]]` `key`, `label`, `channels` (name or numeric ID, mixed allowed).
- **Hard rules:** message cap 5000 chars, body cap 20 KB → `413`, rate limit 20 requests/min per key → `429`, delivery wait 3 s, key compared in constant time, key checked before the body is parsed.
- **Channel passwords:** a separate `[[token]]` list (`channel` + `password`), presented at login; the login code takes the passwords from the config, nothing else. Passwords are never logged.
- **Status codes:** `200 {"delivered":true}` · `401` unknown key · `403` channel not allowed for key · `404` unknown channel · `400` bad/empty/too long body · `413` body too large · `405` wrong method · `429` rate limited · `503` not connected. No channel list in error bodies.
- **Delivery answer:** murmur does not echo a client's own chat message, so "delivered" means the server accepted it: sent on the control channel and no `PermissionDenied` for chat within the 3 s window. This wording goes into the docs and the report (spec §13).
- **Logging:** timestamp, key label, channel id, status. Never the key, never the message text.

## Verification status

Build, unit tests, wire test and clippy pass. The live test `crates/bot/tests/webhook_live.rs` ran against the local standard server (`testenv/up.sh v15`) and passed: AC-3.1–3.4, AC-3.6, AC-3.7, AC-3.8 ✅, AC-3.5 ✅ on the bot side with a manual look at the client rendering still open.
