# Mumcord · Epic 3 – Identity manager (several identities, per-server choice) · Specification

> Status: Draft for review · As of: 2026-10-30
> Sources: issue #148 (Epic 3, milestone 0.5.0), issue #63 (Epic 3 overview), Epic 3 mockups (wiki: `epic3-ids-dark/light`, `epic3-srv-dark/light`), `specs/01-foundation-specification.md` (FR-1.2, FR-1.3, FR-1.34, section 7), `specs/02-multi-server-themes-specification.md` (multi-server behaviour, per-server settings)
> Builds on: spec 01 and spec 02; all decisions there stay valid unless this spec changes them
> Numbering: this spec uses FR-3.1 … FR-3.19 and AC-3.1 … AC-3.14 for the identity manager only. Other Epic 3 specs continue from FR-3.30.

## 1. Goal & context

Today Mumcord has exactly one identity: a client certificate with its private key, created in the wizard or imported from a PKCS#12 file, stored in the OS secure storage. Every server connection uses it.

Mumble servers recognise a user by that certificate: the registered name, the permissions, the avatar and the friends list of other people all hang on it. People who play on a gaming server with friends, join a work Mumble with their real name, and stream on a third server want three separate appearances – and they want to switch between them without re-importing files.

This spec makes identities a list: create, import, rename, export, delete and inspect several identities, mark one as the default, and pick a different one per saved server. Certificates and keys stay in the OS secure storage; the database only ever holds non-secret labels and fingerprints. Upgrading changes nothing for existing users: their single identity becomes "Default" and every server keeps connecting with it.

Nothing changes on the server side. A server sees a normal Mumble connection with a different client certificate.

## 2. Scope

### 2.1 In scope

- Identity list in Settings → Identity: create, import, rename, export, delete, details, "set as default".
- Delete with strong warning, typed confirmation, and a fallback rule for servers using the identity.
- Per-server identity: "Use default" or a specific identity, in the server's menu and in the server's edit dialog, with a confirmation before the server reconnects.
- The identity in use is visible where the user needs it: server header badge (only when it is not the default), connection info panel, and the fallback name in the user panel.
- Storage: metadata in SQLite, certificate and key only in the OS secure storage.
- Migration of today's single identity to "Default" plus a new keychain layout.
- All new texts in all 24 locales.

### 2.2 Out of scope / later

