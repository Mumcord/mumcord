# Bovnar media streams for screen share and camera — 0.9.0 implementation plan

> Status: draft, 2026-11-10.
> Tracks: #492 (decision and measurements), #680 and #681 (done upstream), specs 07 and 08.
> Base branch: `milestone/0.9.0` (created from `main` after 0.8.0 is released).
> Upstream: `bovnar` 2.1.0, tag `v2.1.0`, commit `4895049`, with `bovnar::codec` and `bovnar::live`.
> This file is a plan, not the specification. The spec addendum for specs 07 and 08 is the gate
> for all code below.

Product decisions taken with the owner on 2026-11-10:

- **Keep the old format as a fallback.** 0.9.0 speaks the new Bovnar media profile when the peer
  and the relay support it, and the old 14/21-byte format otherwise. The clean-break decision of
  2026-11-04 is superseded.
- **Camera gets a direct path.** Camera works without a relay and with an old relay, not only
  through a relay broadcast.
- **Start the spec addendum now**, while 0.8.0 finishes; commit it as soon as
  `milestone/0.9.0` exists.

## 1. Result in one sentence

Screen share and camera keep their current features and viewer UI, gain a direct camera path,
and use one endless Bovnar document per media link when both sides and the relay support it;
with 0.8.x peers or an old relay they fall back to today's binary header. A 0.8.x viewer of a
new-format camera cannot be served: the sender sends it `needs`, and it shows a clear "This
camera needs Mumcord 0.9.0" text (P0.5 ships that receive path in 0.8.0).

## 2. What changes for users

- Screen share, share sound, camera and the camera room look and behave as before.
- With two 0.9.0 clients and an updated relay, media uses the new Bovnar profile: less framing
  overhead, less relay work, and no timestamp, resolution or frame sequence visible to the relay.
- With a 0.8.x peer, the old format is used; screen share keeps working, and camera works when an
  old relay carries the old-format broadcast. A 0.8.x viewer of a new-format camera receives
  `needs` and sees the clear "This camera needs Mumcord 0.9.0" text (no old-format direct copy);
  0.8.0 ships that receive path before its release (P0.5).
- With an old relay, screen share and camera fall back to direct connections between 0.9.0
  clients; the relay fan-out is unavailable, and the client says why.
- Camera now works without a relay at all (direct between 0.9.0 clients), which it cannot today.
- A new relay serves both old and new clients: 0.8.x clients keep the old format, 0.9.0 clients
  use the new one.
- The relay does not need to be updated to 0.9.0 before the client, but the new format and the
  camera without a relay need the updated relay (or a direct connection).

## 3. Starting point

Done and closed:

- #680: the prototype measured the mux framing; GO (9–34 ns write and relay read, zero
  allocations).
- #681: `bovnar` 2.1.0 ships `bovnar::codec` (`MuxWriter`, `MuxReader`, `Peek::skip` /
  `Peek::forward`, frames, preambles) and `bovnar::live`. Every acceptance criterion is covered
  by upstream tests, including input split at every byte, messages above 64 KiB, the terminator,
  shared vectors against the C reference in both directions, and the counting-allocator test.
- The published crate carries a clean `PROVENANCE.json`, so the notices gate of PR #486 works
  unchanged.
- #492 stays open until this plan is implemented and released.

Current code that this plan touches:

| Path | Today |
|---|---|
| `crates/engine/src/screen_share/wire.rs` | Direct `STREAM_SCREEN`: 20-byte v1 / 21-byte v2 header per frame |
| `crates/p2p/src/broadcast.rs` | Relay broadcast: 14-byte clear header per unit (kind, codec, layer, key id, sequence, size) |
| `crates/p2p/src/relay.rs` | Broadcast link: one QUIC stream per unit, or `0x01` frames on TCP |
| `crates/engine/src/screen_share/broadcast/link.rs` | Publisher/subscriber: seals the frame *including the 21-byte direct header* as the unit payload |
| `crates/engine/src/screen_share/broadcast/crypto.rs` | SFrame key schedule, but key id and sequence live in the 14-byte header; the header is the AAD |
| `crates/engine/src/plugin_relay/broadcast.rs` | Plugin data through the same broadcast header (`Content::Data`) |
| `crates/engine/src/camera/session.rs`, `recv.rs` | Camera always goes through the relay broadcast and the same publisher/subscriber |

