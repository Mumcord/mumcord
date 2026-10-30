# Issue #175 – mumble:// links as a server/channel card

No `specs/03-*-specification.md` exists yet, so GitHub issue #175 is the contract for this build
(part of the Epic 3 overview #63). The task list follows the `/dev` rules: one row per task,
dependencies, complexity, and the acceptance criteria it covers.

Acceptance criteria taken from the issue:

- AC-1 A `mumble://` link in a message shows a card below it (same family as the link preview cards).
- AC-2 Card shows server name (or host), host and port (default 64738), channel path, live ping, users online / max users, server version.
- AC-3 Buttons: Join (Go to channel when that server is already connected), Add to servers, Copy link.
- AC-4 A saved server shows its saved label.
- AC-5 The public ping is sent only when link previews are allowed; nothing is fetched for links from blocked users.
- AC-6 Nothing is connected without a click; credentials in a link are warned about and passwords are never shown.
- AC-7 Unreachable server: "Server not reachable" with Retry.
- AC-8 IPv6 hosts, default port 64738, invalid links show no card, at most 3 cards per message.
- AC-9 Composer: a pasted mumble:// link shows the card preview before sending.
- AC-10 Accessible: keyboard operable, screen-reader labels, status not conveyed by colour alone.
- AC-11 All 24 locales translated, theme tokens only.
- AC-12 Client only, works against an unmodified standard Mumble server.

| ID | Task | Status | Owner | Depends on | Complexity | Covers | Files |
|---|---|---|---|---|---|---|---|
| T1 | Analysis: card family, sanitizers, existing link parsing, ping wire format | done | lead | – | simple | – | see notes |
| T2 | Protocol: encode the public UDP query and decode both reply formats | done | lead | T1 | complex | AC-2, AC-12 | `crates/protocol/src/query.rs` |
| T3 | Engine: query with privacy gate, timeouts and cache | done | lead | T2 | complex | AC-2, AC-5, AC-7 | `crates/engine/src/server_query.rs` |
| T4 | Engine: allow the `mumble:` scheme in the message sanitizer | done | lead | T1 | complex | AC-1, AC-6 | `crates/engine/src/chat/sanitize.rs` |
| T5 | App shell: `mumble_link_stats`, `mumble_link_join` commands + bindings | done | lead | T3, T4 | complex | AC-2, AC-3 | `src-tauri/src/commands/links.rs`, `src-tauri/src/deeplink.rs` |
| T6 | UI: link extraction and card component with buttons and states | done | lead | T4, T5 | complex | AC-1, AC-2, AC-3, AC-4, AC-6, AC-7, AC-8 | `ui/src/features/chat/mumbleLink.ts`, `MumbleLinkCard.tsx`, `sanitize.ts`, `links.ts` |
| T7 | UI: composer preview of a pasted link | done | lead | T6 | simple | AC-9 | `ui/src/features/chat/Composer.tsx` |
| T8 | Styles: card rules with theme tokens only | done | lead | T6 | simple | AC-11 | `ui/src/features/chat/chat.css` |
| T9 | Translations: 24 locales | done | lead | T6 | simple | AC-11 | `ui/src/locales/*/chat.json` |
| T10 | Mock engine + docs/engine-api.md | done | lead | T5 | simple | – | `ui/src/api/mock.ts`, `docs/engine-api.md` |
| T11 | Tests: protocol codec, engine query against a local fake server, UI unit tests | done | lead | T6 | complex | AC-2, AC-6, AC-8, AC-10 | `crates/protocol/tests/query.rs`, `crates/engine/tests/server_query.rs`, `ui/src/features/chat/*test*` |
| T12 | Verification: build, tests, type checks, linters | done | lead | T11 | simple | – | – |
| T13 | Verification in the running client against the local test server | done | lead | T12 | complex | AC-1, AC-2, AC-5, AC-7, AC-12 | `e2e/specs/chat/mumble-link-card.e2e.ts` |

## Notes

- The reply format was taken from the murmur source and is covered by tests against a scripted UDP
  responder, and by `crates/engine/tests/live_server_query.rs` against the local test server
  (`testenv/up.sh v15`, then `MUMCORD_LIVE=1 cargo test -p mumcord-engine --features test-backend
  --test live_server_query -- --ignored --test-threads=1`), which answered with its version, the
  users online and the user limit. T13 ran the E2E spec against `testenv` v15 and v14: the card
  shows the channel and the figures, credentials are warned about and never shown, one message
  gets at most three cards, Join only connects after the click (including the save-and-connect
  dialog) and the composer preview has no action buttons. Dark and light screenshots are in
  `docs/images/screenshots/mumble-link-card-*.png`.
- `#140` (block users) is not implemented yet, so AC-5 is prepared (the query goes through the
  same privacy gate as previews) but the "never for blocked users" rule cannot be enforced.
- The card shows the credentials warning as a plain sentence, and the composer preview has no
  buttons; the collapsed-message rule in the issue has no rendering in the chat today, so the card
  is always shown in full.
- `#148` (identity manager) is not implemented, so the card shows the saved label only, not the
  identity that would be used.
- The public query is the "extended information" UDP ping. murmur answers it only when `allowPing`
  is on (its default), and the answer is untrusted: it is validated and rendered as plain text.