- Identity-aware blocks (#140) and "only accept private messages from friends" (#141): both features do not exist yet. This spec records the decision that they will be **per own identity** (section 11) but builds nothing.
- The redesigned add/edit server dialog (#151). It will embed the same identity dropdown; this spec adds the dropdown to the current dialog so the feature is usable now.
- Redesigned Settings (#161) and a per-server settings page inside Settings → Servers (as sketched in the mockup). The dropdown lives in the server dialog and the server menu for now.
- The setup wizard (#149, #159) and its identity step: unchanged. A rerun never touches identities.
- Several identities used at the same time inside the peer-to-peer layer: direct connections always use the default identity (section 4.6).
- Certificate expiry warnings, certificate renewal, hardware-backed keys, passwords on identities inside Mumcord (the PKCS#12 export password stays the only one).
- Importing several identities at once from one file, or importing from other clients' profile folders (that is #66).

## 3. Roles & permissions

Nothing is enforced on the server side. The server treats each identity as a separate user.

| Action | Guest | Registered user | Channel admin | Server admin | Enforced by |
|---|---|---|---|---|---|
| Create, import, rename, export, delete a local identity | ✅ | ✅ | ✅ | ✅ | Client (local data) |
| Choose the identity of a saved server | ✅ | ✅ | ✅ | ✅ | Client (a normal Mumble connection with another certificate) |
| Keep a registered name after switching the identity | ❌ (the new identity is unregistered) | ❌ (the name belongs to the old certificate) | ❌ | ❌ | Server – the client only explains this before switching |
| Delete an identity other servers would fall back from | ✅ | ✅ | ✅ | ✅ | Client (typed confirmation) |

## 4. Functional requirements

### 4.1 Identity list

- **FR-3.1** Settings → Identity shows an **identity list** instead of the single current identity. Each entry shows:
  - a round avatar with the first letter of the label in a colour that stays the same for that identity (colour chosen from a fixed palette by identity id, not by certificate content),
  - the **label** (user-chosen, local only) and, when it is the default, a "Default" badge,
  - "Name: ‹common name from the certificate› · Created ‹date›" (`Created` = when the identity was added to Mumcord, day precision),
  - "Fingerprint ‹grouped SHA-1 hash, first 8 and last 4 groups, middle elided› · Used by ‹n› servers: ‹labels›", or "· Not used by any server yet", or, for the default, "· All servers without their own identity" when no server uses it explicitly,
  - the actions for that entry: "Set as default" (only for non-default entries), "Details", "Rename", "Export…", "Delete".
  Above the list: an intro text explaining what an identity is and that it can be chosen per server, plus the buttons "Import…" and "New identity" (primary).
  Below the list: a warning box (text plus icon, not colour only): "Back up before you delete. If you delete an identity without an export, you lose the names and permissions registered with it on servers. This cannot be undone."
- **FR-3.2** The list is ordered: default identity first, then the other identities by the date they were added (oldest first).
- **FR-3.3** Exactly one identity is the default. The first identity ever created or imported is the default automatically; "Set as default" makes another entry the default at once (no confirmation). Servers that use "Use default" follow the change on their next connect; connected servers are not reconnected by this action.
- **FR-3.4** **Details** opens a dialog with the full fingerprint (grouped in 4-character blocks, monospace, selectable), the common name inside the certificate, the date it was added to Mumcord, whether it is the default, and the servers that use it (their labels, each with the note whether it uses this identity directly or through the default). A server list that is empty shows "Not used by any server yet". Details is read-only.

### 4.2 Create, import, rename, export, delete

- **FR-3.5** **New identity** opens a dialog with two fields: "Name on servers" (the certificate's common name, e.g. "Zoe", 1–64 characters, required) and "Label" (local, defaults to the name, 1–40 characters, required, may repeat across identities but not be empty). The primary button "Create" creates a self-signed RSA-2048 certificate exactly as today (FR-1.2), stores it in the secure storage, adds it to the list and closes the dialog. The new identity is **not** made the default automatically unless it is the first one. Generating takes a moment on slow machines: the button shows a busy state and the dialog stays keyboard-locked until done.
- **FR-3.6** **Import…** picks a PKCS#12 file (`.p12`/`.pfx`, existing file dialog) and asks for the file's password (existing password dialog, empty password allowed). Import **adds** an identity; it no longer replaces the current one (replaces FR-1.3). The label defaults to the certificate's common name (or the file name when the certificate has none) and can be changed in the same dialog before saving. An identity whose certificate hash is already in the list is refused with "This identity is already in your list." and nothing is stored. The first identity ever imported becomes the default.
- **FR-3.7** **Rename** changes only the local label. A dialog with one field, "Save" applies. The certificate, the common name, the fingerprint and everything the servers know stay untouched; the dialog says so in one line.
- **FR-3.8** **Export…** exports the chosen identity as a password-protected PKCS#12 file, exactly as today (FR-1.3): file dialog, password entered twice, non-empty password required, default file name `mumcord-identity-‹label, filesystem-safe›.p12`. The export always refers to the identity whose button was used, never to "the current one".
- **FR-3.9** **Delete** opens a warning dialog that cannot be confused with a normal confirmation:
  - Title "Delete the identity ‹label›?", an icon plus text: losing a certificate without a backup means losing the registered name and permissions on servers, and it cannot be undone.
  - It lists the servers that use the identity (directly or through the default). When any do, a second line explains: "These servers will use the default identity ‹default label› afterwards. Their registered names and permissions stay with the old certificate."
  - It offers "Back up first" (starts the export flow for this identity and returns to the warning afterwards) and "Delete" (danger).
  - Deleting requires a **typed confirmation**: a text field labelled "Type the identity label to confirm" that must match the label exactly (trimmed, case-sensitive). The Delete button is disabled until it matches.
  - After deleting, the identity and its key material are gone from the secure storage, and every server that used it directly is set back to "Use default" (section 5).
  - If the deleted identity was the default, the oldest remaining identity becomes the default. The dialog says which one before deleting.
  - **The last identity cannot be deleted** (Mumcord always needs one). Its Delete button is disabled with the hint "Mumcord needs at least one identity."
- **FR-3.10** Deleting an identity that a **connected** server uses disconnects that server first and leaves it disconnected, with a notice in the server bar ("Identity deleted – connect again with ‹default label›"). Other servers are not affected.

### 4.3 Identity per server

- **FR-3.11** Every saved server has an **identity setting**: "Use default (‹default label›)" or one specific identity. It is shown and changed in two places, both using the same dropdown component (themed, keyboard operable, `aria-expanded`, arrow keys, Escape):
  - the server's context menu → new entry "Identity…", which opens a small dialog: title "Identity for ‹server›", the dropdown listing "Use default (‹label›)" followed by every identity as "‹label› (‹common name›)", a link "Manage identities…" that opens Settings → Identity, the hint "Changing the identity reconnects this server. Your registered name and permissions here belong to the identity you pick.", and the buttons "Save and reconnect" / "Cancel";
  - the server's edit dialog (the current one, until #151 replaces it): a field "Identity for this server" with the same dropdown, above the behaviour settings.
- **FR-3.12** Saving a changed identity while the server is connected asks once: "Reconnect ‹server› with ‹label› now?" with "Reconnect" and "Later". "Reconnect" disconnects and connects **only that server**; "Later" stores the choice and the server keeps its current connection until the next connect. Saving while the server is disconnected just stores the choice. Choosing the identity it already uses changes nothing and shows no confirmation.
- **FR-3.13** A server that reconnects because of an identity change starts **read-only** like any fresh connect (FR-2.3) and gets no "Rejoin ‹channel›?" prompt: the new identity is a new user on the server, so voice is not restored automatically. If the server was the voice server, voice moves to it only when the user joins a channel.
- **FR-3.14** A server whose chosen identity is missing (deleted by hand in the database, or unreadable in the secure storage) connects with the default identity instead. The server bar shows a one-time notice naming the server, and the log gets a warning without any secret content.

### 4.4 Where the identity in use is visible

- **FR-3.15** The **chat header** (the conversation title line, where the server is shown for server messages) gets a small pill "Identity: ‹label›" **only when the shown server uses a non-default identity**. With the default identity nothing extra is shown. The pill has the accessible description "Connected with the identity ‹label›". (Implementation note: the channel column header holds the follow menu only, so the chat header is the one place that is always visible.)
- **FR-3.16** The **connection info panel** (AC-1.23) gets one more row, "Identity", showing "‹label› (‹common name›)" and the short fingerprint, for the shown server. It is filled from local data, so it shows even while the panel reports a connection problem.
- **FR-3.17** The user panel's fallback name while not connected (and the "Register on this server" export) uses the identity of the **shown server** instead of always the default. The registration dialog explains that the registered name belongs to that identity, and its "Export identity" button exports exactly that identity.

### 4.5 Storage, security & migration

- **FR-3.18** Certificates and private keys live **only** in the OS secure storage. SQLite holds only: identity id, label, certificate common name, certificate hash (SHA-1, as Mumble uses it), the date it was added, and whether it is the default. No key material, no PKCS#12 blob, no password ever reaches the database, the settings JSON, a log line or a crash report. `Identity`'s `Debug` output still hides the key.
- **FR-3.19** **Migration** (existing installations): the stored single identity is moved into the new layout on first start, without user interaction and without a reconnect:
  - It becomes an entry labelled **"Default"** with its certificate's common name and hash; "added" date = the date of the upgrade.
  - Every saved server keeps "Use default", so every server connects with the same certificate as before; the registered names and permissions stay.
  - The old keychain items are removed only after the new ones are written and readable; if anything fails, the old layout stays and the next start tries again (the app keeps working with the old single identity in the meantime).

## 5. Data model

### 5.1 SQLite (new migration, appended to `crates/store/src/db.rs`)

```sql
CREATE TABLE identity (
    id         INTEGER PRIMARY KEY,
    label      TEXT    NOT NULL,
    cert_name  TEXT    NOT NULL,   -- CN from the certificate
    cert_hash  TEXT    NOT NULL UNIQUE, -- SHA-1, 40 lowercase hex, as UserState.hash
    created_at INTEGER NOT NULL,   -- when it was added to Mumcord (Unix ms)
    is_default INTEGER NOT NULL DEFAULT 0 CHECK (is_default IN (0, 1))
);
-- Exactly one default identity.
CREATE UNIQUE INDEX identity_one_default ON identity (is_default) WHERE is_default = 1;

ALTER TABLE saved_server ADD COLUMN identity_id INTEGER REFERENCES identity (id) ON DELETE SET NULL;
```

- `saved_server.identity_id IS NULL` means "use the default identity", so the upgrade needs no server rows to be rewritten and a deleted identity automatically falls back.
- Deleting an identity removes the key material **first** (like removing a server): if the secure storage fails, the row stays and the user can retry instead of leaving an orphaned secret behind. Then one transaction deletes the row — servers fall back through `ON DELETE SET NULL` — and, if it was the default, promotes the oldest remaining identity.
- The store refuses to delete the last identity (`Error::LastIdentity`) and to delete an unknown id (`Error::UnknownIdentity`).

### 5.2 OS secure storage (`crates/store/src/secrets.rs`)

| Account | Content |
|---|---|
| `identity/<id>/count` | number of chunks of identity `‹id›` (decimal text) |
| `identity/<id>/<n>` | chunk `n` of the identity as PKCS#12 (empty password; protection comes from the OS store) |
| `server/<server id>` | unchanged (password and access tokens) |

- Only Windows chunks (Credential Manager caps a secret at 2560 bytes); the chunk size and the "write count last" ordering are unchanged.
- Legacy layout (`identity`, `identity/<n>`) is read once, migrated (section 4.5) and deleted afterwards.
- Reads are cached per session like today, now per identity id, so a list of five identities does not cost five prompts per screen.
- New store errors: `LastIdentity`, `UnknownIdentity`, `DuplicateIdentity`, `EmptyIdentityLabel`.

## 6. Engine & command API

New/changed Tauri commands (generated bindings are regenerated with the usual `export_bindings`; `docs/engine-api.md` is updated):

| Command | Signature | Notes |
|---|---|---|
| `identity_get` | → `Identity \| null` | unchanged meaning: the **default** identity, `null` before the wizard created one. Keeps the wizard gate and old callers working. |
| `identity_list` | → `Identity[]` | all identities, default first, then by `createdAt`. |
| `identity_create` | `name, label?` → `Identity` | new self-signed certificate; `label` defaults to `name`; becomes default when it is the first. |
| `identity_import` | `path, password \| null, label?` → `Identity` | **adds** an identity; `duplicateIdentity` when the hash is already in the list. |
| `identity_rename` | `id, label` → `Identity` | local label only. |
| `identity_export` | `id, path, password` → `null` | exports that identity. |
| `identity_delete` | `id` → `null` | typed confirmation happens in the UI; the engine enforces "not the last identity". |
| `identity_set_default` | `id` → `Identity[]` | returns the new order. |
| `server_set_identity` | `serverId, identityId \| null` → `SavedServer` | `null` = use default. Does not reconnect; the UI asks and then calls `server_connect`. |

- `Identity` (TS) gains `id`, `label`, `certName`, `createdAt`, `isDefault`, `serverIds` (servers that use it **directly**), plus the existing `name` (common name) and `certHash`. `SavedServer` gains `identityId: number | null`.
- Engine internals: the single `Mutex<Option<Option<Identity>>>` identity cache becomes a map keyed by identity id, loaded on demand from the secure storage. The connect path resolves `server.identity_id` → identity, falling back to the default per FR-3.14. `connect` reads the identity for that server only; two servers with different identities connect independently.
- **Peer-to-peer** (full-size images, watch party, screen sharing) always uses the **default** identity, independent of the server connections: the direct connection identifies the device, not the server account.
- Profile/registration commands keep working per connection and now use the identity of that connection (they already use the connection's own certificate hash from the server's `UserState`).

## 7. UI

### 7.1 Settings → Identity (replaces today's single-identity view)

```
Settings                                                        [ Close ]
 ┌───────────────┐
 │ Identity      │  Identities                        [Import…] [New identity]
 │ Servers       │  An identity is a certificate stored in your system's secure
 │ Audio         │  storage. Servers recognise you by it. Use one identity
 │ Voice         │  everywhere, or pick a different one per server.
 │ …             │  ┌────────────────────────────────────────────────────────┐
 │               │  │ (M) Main [Default]                [Details] [Rename]   │
 │               │  │     Name: Zoe · Created 12 Mar 2026  [Export…] [Delete]│
 │               │  │     Fingerprint 4F:A2:9C:1B:…:6A:B1 · Used by 3 servers│
 │               │  │     Friends Hangout, Book Club, Game Night             │
 │               │  └────────────────────────────────────────────────────────┘
 │               │  ┌────────────────────────────────────────────────────────┐
 │               │  │ (S) Streaming      [Set as default] [Details] [Rename] │
 │               │  │     Name: ZoeLive · Created 2 Jun 2026  [Export…] [Delete]│
 │               │  │     Fingerprint B9:03:7E:44:…:D0:2E · Used by 1 server │
 │               │  └────────────────────────────────────────────────────────┘
 │               │  ┌────────────────────────────────────────────────────────┐
 │               │  │ (W) Work           [Set as default] [Details] [Rename] │
 │               │  │     Name: Zoe (Acme) · Imported from the official …    │
 │               │  │     Fingerprint 1D:77:E0:5B:…:F3:09 · Not used yet     │
 │               │  └────────────────────────────────────────────────────────┘
 │               │  ┌────────────────────────────────────────────────────────┐
 │               │  │ ! Back up before you delete. … This cannot be undone.  │
 │               │  └────────────────────────────────────────────────────────┘
 └───────────────┘
```

- The section keeps the existing groups below the list (default avatar, now playing, now gaming) unchanged.
- The identity list is a labelled list (`role="list"`, each entry a `listitem` with a heading-level label); every action button's accessible name contains the identity label ("Export Streaming").
- After every action that changes the list, focus returns to the button that started it, or to the neighbouring entry when that entry is gone (e.g. after deleting the last row, focus goes to "New identity").
- Empty state: only the intro text and the buttons (cannot happen after the wizard, but the section must not break if the list is empty).

### 7.2 Identity for a server (context menu → "Identity…", and the edit-server dialog)

```
Identity for Stream Lounge                              [ × ]
 ┌──────────────────────────────────────────────────────────┐
 │ Choose which identity Mumcord uses to connect here.      │
 │ "Use default" follows the default identity (Main).       │
 │  ┌────────────────────────────────────────────────────┐  │
 │  │ Streaming (ZoeLive)                            ▾  │  │
 │  └────────────────────────────────────────────────────┘  │
 │    Use default (Main)                                    │
 │  ✓ Streaming (ZoeLive)                                   │
 │    Main (Zoe)                                            │
 │    Work (Zoe (Acme))                                     │
 │    Manage identities…                                    │
 │ ┌──────────────────────────────────────────────────────┐ │
 │ │ i Changing the identity reconnects this server. Your  │ │
 │ │   registered name and permissions here belong to the  │ │
 │ │   identity you pick.                                 │ │
 │ └──────────────────────────────────────────────────────┘ │
 │                                    [Cancel] [Save and reconnect]
 └──────────────────────────────────────────────────────────┘
```

- "Save and reconnect" is the label while the server is connected, "Save" while it is not.
- The dropdown uses the shared themed dropdown of FR-2.20 (not a native `<select>`), so it matches the mockup and works with the keyboard alone.
- "Manage identities…" closes the dialog and opens Settings → Identity (same behaviour as the existing settings jump, with the settings section focused).

### 7.3 Server header badge and connection info

- Server header: `Nordic Gamers  ( Identity: Streaming )` – a bordered pill, small text, muted colours; only for non-default identities (FR-3.15).
- Connection info panel: a new "Identity" row after "Transport", value "Streaming (ZoeLive) · 4F:A2:9C:1B" (short fingerprint, monospace).

## 8. Accessibility

- Both new dialogs and the identity list are fully keyboard operable; focus is trapped in a dialog while it is open and returns to the opening control afterwards (existing `Dialog` behaviour).
- The dropdown is a `button` with `aria-haspopup="listbox"`, `aria-expanded`, a `listbox` with `option` children and `aria-selected`; arrow keys, Home/End, Enter and Escape work; the current value is announced.
- Validation errors ("Type the identity label to confirm" mismatch, empty label, duplicate identity, wrong file password) are announced via `role="alert"` and referenced with `aria-describedby`; nothing fails silently.
- The delete warning is text plus an icon, never colour only. The "Delete" button is the only danger-styled control and is disabled until the typed confirmation matches (with `aria-disabled` and a visible hint).
- Works with the system theme (dark and light) and with text scaling up to 200 % without clipping the action buttons.

## 9. Languages

Every new text exists in all 24 locales (`bg, cs, da, de, el, en, es, et, fi, fr, ga, hr, hu, it, lt, lv, mt, nl, pl, pt, ro, sk, sl, sv`), in `settings.json` (identity manager) and `servers.json` (server identity, header badge). Fingerprints, dates and server labels are formatted with the active locale (dates via the existing date formatting helper).

## 10. Tests & acceptance criteria

Unit and integration tests are written with the code:

- **Store:** migration creates `identity`, keeps `identity_id` NULL for all servers; default-uniqueness index rejects a second default; delete promotes the oldest remaining identity and clears `identity_id` of its servers; last identity cannot be deleted; duplicate hash is refused; label validation.
- **Secrets:** new chunk layout round-trips an identity per id; legacy layout is migrated exactly once and the old items are gone afterwards; a failed migration keeps the legacy items and still returns the identity; per-id cache means one read per identity per session.
- **Engine:** `identity_list` order; `identity_create`/`import`/`rename`/`export`/`delete`/`set_default`; `server_set_identity` stores `identityId`; connect uses the server's identity; missing identity falls back to the default and emits the notice; p2p uses the default identity.
- **UI:** identity list renders default badge, "used by" line and per-entry action names; create/rename/import/delete dialogs (typed confirmation disables Delete, mismatch keeps it disabled, "Back up first" opens the export dialog); dropdown options and keyboard navigation; server identity dialog with and without connection; header badge only for non-default; connection info row; user panel fallback name per shown server.
- **Live (local test servers `v15` and `v15b` from `testenv/`):** two servers, two identities, both connected at the same time; each server's user list shows the certificate hash of its own identity; switching one server's identity reconnects only that server.

Acceptance criteria (checked in the running client against the local test server; no public servers):

- **AC-3.1** Upgrade path: start 0.5.0 with an existing 0.4.x profile → Settings → Identity shows one entry "Default" with the old certificate's fingerprint; the server connects with the same registered name as before, without a new registration and without a reconnect prompt.
- **AC-3.2** Create two identities ("Work" with name "Zoe (Acme)", "Streaming" with name "ZoeLive") → both appear in the list with their own colour avatar and fingerprint; each fingerprint matches `openssl x509 -fingerprint -sha1` of the exported file (first 8 groups compared).
- **AC-3.3** Import a PKCS#12 file exported by the official Mumble client → it is added to the list with its common name as the label, and importing the same file again is refused with "This identity is already in your list." and no second entry appears.
- **AC-3.4** Rename "Streaming" to "Stream" → the label changes everywhere (list, dropdown, header badge) while the fingerprint and the common name stay the same; export after the rename produces the same certificate hash as before.
- **AC-3.5** Export "Work" with a password → the file opens in the official Mumble client (or `openssl pkcs12 -info`) with the same certificate; exporting with an empty password is refused.
- **AC-3.6** Delete an identity that a saved server uses → the dialog lists that server, explains the fallback to the default, offers "Back up first"; typing the wrong label keeps "Delete" disabled, typing the exact label enables it; after deleting, the server shows "Use default" and connects with the default identity.
- **AC-3.7** Deleting the default identity with two others present names the identity that becomes the new default; after deleting, that identity has the "Default" badge and servers without their own identity connect with it.
- **AC-3.8** The last remaining identity cannot be deleted: the Delete button is disabled with the hint, and the engine refuses the call as well.
- **AC-3.9** Two servers connected at the same time with different identities → each server's user list (checked from a second client or a bot) shows the different certificate hashes; both connections stay up.
- **AC-3.10** Change the identity of a connected server via the context menu → the confirmation appears, "Reconnect" reconnects only that server (the other stays connected, voice unaffected), and the server header shows the new "Identity: …" badge while the other server shows none.
- **AC-3.11** Same change with "Later" → nothing reconnects now; after disconnecting and connecting the server it uses the new identity.
- **AC-3.12** A server with a non-default identity: connection info shows the "Identity" row with label, common name and short fingerprint; with the default identity the header badge is absent and the connection info row shows the default identity.
- **AC-3.13** Nothing secret in local data: after creating and importing identities, `mumcord.sqlite3` and the settings JSON contain labels, common names, fingerprints and dates only (checked with `strings`/`sqlite3`); the log files contain no key material and no PKCS#12 data; the keychain holds the identities under `identity/<id>`.
- **AC-3.14** Accessibility and languages: the whole flow (create → rename → set default → details → export → delete) is doable with the keyboard alone, in dark and light theme and at 200 % text scaling, with screen-reader labels on every control and the dropdown announcing its value; the German and Polish UI show the new texts fully translated (spot check of two locales, all 24 present in the files).

## 11. Decision log

| Question | Decision | Note |
|---|---|---|
| D1 Where do identities live? | Certificates and keys only in the OS secure storage; SQLite holds label, common name, hash, date, default flag | Issue requirement, FR-3.18 |
| D2 How is "default" modelled? | One `is_default` flag with a unique partial index; exactly one default, first identity becomes it | FR-3.3, section 5.1 |
| D3 How is a per-server override modelled? | `saved_server.identity_id`, `NULL` = use default | Upgrade needs no data change, delete falls back automatically |
| D4 Does import replace or add? | Adds; duplicate fingerprints are refused; the wizard's import on a fresh install still becomes the default | FR-3.6 (replaces FR-1.3) |
| D5 Deleting the default? | The oldest remaining identity becomes the default; the dialog names it beforehand | FR-3.9 |
| D6 Deleting the last identity? | Not allowed | FR-3.9 |
| D7 What happens to a connected server when its identity is deleted? | It is disconnected and left disconnected with a notice; other servers unaffected | FR-3.10 |
| D8 Does switching the identity restore the channel/voice? | No: the reconnect is a fresh read-only connect, no rejoin prompt | FR-3.13 |
| D9 Where is the per-server choice? | Server context menu ("Identity…" dialog) and the current edit-server dialog; the settings-page mockup moves to #161/#151 | Section 2.2, FR-3.11 |
| D10 Which identity does the peer-to-peer layer use? | The default identity, independent of server connections | Section 6 |
| D11 Are blocks (#140) and "PMs from friends only" (#141) per identity or per server? | **Per own identity** – that is what the other side sees; recorded for those specs, nothing built here | Section 2.2 |
| D12 Is the identity name inside the certificate ever changed by rename? | No, rename is a local label only | FR-3.7 |
| D13 What happens when a chosen identity is missing? | Connect with the default, show a notice, log without secrets | FR-3.14 |
| D14 Wizard behaviour | Unchanged; a rerun never touches identities (#149) | Section 2.2 |

## 12. Open points & risks

- **Keychain prompts per identity.** Every identity is at least one secure-storage item. On macOS an unsigned build may ask for the login password per item, so a list of five identities can prompt several times per start. Mitigation: per-id caching (section 5.2) and reading an identity only when it is needed (connect, export, details). A signed build removes the prompts.
- **RSA key generation on first create** takes up to a few seconds on slow machines; the dialog shows a busy state (FR-3.5). Importing an existing identity avoids the wait.
- **Migration edge cases:** a corrupted legacy identity must not brick the app – the migration then reports "identity unreadable" and the wizard offers to create a new one, as today. A user who deletes identities outside Mumcord (e.g. via the OS keychain app) gets the FR-3.14 fallback.
- **Server-side consequences are the user's:** switching identities loses the registered name on that server until it is re-registered with the new identity. The dialog warns about it, and the delete warning is deliberately strong.
- **Test coverage for "two servers, two identities"** needs both local test servers (`v15`, `v15b`) and two identities; the live test is part of the epic's verification, not of CI.

## 13. Impact on later Epic 3 issues

- **#151 (add/edit server dialog):** the identity dropdown and its dialog already exist; the redesign only moves the dropdown into the new "Identity" section and reuses `ServerIdentityDialog`.
- **#147 (server info panel):** shows the identity from the connection info row built here.
- **#140 (blocks) and #141 (PMs from friends):** keyed by own identity (D11).
- **#149 (wizard rerun):** must not touch the identity list.
- **#66 (import from the official Mumble client):** imports identities through `identity_import`.