The engine's internal `Frame` / `FrameHeader` type (`wire.rs`) stays the interface between the
media path and everything above it (fanout, viewer, decoder, UI). Only the wire types and the
transport selection change. That is the main scope-control decision: no UI rewrite, no decoder
change.

## 4. Scope

### In scope

- Screen-share video and share sound on the direct `STREAM_SCREEN` path.
- Camera video on the relay broadcast path **and a new direct camera path**.
- Screen-share and camera relay broadcasts over QUIC and TCP.
- The broadcast control messages: `keyreq`, `key`, `kf`, `layers`, `want`, `gone`.
- Plugin data that today uses the same broadcast header (`Content::Data`); the transport change
  forces this, and moving it to the profile's data channel removes the old header completely
  from new-format broadcasts.
- **Both media formats:** the new Bovnar profile and the old 14/21-byte format, selected by
  capability. The old code is retained, tested and marked legacy; it is not dead code.
- Late joiners, layer filtering, keyframe requests, rekey, and the relay's TCP fallback.
- Description changes during a stream (resolution, codec, tracks).
- Capability negotiation through the Mumcord hello, `Watch`, the relay announcement and `bind`.
- The packet inspector, user-visible fallback texts, notices and release notes.

### Out of scope

- File transfer (already Bovnar, #673, shipped in 0.8.0).
- Plugin data to one person on the direct `STREAM_PLUGIN_DATA` path (not a broadcast).
- Watch party: its media is the embedded web player, not the `STREAM_SCREEN` stream. Only its
  shared media-view UI is regression-tested.
- Server-side changes. Mumcord remains client-only against an unmodified Mumble server.
- Removing the old media format. That is a later decision, once 0.8.x is no longer supported.

## 5. The media stream profile (proposal to be fixed by the spec addendum)

### 5.1 One endless document per media link

Every new-format media link carries one Bovnar document that never ends:

```bovnar
#!bovnar 1.2
.media = \x00 …messages… \x00;
```

A message is a mux message: `varint(channel) varint(length) bytes`. Every message starts a new
chunk; a message above 64 KiB spans several chunks, each starting with its channel varint. A
writer ends only when the link closes.

The same document shape is used on the direct stream, the relay over QUIC and the relay over
TCP. Only the transport framing around the document differs.

### 5.2 Channel map (recommendation)

| Channel | Use |
|---|---|
| 0 | Control and stream description |
| 1–4 | Video temporal layers 0–3 (channel = 1 + layer) |
| 5 | Audio (Opus) |
| 6 | Plugin relay data |
| 7–4095 | Reserved |

The channel map is fixed and clear. The relay routes by channel and never needs the
description to do it. A stream with no audio or no data simply never uses those channels.

Video frames are placed on the channel for the temporal layer the encoder reports. A viewer
that wants layer ≤ N receives channels 1…N+1. The relay drops the higher channels whole, at
chunk boundaries; what it forwards is still a valid stream. This replaces today's `layer` field
in the header.

Direct camera uses the same map with only video channels (no audio channel).

### 5.3 Channel 0: control and description

Every channel-0 message starts with a one-byte kind:

| Kind | Message | Direction | Payload |
|---|---|---|---|
| 0 | Description | Sharer → all | Embedded Bovnar document (§5.5) |
| 1 | `keyreq` | Subscriber → publisher | opaque chunk; the relay prepends the u16 slot |
| 2 | `key` | Publisher → one subscriber | u16 slot + opaque chunk; the relay strips the slot |
| 3 | `kf` | Subscriber → relay → publisher | empty |
| 4 | `layers` | Subscriber → relay | one byte, highest temporal layer |
| 5 | `want` | Subscriber → relay → publisher | u16 tile height; the relay prepends the slot |
| 6 | `gone` | Relay → publisher | u16 slot |

The payload layouts stay as they are today, so `keyx` and the admission logic do not change.
The kind byte is clear so the relay can route control messages and cache the latest description
without parsing it.

The description is the first channel-0 message. It is sent again whenever the encoder changes
resolution, codec, rate or track set, and at every rekey (so a late joiner's cached copy always
matches a key it holds). The viewer applies a new description only at a keyframe.

On the direct stream there is no key exchange: QUIC already encrypts the connection, so the
description is clear inside the QUIC stream. On the relay path the description is encrypted
with the share key (§5.4); the relay caches the latest kind-0 ciphertext opaquely and replays it
to a late joiner after the key exchange. If the spec chooses a clear description instead, it
must justify the loss of relay blindness; the recommendation is encrypted.

Direct viewer → sharer keyframe requests stay the current one-byte `KEYFRAME_REQUEST` on the
reverse stream. That stream is not relayed and is not part of the media document; keeping it
avoids a second framing path. For direct camera, the same reverse stream gains a small `want`
message (one-byte kind + u16 height) for the tile height. The spec should state both exceptions
explicitly.

### 5.4 Track message and encryption

Every track message starts with one clear byte:

```
flags: bit 0 = keyframe, other bits must be zero
```

Then:

- Direct stream: an 8-byte big-endian capture timestamp in microseconds, then the encoded frame.
  QUIC TLS protects the whole message; no SFrame layer is added.
- Relay broadcast: an SFrame header (RFC 9605 compact key id and counter), then the SFrame
  ciphertext of (8-byte timestamp + encoded frame).

Key id and counter move into the SFrame header; the 14-byte clear header goes away. The AAD is
channel varint + flags + SFrame header, so the relay cannot change the channel or the keyframe
flag unnoticed. The timestamp is inside the ciphertext. The relay sees only channel, flags and
the SFrame header.

The timestamp unit is microseconds, as today; the description states the timebase.

Audio packets use the same track message layout on channel 5; the plaintext is the Opus packet,
and the timestamp is the capture timestamp.

Plugin data uses the same layout on channel 6; the plaintext is the existing relay-data
envelope. The relay forwards channel 6 without looking inside and treats it as the lowest
priority.

### 5.5 Description document (proposal)

The description is one embedded Bovnar document, capped at 8 KiB, for example:

```bovnar
.v = <uint:8> 1;
.timebase = <uint:32,µ~s> 1;
.kind = "screen";                       # "screen" | "camera"
.id = "…";                              # camera session id, absent for screen
.tracks = [
    { .ch = <uint:8> 1; .kind = "video"; .codec = "av1"; .layer = <uint:8> 0;
      .width = <uint:16> 1920; .height = <uint:16> 1080; .rate = <float:32,Hz> 30.0;
      .bitrate = <uint:32,k~b/s> 0; },
    { .ch = <uint:8> 2; .kind = "video"; .codec = "av1"; .layer = <uint:8> 1;
      .width = <uint:16> 1920; .height = <uint:16> 1080; .rate = <float:32,Hz> 30.0;
      .bitrate = <uint:32,k~b/s> 0; },
    { .ch = <uint:8> 5; .kind = "audio"; .codec = "opus"; .layer = <uint:8> 0;
      .width = <uint:16> 0; .height = <uint:16> 0; .rate = <uint:32,Hz> 48000;
      .bitrate = <uint:32,k~b/s> 96; }
];
```

Every track object carries the same eight fields, because a Bovnar `DomDoc` array must hold one
struct shape; fields that do not apply to a track are zero (video `.bitrate` = 0; audio `.layer`,
`.width` and `.height` = 0). The parser requires the fields that belong to the track's kind,
validates their ranges, and ignores unknown or inapplicable fields, so a streaming parse still
accepts either shape. `width`, `height`, `codec` and `rate` change with the quality step; `ch`
and `layer` state where the frames for that track arrive. Unknown fields remain ignored, so a
later codec, HDR metadata or a second audio track needs no wire version bump. The camera id ties
a direct camera stream to the `on`/`cam-join` session, so a stale stream is refused.

Everything received is untrusted: check the document size before parsing, validate the channel
map, the codec and the track count, and reject anything that does not fit the profile.

### 5.6 Limits and validation

- Video message: at most 4 MiB + 64 bytes (today's `MAX_VIDEO_PAYLOAD`).
- Audio message: at most 4 KiB + 64 bytes (today's `MAX_AUDIO_PAYLOAD`).
- Plugin data message: at most 64 KiB + 64 bytes (today's `MAX_DATA_PAYLOAD`).
- Description: at most 8 KiB before parsing.
- Channels: 0–4095, as the crate's demux allows.
- A frame that arrives on the wrong channel, has unknown flag bits, or exceeds its cap is
  dropped with a log line that contains no media content.
- A direct stream that ends at a message boundary is a clean stop, not a truncated document.

### 5.7 Capability negotiation and fallback

The new format is used only when both ends and, for a relay broadcast, the relay support it.
Otherwise the old format is used; the fallback is a normal mode, not an error.

**New capabilities:**

- Mumcord hello feature `media-v2` (wire name; the exact name is fixed by the spec). The hello
  has room: 13 of at most 16 features are in use today.
- Direct `Watch` gains an optional `mv: 2`; it repeats the hello capability for the specific
  viewer and lets the sharer choose the stream format.
- mumpeer `/v1/status` gains `"media": 2`; mumbot's `relay` announcement carries `mv` so the
  client knows before it asks for a token.
- `bcast-req` carries `mv` (the publisher's chosen format); `bcast` and `bcast-tok` carry it so
  every subscriber binds with the same format.
- Relay `bind` gains an optional `"media": 2`. Old binds (no field) use the old format.
- A new mumpeer accepts both formats and advertises `media: 2`. An old mumpeer has no `mv`, so
  the client chooses the old format.

**Fallback matrix (proposal):**

| Sharer | Viewer | Relay | Screen share | Camera |
|---|---|---|---|---|
| 0.9.0 | 0.9.0 | new (`mv:2`) | new relay broadcast | new relay broadcast, or direct if the relay is unavailable |
| 0.9.0 | 0.9.0 | old/none | new direct | new direct |
| 0.9.0 | 0.8.x | old | old relay broadcast | old relay broadcast |
| 0.9.0 | 0.8.x | new | old direct copy if capacity, else "needs 0.9.0" | `needs` sent; "This camera needs Mumcord 0.9.0"; no old-format copy |
| 0.8.x | 0.9.0 | any | old direct/relay as today | old relay as today |
| 0.8.x | 0.8.x | any | old | old |

- The relay broadcast format is chosen by the publisher and fixed for the broadcast. A new
  relay does not mix formats inside one broadcast; a subscriber binds with the broadcast's
  format.
- An old viewer of a new-format screen-share broadcast gets a direct copy in the old format when
  the direct path has room (8 viewers, as today's direct screen share); otherwise the viewer sees
  "This share needs Mumcord 0.9.0". For camera this copy is not possible: 0.8.x has no direct
  camera receive path, so a 0.8.x viewer of a new-format camera receives `needs`, shows "This
  camera needs Mumcord 0.9.0" and no second relay broadcast is created for it (spec 08 FR-8.95,
  decision of 2026-11-10; the 0.8.0 receive path is P0.5).
- The old format stays in 0.9.0 for 0.8.x peers and old relays. Removing it is a later decision,
  once 0.8.x is no longer supported.

## 6. Decisions to settle in the spec addendum

| # | Question | Recommendation |
|---|---|---|
| D1 | Description on channel 0 or as a `.meta` preamble? | Channel 0 only. One mechanism for the first description, later changes and the relay cache. |
| D2 | Is the description encrypted on the relay path? | Yes, with the current share key; resend at every rekey; relay caches ciphertext opaquely. |
| D3 | Channel map and temporal-layer encoding | Fixed map of §5.2; layer = channel − 1. Measure the encoder's layer output first. |
| D4 | SFrame header and AAD | RFC 9605 header; AAD = channel + flags + SFrame header; timestamp inside the ciphertext. |
| D5 | Direct reverse keyframe request | Keep the one-byte `KEYFRAME_REQUEST`; add a small `want` message for direct camera. |
| D6 | Plugin relay data | Move to channel 6 in new-format broadcasts; keep the old header path for old peers/relays. |
| D7 | Late joiner | Relay caches the latest description and the prologue; publisher sends a keyframe; viewer gets keys first. |
| D8 | QUIC relay transport | One unidirectional stream per message, in order, as the old plan says. Revisit only if the measured per-message stream cost matters. |
| D9 | Capability names | `media-v2`, `mv`, `"media": 2`; fix them in the spec before code. |
| D10 | Fallback rule | Capability-based as §5.7; old format is retained and tested, not a best-effort guess. |
| D11 | Camera direct signalling | Reuse `mumcord.camera` `on`/`cam-join`; the sharer opens `STREAM_CAMERA` to the viewer; the description carries the camera session id. |
| D12 | Camera direct preference | Relay broadcast first when a new relay is available (scales to 25); direct when the relay is old, missing, or the user is in direct-only mode. |
| D13 | Direct camera limit | The screen-share direct limit (8 viewers); relay broadcasts keep the camera limit (25). The UI shows the path per viewer. |
| D14 | Old-format lifetime | Keep it through 0.9.x; remove only in a later milestone once 0.8.x is unsupported. |

D3 is the highest risk: if the encoder cannot route each frame to a layer channel cleanly (for
example because a keyframe is always layer 0 and higher layers depend on lower ones), the spec
may instead put the temporal layer into the flags byte or a short prefix and have the relay read
it. A small spike before the spec is frozen should settle this.

## 7. Work plan

Each task is one fresh agent with a self-contained brief; do not bundle tasks. Each task ends
with its own tests and a report; the next task starts from the merged branch.

### Phase 0 — prerequisites (no 0.9.0 code yet)

| ID | Repo | Task | Done when |
|---|---|---|---|
| P0.1 | Mumcord | Finish and release 0.8.0 after P0.5; create `milestone/0.9.0` from `main`. | `milestone/0.9.0` exists with CI green; v0.8.0 is tagged after P0.5 is merged. |
| P0.2 | Mumcord | Bump `bovnar` from 2.0.2 to 2.1.0 in the workspace, regenerate notices, refresh the plugin-SDK prebuilt wasm library (still 2.0.2). | `cargo build`, tests, clippy and the notices gate pass; the lock has the 2.1.0 checksum; notices name v2.1.0. |
| P0.3 | mumpeer | Spike: build `bovnar` 2.1.0 for the static musl targets with cargo-zigbuild. | A static binary builds and runs; if not, an upstream `codec-only` feature is requested before Phase 4. |
| P0.4 | — | Spike: map encoder output to temporal-layer channels (D3). | A short measurement or test shows every frame can be routed cleanly; the result is added to the spec notes. |
| P0.5 | Mumcord (`milestone/0.8.0`) | 0.8.0 forward-compatibility: read the `mumcord.camera` `needs` message (spec 08 §6.5), show "This camera needs Mumcord 0.9.0" on the tile, stop retrying until the camera's next `on`; text in `en` and all 24 locales. Ships in 0.8.0, before its release. | A 0.8.0 build shows the text after `needs` and does not retry; unit tests cover the parser, the tile state and the no-retry rule. |

P0.2 is a small dependency-update PR. It can start as soon as the 0.9.0 branch exists; it does
not need the spec addendum because it changes no behavior. P0.3 and P0.4 run in parallel. P0.5
runs on `milestone/0.8.0` because 0.8.0 is still unreleased; it is the only 0.9.0-related change
that ships in 0.8.0, and P0.1's release step completes only after P0.5 is merged.

### Phase 1 — spec addendum (the gate)

| ID | Task | Done when |
|---|---|---|
| S1 | Write the "media stream profile" addendum to specs 07 and 08: document shape, channel map, description, control messages, encryption, transports, limits, capability negotiation and fallback, camera direct path, old-format retention, refusal texts, acceptance criteria. Resolve D1–D14. | The addendum is reviewed with the product owner and committed on `milestone/0.9.0` with `/spec`. |
| S2 | Add the new acceptance criteria (AC-7.x / AC-8.x), including every fallback combination, old-client behavior, old-relay behavior and malformed input. | Every criterion is verifiable in the running client against a local server and a local mumpeer. |

The addendum is drafted now (2026-11-10) and committed once the 0.9.0 branch exists. No
implementation starts before S1/S2 are merged.

### Phase 2 — profile in `mumcord-p2p`

| ID | Task | Done when |
|---|---|---|
| W1 | New `media` module: channel constants, control kinds, description parser/writer, limits, version constants. | Unit tests for the description, unknown fields, oversized input and every limit. |
| W2 | `MediaWriter` / `MediaReader` over `bovnar::codec`: write the `.media = \x00` opening, messages, chunks, description updates and the terminator; read with arbitrary splits and borrowed payloads; clean EOF at a message boundary. | Round-trip, split at every byte, > 64 KiB, terminator, malformed streams, reset; zero allocations per message with the counting allocator. |
| W3 | Crypto profile: SFrame header, key id/counter, AAD (channel + flags + header), timestamp inside plaintext, rekey at keyframes, old-key retirement. | RFC 9605 vectors, the existing crypto tests adapted, rekey and late-key tests. |
| W4 | Conformance and fuzz: shared vectors with the C reference (through the crate), fuzz every decoder that reads from the network. | Vectors pass in both directions; fuzz targets run for a fixed budget in CI. |
| W5 | Benchmark regression: write, peek/skip/forward and read for 150 B, 30 KB and 400 KB, against today's header. | Figures recorded in the PR; no target is missed beyond the accepted 400 KB keyframe caveat. |
| W6 | Legacy module: keep the old header/wire types behind a `legacy` module with tests, used by the fallback selection. | The old format still round-trips and passes its existing tests; no new code uses it by default. |

W1–W3 can be one agent each; W2 depends on W1, W3 on W1, W4/W5 on W2+W3, W6 is independent.

### Phase 3 — direct `STREAM_SCREEN` path and camera direct path

| ID | Task | Done when |
|---|---|---|
| D1 | Replace `screen_share/wire.rs` framing with a format switch: new profile for `media-v2` peers, old v1/v2 for old peers; keep `Frame` / `FrameHeader` as the internal type; fill width, height, codec and timestamp from the description and flags. | Screen-share unit tests pass for both formats; old header code is still covered. |
| D2 | Sharer: write the description at stream start and on quality/encoder changes; route frames to layer channels; map direct keyframe requests; choose the viewer's format from `Watch` / hello. | A direct share works between 0.9.0 clients and with a 0.8.x viewer, at every quality step and codec. |
| D3 | Viewer: read the description before the first frame, apply changes at keyframes, request keyframes on the reverse stream, treat EOF at a message boundary as a clean stop; accept both formats. | Direct viewer tests, including a late frame, a mid-stream quality change and an old sharer. |
| D4 | Version negotiation and refusal texts: hello `media-v2`, `Watch` `mv`, no offer to a viewer that cannot be served, clear "needs 0.9.0" text when the direct fallback is full. | Old-client scenarios show the clear message and never a broken picture. |
| D5 | New direct camera path: `STREAM_CAMERA` kind, `mumcord.camera` signalling, per-viewer queue, direct viewer, `want` on the reverse stream, camera room. | Camera works with no relay, with an old relay and with two 0.9.0 clients; the camera room still works with multiple senders. |
| D6 | Camera relay/direct choice: relay first when a new relay is available, direct otherwise; the sender sends `needs` to a viewer whose hello lacks `media-v2`, so a 0.8.x viewer gets the "needs 0.9.0" text (no old-format copy). | Every cell of the fallback matrix for camera is tested; a real 0.8.0 build (P0.5) shows the text after `needs` and does not retry. |
| D7 | Watch-party UI regression: the shared media view still plays and resizes. | The existing screen-share/watch-party UI tests pass unchanged. |

D1–D3 are one vertical slice; D4–D6 follow. D5 is the largest new task and adds the `needs`
sender side. D6 depends on P0.5: its 0.8.x viewer cell needs a 0.8.0 build that understands
`needs`; without P0.5 the cell cannot pass and must be reported, not worked around.

### Phase 4 — relay broadcast and mumpeer

| ID | Task | Done when |
|---|---|---|
| R1 | `p2p/broadcast.rs`: add the profile types next to the legacy `Header` / `Unit` / `Content`; publisher and subscriber speak channel 0 and track channels on new links; key exchange and admission unchanged. | Publisher/subscriber unit tests pass for new and old links; no old header on a new link. |
| R2 | `p2p/relay.rs`: new links use QUIC one uni stream per message and TCP mux chunks inside `0x01` frames; old links keep the current framing; `bind` carries `media`. | TCP and QUIC transport tests pass for both formats, messages cut at every byte. |
| R3 | Engine `broadcast/link.rs`: seal/open track messages, description handling, late joiner, layer filter, `want`, plugin data channel, format selection. | Broadcast link tests pass; plugin relay messages still arrive in order and once. |
| R4 | `plugin_relay/broadcast.rs`: new links use channel 6; old links keep `Content::Data`. | Existing plugin relay tests pass for both formats; the relay never parses the payload. |
| R5 | mumpeer: add `bovnar`, peer/skip routing, channel-based layer filtering, chunk-boundary drops, description cache, `media` bind gate, `/v1/status` `media:2`, both formats. | mumpeer's live tests and `bcast-sim` pass for old and new clients; a benchmark shows the relay cost. |
| R6 | mumbot: pass `mv` through `relay`, `bcast-req`, `bcast`, `bcast-tok`; bump `mumcord-p2p`, no media parsing; live peer tests. | mumbot's tests pass against old and new clients and relays. |

R1–R4 are Mumcord; R5 is the separate mumpeer repo; R6 is the separate mumbot repo. R5 must
start before R3 can be tested end to end. The old-format paths are not removed.

### Phase 5 — polish, acceptance and release

| ID | Task | Done when |
|---|---|---|
| E1 | Packet inspector: decoded description, channel, flags, keyframe state and the detected format. | A captured share shows the description and every message's channel; docs updated. |
| E2 | UI texts and 24 locales for the fallback and old-relay messages; error paths. | No untranslated text; screen reader and keyboard checks. |
| E3 | Notices and licences for Bovnar 2.1.0; third-party notices regenerated. | The notices gate passes with the 2.1.0 provenance. |
| E4 | Acceptance run: all spec ACs against a local Mumble server, local mumpeer and local mumbot; every fallback combination; direct and relay; late join; layer switch; keyframe; rekey; TCP fallback; malformed, truncated and oversized input; old client served by fallback; a real 0.8.0 build (P0.5) shows the `needs` text and does not retry. | Every AC passes; dark and light screenshots for changed UI. |

E4 depends on P0.5 for the 0.8.x side of AC-8.31 (the `needs` text and the stopped retry); a
0.8.0 build without P0.5 cannot run that check.
| E5 | Release: Mumcord 0.9.0, mumpeer 0.3.0, mumbot patch; release notes explain the new format, the fallback and the direct camera path. | The three releases are tagged and the notes are published. |

## 8. Test plan

- **Unit:** description parsing, every channel and kind, flags, limits, malformed input, format
  selection.
- **Transport:** messages split at every byte, > 64 KiB, chunk boundaries, TCP `0x01` frames,
  QUIC one stream per message, clean close, old-format units.
- **Crypto:** RFC 9605 vectors, key id/counter, AAD tampering, rekey at a keyframe, a late
  joiner with the current key, an old key that cannot decrypt new frames.
- **Conformance:** the Rust output against the C reference in both directions through the crate.
- **Fuzzing:** every decoder that reads from the network (mux, description, control, crypto,
  legacy header).
- **Allocations:** counting allocator; zero per message once warm in the codec; no new per-frame
  allocation in the engine relative to 0.8.0.
- **Benchmark:** relay write/peek/skip/read for 150 B, 30 KB and 400 KB; record the result.
- **Fallback matrix:** every cell of §5.7, for screen share and camera, direct and relay, old and
  new.
- **Integration/E2E:** local server + mumpeer + mumbot; screen share, share sound, camera,
  camera room, direct and relay; late join, layer switch, keyframe request, rekey, TCP fallback;
  malformed/truncated/oversized input; old client and old relay served by fallback.
- **Manual:** Windows, macOS and Linux; camera and share sound; packet inspector; two themes;
  one non-English locale; screen reader.

## 9. Risks and mitigations

| Risk | Mitigation |
|---|---|
| Temporal layer → channel mapping does not fit the encoder output. | P0.4 spike before the spec is frozen; fallback is a layer byte in the flags/prefix read by the relay. |
| SFrame header integration mistakes (nonce reuse, wrong AAD). | Implement from RFC 9605, add the RFC vectors, review the crypto diff, keep the current sealer/opener tests. |
| mumpeer cannot build the C part of `bovnar` for static musl. | P0.3 spike; if it fails, request an upstream codec-only feature before Phase 4. |
| Two formats double the test matrix and keep old code alive. | A single format-selection point, legacy tests kept, a removal milestone recorded; the fallback matrix is explicit. |
| Direct camera fan-out costs upload per viewer. | Reuse the screen-share direct limit (8) and the existing queue policy; relay first when available; the UI shows the path. |
| Old viewers on a new relay get a worse path. | Screen share: direct old-format copy when capacity allows, otherwise a clear "needs 0.9.0" text. Camera (decided 2026-11-10): the sender sends `needs` and the 0.8.x viewer shows the clear "This camera needs Mumcord 0.9.0" text, because 0.8.x has no direct camera receive path; no old-format copy and no second relay broadcast. The receive path ships in 0.8.0 before its release (P0.5), so the text is available when 0.9.0 arrives. |
| Relay description cache grows or leaks. | Cap at 8 KiB per broadcast, one entry per broadcast, clear on broadcast end; tests. |
| Per-frame allocations in the engine. | Measure; reuse buffers where the existing pipeline already can; do not add copies in the new codec. |
| 0.8.0 slips and 0.9.0 cannot start. | The plan and the spec addendum are drafted now; code waits for `milestone/0.9.0`. |

## 10. Release and coordination

- mumpeer 0.3.0 and the mumbot patch add the new format and keep the old one. They can ship
  before Mumcord 0.9.0; old clients keep working.
- Mumcord 0.9.0 ships as part of the 0.9.0 milestone, not as a patch release.
- Release notes: 0.9.0 uses the new format with 0.9.0 peers and a new relay, and falls back to
  the old format otherwise; camera now works without a relay; a new relay is recommended for
  fan-out and the best camera path.
- The 0.9.0 milestone branch merges to `main` only when the whole milestone is done, per the
  branch rules.

## 11. Definition of done

- The spec addendum is committed and every new AC passes in the running client.
- 0.8.0 ships the `needs` receive path, tile state and 24 locales (P0.5) before its release; a
  real 0.8.0 build shows "This camera needs Mumcord 0.9.0" and does not retry.
- The new format is used when both peers and the relay support it; every fallback cell works.
- The old format still works with 0.8.x clients and old relays.
- The codec allocates nothing per message; the relay path shows no regression against 0.8.0.
- Conformance vectors, fuzzing and the crypto tests pass.
- Notices and provenance name Bovnar 2.1.0.
- Dark/light screenshots for changed UI; 24 locales; keyboard and screen reader.
- Mumcord 0.9.0, mumpeer 0.3.0 and mumbot are released together with the notes above.

## 12. Product-owner decisions (2026-11-10)

1. **Direct camera preference:** relay broadcast first when a new relay is available (scales to
   25 viewers); direct when the relay is old, missing, or the user is in direct-only mode.
2. **Old viewers on a new relay:** give them an old-format direct copy when the 8-viewer direct
   limit allows; otherwise show "This share needs Mumcord 0.9.0".
3. **Old-format lifetime:** keep it through 0.9.x and remove it in the milestone after 0.8.x is
   no longer supported.
4. **Camera fallback for 0.8.x viewers (follow-up, 2026-11-10):** a 0.8.x viewer of a new-format
   camera always sees "This camera needs Mumcord 0.9.0". 0.8.x has no direct camera receive path,
   so there is no old-format direct camera copy and no second relay broadcast for it. An old relay
   still serves 0.8.x viewers with the camera's old-format relay broadcast. Decision 2 stays as is
   for screen share, which 0.8.x can receive directly. The refusal is carried by `needs` (item 5).
5. **0.8.0 forward-compatibility (`needs`) (2026-11-10):** a 0.9.0 sender sends the
   `mumcord.camera` message `needs` (`.v = 1; .t = "needs"; .id = <16 lowercase hex camera
   session id>`, spec 08 §6.5) to a viewer whose hello lacks `media-v2` when the camera uses the
   new format and it cannot serve that viewer. 0.8.0 ships the receive path, the tile state and
   the text in all 24 locales before its release (P0.5): the tile shows "This camera needs
   Mumcord 0.9.0" and stops retrying until the camera's next `on`. A 0.7.x client ignores the
   unknown `needs` and keeps the "Couldn't join {name}'s camera" retry of FR-8.52. Reason: 0.8.x
   has no direct camera receive path, so 0.8.0 must know this refusal state before release.
