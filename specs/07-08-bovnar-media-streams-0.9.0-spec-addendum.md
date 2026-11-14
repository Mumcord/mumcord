# Addendum — Bovnar media streams for screen share and camera (0.9.0)

> **Merged, 2026-11-10.** The requirements of this addendum are now part of the two
> specifications on `milestone/0.9.0` and are no longer kept here. The specifications are
> authoritative; this file is only the merge record.
> `specs/07-file-sharing-screenshare-specification.md` and
> `specs/08-camera-sharing-specification.md` win over this file and over the plan.
>
> The addendum superseded the clean-break decision of 2026-11-04: 0.9.0 keeps the old media
> format as a capability-based fallback and adds a direct camera path.

## 1. Where the requirements live

| Topic | Specification |
|---|---|
| Media document, channel map, channel-0 control, track message, plugin data, limits | Spec 07 §4.11, FR-7.89 … FR-7.94 |
| Description, capability negotiation, fallback matrix, old-format retention, refusal texts, late joiner, packet inspector | Spec 07 §4.11, FR-7.95 … FR-7.101 |
| Protocol additions (hello `media-v2`, `Watch` / `mumcord.peer` `mv`, relay `bind` `"media": 2`, mumpeer `/v1/status`) | Spec 07 §6.6; spec 08 §6.5 |
| Camera direct path, direct control, path choice, camera fallback, camera without a relay | Spec 08 §4.14, FR-8.92 … FR-8.96 |
| Acceptance criteria | Spec 07 §11 AC-7.72 … AC-7.84; spec 08 §11 AC-8.29 … AC-8.41 |
| Decision log | Spec 07 §12 (D1–D10, D14); spec 08 §12 (D11–D13, the 0.8.x camera fallback and the 0.8.0 `needs` forward compatibility) |

## 2. FR / AC number mapping

| Addendum | Specification |
|---|---|
| FR-7.89 … FR-7.101 | Spec 07, unchanged numbers |
| FR-8.92 … FR-8.96 | Spec 08, unchanged numbers |
| AC-7.72 … AC-7.84 | Spec 07 §11 |
| AC-8.29 … AC-8.41 | Spec 08 §11 |

The numbers stay provisional until the 0.8.0 branch is merged into `main`. They start after
the highest numbers on `main` (FR-7.88, FR-8.91, AC-7.71, AC-8.28) and are renumbered once
after that merge, never against `milestone/0.8.0`.

The `needs` message adds no FR or AC number: it refines FR-8.95 and AC-8.31 and is specified
in spec 08 §6.5, so the AC range AC-8.29 … AC-8.41 above stays current.

## 3. Decisions D1–D14

| # | Decision | Where it now lives |
|---|---|---|
| D1 | Description on channel 0 only; no `.meta` preamble | Spec 07 FR-7.91, FR-7.95 |
| D2 | Description encrypted on the relay path, resent at every rekey, relay caches the ciphertext opaquely | Spec 07 FR-7.95, FR-7.100 |
| D3 | Channel map with channel = 1 + temporal layer; keyframe on layer 0; max 4 layers in channels 1–4; P0.4 passed, hardware through mock drivers and normal acceptance | Spec 07 FR-7.90 |
| D4 | SFrame header (RFC 9605); AAD = channel varint + flags + SFrame header; timestamp inside the ciphertext | Spec 07 FR-7.92 |
| D5 | The one-byte `KEYFRAME_REQUEST` stays; `WANT = 2` followed by a big-endian u16 tile height on the direct camera reverse stream | Spec 07 FR-7.91; spec 08 FR-8.93 |
| D6 | Plugin relay data moves to channel 6 on new links; old links keep the old header path | Spec 07 FR-7.93 |
| D7 | Late joiner gets the cached description and prologue after the key exchange, then frames from the next keyframe; publisher sends a keyframe on request | Spec 07 FR-7.100 |
| D8 | QUIC relay transport: one unidirectional stream per message, in order | Spec 07 FR-7.89 |
| D9 | Capability names: hello `media-v2`, `mv`, relay bind `"media": 2`, mumpeer `/v1/status` `"media": 2`; absent = old format | Spec 07 FR-7.96 |
| D10 | Capability-based fallback; old format retained and tested, not a best-effort guess | Spec 07 FR-7.97, FR-7.98 |
| D11 | Direct camera signalling reuses `mumcord.camera`; sender opens `STREAM_CAMERA` to the viewer; description carries the camera session id | Spec 08 FR-8.92, §6.5 |
| D12 | Relay broadcast first when a new relay is available; direct when the relay is old or missing, or in `direct_only` | Spec 08 FR-8.94 |
| D13 | Direct path: 8 viewers per camera; relay: 25 viewers | Spec 08 FR-8.93, FR-8.95 |
| D14 | Old format kept and tested through 0.9.x for 0.8.x peers and old relays; removal is a later decision | Spec 07 FR-7.98 |

## 4. Open points

None. D1–D14 are settled in the specifications. The only provisional item is the FR/AC
numbering above, which is fixed once after the 0.8.0 branch is merged into `main`.

The camera fallback for 0.8.x viewers was decided on 2026-11-10: 0.8.x has no direct camera
receive path and cannot decode the new media profile, so a 0.8.x viewer of a new-format camera
always sees "This camera needs Mumcord 0.9.0" on the tile. There is no old-format direct camera
copy and no second relay broadcast. An old relay still serves 0.8.x viewers with the camera's
old-format relay broadcast as today, and a 0.9.0 viewer of a 0.8.x camera keeps the old relay
path. The screen-share fallback of FR-7.97 is unchanged, because 0.8.x does have a direct
screen-share receive path. The decision is recorded in spec 08 FR-8.94, FR-8.95, §12 and §13
(AC-8.31).

The 0.8.0 forward-compatibility part was decided on 2026-11-10 too: the 0.9.0 sender sends the
new `mumcord.camera` message `needs` (`.v = 1; .t = "needs"; .id = <16 lowercase hex camera
session id>`, spec 08 §6.5) to a viewer whose hello lacks `media-v2` when the camera uses the new
format and it cannot serve that viewer. 0.8.0 ships the receive path, the tile state and the text
in all 24 locales before its release (plan P0.5): the tile shows "This camera needs Mumcord
0.9.0" and stops retrying until the camera's next `on`. A 0.7.x client ignores `needs` and keeps
the FR-8.52 retry. Recorded in spec 08 FR-8.91, FR-8.95, §6.5, §7.2, §12 and §13; the AC range
stays AC-8.29 … AC-8.41 because `needs` adds no new AC number.
