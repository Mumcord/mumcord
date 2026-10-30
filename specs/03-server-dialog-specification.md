# Mumcord · Epic 3 – Redesigned add/edit server dialog · Specification

> Status: Draft for review · As of: 2026-10-30
> Sources: issue #151 (Epic 3, milestone 0.5.0), Epic 3 mockups (`server-dialog-dark/light`), `specs/03-identity-manager-specification.md` (identity dropdown), spec 01 (FR-1.7, FR-1.11, §5.2), spec 02 (FR-2.7, FR-2.17, FR-2.78)
> Builds on: spec 01, spec 02 and the identity manager spec; their decisions stay valid
> Numbering: FR-3.30 … FR-3.44 and AC-3.20 … AC-3.29 (continues after the identity manager spec)

## 1. Goal & context

Adding a server today means scrolling through one long list of fields: address, port, label, user name, password, channel passwords, "Always connect at start" and the per-server theme. Nothing tells the user whether the address is even reachable before saving, and the same list is used for editing with slightly different rules.

This spec groups the fields into four tabs, gives every field inline validation with a plain-language message, and adds a **Test connection** button that names the server's version and answers within a few seconds — before anything is saved. Add and Edit stay the same component, so both look and behave identically.

Only the client changes. The test connection is a normal Mumble login that is thrown away afterwards; no server-side feature is used.

## 2. Scope

### 2.1 In scope

- One dialog for Add and Edit with four tabs: **Connection**, **Identity**, **Passwords**, **Behaviour**.
- Inline validation with clear messages, no silent failures, and a focus jump to the field that needs attention (also across tabs).
- **Test connection**: ping and server version for the values currently in the form, plus the certificate's trust state, without saving anything.
- Keyboard: arrow-key tabs, sensible tab order, Enter submits, Escape cancels, focus returns to the opener.
- Screen-reader labels, announced validation errors and announced test results.
- All new texts in all 24 locales.

### 2.2 Out of scope / later

- Labelled channel passwords (#146): the Passwords tab lists the saved ones as today (numbered, masked, removable). The label appears there once #146 is built.
- "Only accept private messages from friends" (#141) and the mumpeer relay (#62): not built, so the Behaviour tab does not show them. When they exist they belong in that tab.
- Server info panel (#147): the test result line is not the info panel.
- Redesigned Settings (#161) and the setup wizard (#159).
- Certificate pinning UI, importing channel passwords, editing a saved password in clear text.

## 3. Roles & permissions

Nothing new is enforced on the server side.

| Action | Guest | Registered user | Channel admin | Server admin | Enforced by |
|---|---|---|---|---|---|
| Test a connection | ✅ | ✅ | ✅ | ✅ | Client (one normal login, closed again) |
| See the server version in the test result | ✅ | ✅ | ✅ | ✅ | Server (it sends its version to every client) |
| Add/edit a saved server | local only | local only | local only | local only | Client |

## 4. Functional requirements

### 4.1 Layout and tabs

- **FR-3.30** The add/edit dialog is one component (`ServerDialog`) with a tab list at the top and one panel per tab. The title stays "Add a server" / "Edit server" / "Save this server?" (mumble:// link). The footer always shows **Cancel** and the primary button (**Add server** / **Save** / **Save and connect**).
- **FR-3.31** Tabs, in this order:
  1. **Connection** – address, port, label, user name, and the **Test connection** button with its result line.
  2. **Identity** – "Identity for this server" dropdown ("Use default (‹label›)" or an identity, as in the identity manager spec) plus the link "Manage identities…".
  3. **Passwords** – server password (write-only), "Remove the saved password" switch for a saved password, and the saved channel passwords with their remove buttons.
  4. **Behaviour** – "Always connect at start" with its hint, and the per-server theme with its hint.
- **FR-3.32** The tab list is a `tablist` with `tab` buttons and `tabpanel`s: Left/Right arrows (Home/End) move between tabs and switch them, Tab leaves the tab list into the panel, and the panel is labelled by its tab. Each tab button shows a small warning dot when that tab holds an invalid field, with the screen-reader text "‹tab›: needs attention".
- **FR-3.33** A new server opens on **Connection**. Editing a saved server opens on the tab that the user last used in this dialog session; without one, **Connection**.
- **FR-3.34** For a new server the **Passwords** tab shows only the server password field (nothing is saved yet); the channel-password list appears only when editing.

### 4.2 Inline validation

- **FR-3.35** Validation runs when a field loses focus and on submit; messages appear under the field with `role="alert"` and are referenced by the field's `aria-describedby`. Fields start without an error state.
- **FR-3.36** Rules and messages:
  - **Address**: required; must be a host name, IPv4/IPv6 address or `mumble://` link; must not contain spaces. Empty → "Enter the server address."; otherwise → "This doesn't look like a server address."
  - **Port**: required, whole number 1–65535. Empty → "Enter the port."; out of range → "Enter a port between 1 and 65535. Most servers use 64738."
  - **User name**: required, 1–64 characters, no leading or trailing spaces kept. Empty → "Enter the name you want to use on this server."
  - **Label**: optional; when empty the address is used (as today). Longer than 64 characters → "The name in the server bar can be up to 64 characters."
  - **Server password**: no rule (the server decides).
- **FR-3.37** On submit with invalid fields the dialog switches to the first tab that holds an invalid field, focuses that field and shows all its messages. Nothing is saved.
- **FR-3.38** A `mumble://` link pasted into the address fills address, port, user name and (if present) the password, switches to the Connection tab and shows the existing "Filled in from the mumble:// link." hint. A link that cannot be read shows "This mumble:// link can't be read. Check that it was copied completely." and leaves the other fields alone.

### 4.3 Test connection

- **FR-3.39** The Connection tab has a **Test connection** button (secondary style) and a result line below it. It uses the values currently in the form — address, port, user name, the password as it would be saved (a newly typed one, else the saved one) and the chosen identity — so it works before saving and also for a new server. It never saves anything, never stores a certificate trust decision and does not change the saved server or the connection of any server.
- **FR-3.40** While the test runs the button is disabled and shows a busy state ("Testing…"); the result line shows a status message. A test takes at most 15 s, then it reports a timeout.
- **FR-3.41** The result line reports, on success: **ping** ("Ping: ‹n› ms"), the **server version** ("Server version: ‹1.5.0›") and the **certificate state**:
  - "Certificate already trusted" when the fingerprint matches the saved trust decision of this server,
  - "Certificate not trusted yet – you'll be asked when you connect" for an unknown certificate (also for a new server),
  - "Certificate has changed – check it before you connect" when it differs from the saved decision.
  A server that refuses the login (wrong password, name in use) shows that reason and no figures; an unreachable host shows "Couldn't reach the server." plus the address.
- **FR-3.42** Every test result is announced to screen readers (`role="status"`), and the result line names the address it belongs to, so it cannot be mistaken for the saved server's state.
- **FR-3.43** The test uses the same TLS rules as a normal connect: the certificate is checked against the saved decision and the result reports the outcome; the probe itself accepts the certificate only to read the version, because nothing is trusted or connected afterwards.

### 4.4 Behaviour and saving

- **FR-3.44** Saving behaves as today: Add saves the server, writes the password, applies "Always connect at start" and the per-server theme (both are also available for a new server), and with a mumble:// link it connects afterwards. Edit saves, updates password/identity/behaviour as before, and keeps "changing the identity of a connected server asks before reconnecting" (identity manager spec FR-3.12). Closing with Escape or Cancel discards every change without asking.

## 5. Engine & command API

One new command (bindings regenerated, `docs/engine-api.md` updated):

| Command | Signature | Notes |
|---|---|---|
| `server_test_connection` | `input: ServerTestInput` → `TestConnection` | One-shot login probe. `ServerTestInput { address, port, username, password: PasswordUpdate, serverId: ServerId \| null, identityId: u32 \| null }`. `serverId` lets the engine use the saved password (`keep`) and the saved certificate decision. `TestConnection { pingMs, version, certificate: { fingerprint, state: "trusted" \| "unknown" \| "changed" } }`. Errors use the existing codes (`wrongPassword`, `serverUnreachable`, `nameInUse`, `io`, `invalidInput`) plus `timeout`. |

Protocol addition: `Handle::ping() -> Option<Duration>` sends a Mumble `Ping` and waits for the server's answer, so the reported ping is a real round trip rather than the login duration. The probe connects with the existing `connect_with_tap`, waits for `ServerSync`, takes the version from the server info, pings once, then disconnects.

## 6. UI

```
┌ Edit server ─────────────────────────────────────────────────────────────┐
│ ( Connection )  Identity   Passwords   Behaviour                         │
│ ┌──────────────────────────────────────────────────────────────────────┐ │
│ │ Address                                                              │ │
│ │ [ nordic.example.org                                   ]             │ │
│ │ ⓘ For example mumble.example.org. You can also paste a mumble:// …   │ │
│ │ Port                       Name in the server bar                    │ │
│ │ [ 64738 ]                  [ Nordic Gamers            ]              │ │
│ │ Your name on this server                                             │ │
│ │ [ Zoe                                                  ]             │ │
│ │ [ Test connection ]   ✓ Ping: 12 ms · Server version: 1.5.0 ·        │ │
│ │                         Certificate already trusted                  │ │
│ └──────────────────────────────────────────────────────────────────────┘ │
│                                              [ Cancel ]  [ Save ]        │
└──────────────────────────────────────────────────────────────────────────┘
```

- The tab strip sits under the title; the active tab uses the accent underline of the settings navigation, both themes.
- Invalid fields use the existing error style (border + message below); the warning dot on a tab is text plus icon, not colour only.
- The result line uses the success/warning/danger tones of the status bar, with an icon, and wraps at 200 % text scaling.
- Add and Edit are visually identical apart from the title and the primary button label.

## 7. Accessibility

- Tab list: `role="tablist"`, tabs with `aria-selected` and `aria-controls`, panels with `aria-labelledby`; arrows/Home/End switch, focus stays on the selected tab.
- Every field has a visible `<label>`; errors are announced (`role="alert"`) and referenced; the tab warning dot has an accessible name.
- The test result is a live region (`role="status"`), the busy state is announced once ("Testing the connection…").
- Focus returns to the dialog opener on close (existing behaviour); the dialog keeps Tab inside.
- Works in both themes, with system scaling and at 200 % text size without clipping; all controls keyboard operable.

## 8. Languages

Every new text exists in all 24 locales (`settings`-style namespace of the servers feature). Placeholders stay untranslated (`{{address}}`, `{{version}}`, `{{ms}}`).

## 9. Tests & acceptance criteria

Automated:

- Protocol: `Handle::ping()` answers with a round-trip time against the in-repo fake server and gives up cleanly when the connection drops.
- Engine: `server_test_connection` against the local test server returns the version and a ping; a wrong password is reported as `wrongPassword`; an unreachable port as `serverUnreachable`; a changed certificate as `changed` and nothing is trusted afterwards; no slot is created and no connection stays open.
- UI: tab switching by mouse and keyboard (arrows, Home/End), per-tab warning dots, validation messages per field, submit jumps to the first invalid field's tab, mumble:// link fill, test button states (busy, success, error), test result announced, Add and Edit show the same tabs, focus return.

Acceptance criteria (running client, local test server):

- **AC-3.20** Add a server: the dialog opens on Connection; leaving the address empty and pressing "Add server" shows "Enter the server address." under the field, the Connection tab keeps its warning dot, and nothing is saved. Entering `voice.example.org`, port `0` and an empty name shows the three messages and jumps to the first one.
- **AC-3.21** Paste a `mumble://user@voice.example.org:64739/Raid` link into the address → address, port and user name are filled, the hint appears, and saving connects and joins the channel as today.
- **AC-3.22** Press "Test connection" with the local test server's address before saving → within 15 s the line shows a ping in milliseconds, the server version (e.g. 1.5.0) and "Certificate not trusted yet"; the server is not saved and no connection appears in the server bar.
- **AC-3.23** Test with a wrong server password → the line names the wrong password and shows no figures. Test with a port nothing listens on → "Couldn't reach the server." Test with a changed certificate → "Certificate has changed".
- **AC-3.24** Edit a saved, trusted server → the test reports "Certificate already trusted" and the same ping range as the connection info of the live connection.
- **AC-3.25** Keyboard only: Tab reaches the tab list, Left/Right switch tabs, Tab moves into the panel, all fields reachable in reading order, Enter submits, Escape closes and focus returns to the server icon (or the settings entry that opened it).
- **AC-3.26** Screen reader: the tab names and the selected state are announced, each field's error is announced when it appears, and the test result is read out once.
- **AC-3.27** Add and Edit show the same four tabs and the same fields; a new server can set "Always connect at start" and a theme, and both are saved.
- **AC-3.28** Dark and light theme, 200 % text scaling: nothing is clipped, the tab strip and the result line stay readable, the warning dots remain visible without colour.
- **AC-3.29** All new texts are present in all 24 locales (the locale test passes) and two spot-checked languages show no English leftovers.

## 10. Decision log

| Question | Decision | Note |
|---|---|---|
| D1 Tabs or one scrolling list with sections? | Tabs, as in the mockup | FR-3.30, FR-3.31 |
| D2 Same component for Add and Edit? | Yes, one component | FR-3.30 |
| D3 What does the test measure as "ping"? | A real Mumble `Ping` round trip after the login | FR-3.39, section 5 |
| D4 Does the test save anything? | No: no trust decision, no server, no connection | FR-3.39 |
| D5 What happens with an untrusted certificate during a test? | The probe accepts it to read the version and the result says it is not trusted yet | FR-3.41, FR-3.43 |
| D6 Which tab opens for a new server? | Connection | FR-3.33 |
| D7 Are "Always connect at start" and the theme available for a new server? | Yes | FR-3.44, AC-3.27 |
| D8 Where do the PM setting (#141) and the relay (#62) go? | Behaviour tab, once those features exist; not shown now | Section 2.2 |
| D9 Channel password labels? | Stay numbered until #146 | Section 2.2 |

## 11. Open points & risks

- **A test connection is a real login.** The server sees a short login and logout of the user's identity; servers with `autoban` on failed attempts could be annoyed by repeated tests with a wrong password. The button is disabled while a test runs, and the test never retries by itself.
- **Two logins for the same identity** (a test while that server is connected) are fine for Mumble but show the user twice for a moment; the result line names the tested address so it is clear.
- **Ping over TCP only:** the probe does not open the UDP voice link, so the number is the control-channel round trip. It is labelled "Ping" like the connection info's TCP figure; UDP figures need a full connection and stay in the info panel.
- **Fake or captive-portal servers** may accept the TCP connection and never answer the login; the 15 s limit turns that into a clear timeout message.

## 12. Impact on later Epic 3 issues

- **#146** adds the channel label to the Passwords tab.
- **#141** and **#62** add their switches to the Behaviour tab.
- **#147** can reuse `server_test_connection` for a "diagnose" action.
- **#161** moves nothing here: the dialog stays its own component.
