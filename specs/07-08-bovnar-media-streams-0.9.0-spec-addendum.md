# Addendum — Bovnar media streams for screen share and camera (0.9.0)

> **Draft, 2026-11-10 — committed to `milestone/0.9.0`.** This addendum will be merged into
> `specs/07-file-sharing-screenshare-specification.md` and
> `specs/08-camera-sharing-specification.md` on `milestone/0.9.0`.
> FR and AC numbers are provisional until the 0.8.0 branch is merged; they start after the
> highest numbers on `main` (FR-7.88, FR-8.91, AC-7.71, AC-8.28).
>
> It supersedes the clean-break decision of 2026-11-04. The product owner decided on
> 2026-11-10 to keep the old format as a fallback and to add a direct camera path.

## 1. Context and scope

The media of screen share, share sound and camera currently uses a fixed binary header: 20/21
bytes on the direct `STREAM_SCREEN` stream and 14 bytes on a relay broadcast, with the relay
reading kind, codec, temporal layer, key id, sequence and size. Bovnar 2.1.0 now provides an
incremental, allocation-free mux codec (`bovnar::codec`) for an endless octet stream with
multiplexed channels and a stream description.

This addendum replaces the media framing with that profile when both ends and the relay support
it, keeps the old format as a fallback for 0.8.x peers and old relays, and adds a direct camera
path. Watch party media (the embedded web player) is not affected.

In scope: screen-share video and share sound, camera video, direct and relay paths, the
broadcast control messages, plugin data that shares the broadcast header, capability
negotiation, fallback, the packet inspector, and the refusal texts.

Out of scope: file transfer (already Bovnar, #673), plugin data to one person on the direct
`STREAM_PLUGIN_DATA` path, watch-party media, server-side changes, and removing the old format.

## 2. Decisions

| Date | Decision | Note |
|---|---|---|
| 2026-11-04 | Clean break from the old media header, no transition period. | **Superseded on 2026-11-10.** |
| 2026-11-10 | Keep the old media format as a capability-based fallback. | 0.9.0 uses the new profile only when the peer and the relay support it; otherwise the old format. |
| 2026-11-10 | Add a direct camera path. | Camera works without a relay and with an old relay, not only through a relay broadcast. |
| 2026-11-10 | Camera uses the relay broadcast first; direct is the fallback. | The relay scales to 25 viewers; direct is used with an old or missing relay, or in direct-only mode. |
| 2026-11-10 | Relay broadcast format is chosen by the publisher and fixed for the broadcast. | A new relay does not mix formats inside one broadcast. |

## 3. Functional requirements (spec 07)

### 3.1 The media document

**FR-7.89 — One endless Bovnar document per media link.** A new-format screen-share media link
carries one Bovnar document that never ends:

```bovnar
#!bovnar 1.2
.media = \x00 …messages… \x00;
```

A message is `varint(channel) varint(length) bytes`. Every message starts a new chunk; a message
above 64 KiB spans several chunks, each starting with its channel varint. The writer ends only
when the link closes. The same document shape is used on the direct stream, the relay over QUIC
and the relay over TCP.

**FR-7.90 — Channel map.** The channels are fixed:

| Channel | Use |
|---|---|
| 0 | Control and stream description |
| 1–4 | Video temporal layers 0–3 (channel = 1 + layer) |
| 5 | Audio (Opus) |
| 6 | Plugin relay data |
| 7–4095 | Reserved |

A viewer that wants layer ≤ N receives channels 1…N+1. The relay drops higher channels whole at
chunk boundaries; what it forwards is still a valid stream.

**FR-7.91 — Channel 0 control.** Every channel-0 message starts with a one-byte kind:

| Kind | Message | Direction | Payload |
|---|---|---|---|
| 0 | Description | Sharer → all | Embedded Bovnar document (§3.3) |
| 1 | `keyreq` | Subscriber → publisher | opaque chunk; the relay prepends the u16 slot |
| 2 | `key` | Publisher → one subscriber | u16 slot + opaque chunk; the relay strips the slot |
| 3 | `kf` | Subscriber → relay → publisher | empty |
| 4 | `layers` | Subscriber → relay | one byte, highest temporal layer |
| 5 | `want` | Subscriber → relay → publisher | u16 tile height; the relay prepends the slot |
| 6 | `gone` | Relay → publisher | u16 slot |

The payload layouts stay as they are today. The kind byte is clear so the relay can route
control messages and cache the latest description without parsing it.

**FR-7.92 — Track message.** Every track message starts with one clear flags byte (bit 0 =
keyframe, other bits zero). Then:

- Direct stream: an 8-byte big-endian capture timestamp in microseconds, then the encoded frame.
  QUIC TLS protects the whole message; no SFrame layer is added.
- Relay broadcast: an SFrame header (RFC 9605 compact key id and counter), then the SFrame
  ciphertext of (8-byte timestamp + encoded frame).

The AAD is channel varint + flags + SFrame header. The timestamp is inside the ciphertext. The
relay sees only channel, flags and the SFrame header. The timestamp unit is microseconds; the
description states the timebase.

**FR-7.93 — Plugin relay data.** Plugin data that uses a relay broadcast travels on channel 6
with the same track layout; the plaintext is the existing relay-data envelope. The relay
forwards channel 6 without looking inside and treats it as the lowest priority.

**FR-7.94 — Limits and untrusted input.** Video at most 4 MiB + 64 bytes; audio at most 4 KiB +
64 bytes; plugin data at most 64 KiB + 64 bytes; description at most 8 KiB before parsing;
channels 0–4095. A frame on the wrong channel, with unknown flag bits or over its cap is dropped
with a log line that contains no media content. A direct stream that ends at a message boundary
is a clean stop, not a truncated document.

### 3.2 Description

**FR-7.95 — Stream description.** The first channel-0 message is an embedded Bovnar document,
capped at 8 KiB, for example:

```bovnar
.v = <uint:8> 1;
.timebase = <uint:32,µ~s> 1;
.kind = "screen";
.tracks = [
    { .ch = <uint:8> 1; .kind = "video"; .codec = "av1"; .layer = <uint:8> 0;
      .width = <uint:16> 1920; .height = <uint:16> 1080; .rate = <float:32,Hz> 30.0; },
    { .ch = <uint:8> 5; .kind = "audio"; .codec = "opus";
      .rate = <uint:32,Hz> 48000; .bitrate = <uint:32,k~b/s> 96; }
];
```

It is sent again when the encoder changes resolution, codec, rate or track set, and at every
rekey. The viewer applies a new description only at a keyframe. Unknown fields are ignored.

On the direct stream the description is clear inside the QUIC TLS connection. On the relay path
it is encrypted with the current share key; the relay caches the latest kind-0 ciphertext
opaquely and replays it to a late joiner after the key exchange.

### 3.3 Capability negotiation and fallback

**FR-7.96 — Capability announcement.** 0.9.0 clients announce the hello feature `media-v2`.
`Watch` gains an optional `mv: 2`. The relay announcement gains `mv`; `bcast-req` gains `mv`;
`bcast` and `bcast-tok` carry it; relay `bind` gains an optional `"media": 2`; mumpeer
`/v1/status` gains `"media": 2`. A missing field means the old format.

**FR-7.97 — Fallback matrix.** The new format is used only when both ends and, for a relay
broadcast, the relay support it. Otherwise the old format is used:

| Sharer | Viewer | Relay | Screen share |
|---|---|---|---|
| 0.9.0 | 0.9.0 | new | new relay broadcast |
| 0.9.0 | 0.9.0 | old/none | new direct |
| 0.9.0 | 0.8.x | old | old relay broadcast |
| 0.9.0 | 0.8.x | new | old direct copy if capacity, else "needs 0.9.0" |
| 0.8.x | 0.9.0 | any | old direct/relay as today |
| 0.8.x | 0.8.x | any | old |

The relay broadcast format is chosen by the publisher and fixed for the broadcast. A new relay
accepts both formats and advertises `media: 2`; an old relay has no `mv`, so the client chooses
the old format.

**FR-7.98 — Old-format retention.** The old 14/21-byte format stays in 0.9.0 for 0.8.x peers and
old relays. It is tested and marked legacy; removing it is a later decision once 0.8.x is no
longer supported.

**FR-7.99 — Refusal texts.** A viewer that cannot be served says "This share needs Mumcord
0.9.0". An old relay says "This server's relay needs an update". A relay broadcast that is full
keeps the existing "This share is full".

### 3.4 Late joiners, rekey and the packet inspector

**FR-7.100 — Late joiner.** The relay caches the latest description ciphertext and the stream
prologue; after the key exchange it sends them to the late joiner and then frames from the next
keyframe. The publisher sends a keyframe on request.

**FR-7.101 — Packet inspector.** The inspector shows the detected format, the decoded
description, and for each message its channel, flags, keyframe state and size. It never logs
media content.

## 4. Functional requirements (spec 08)

**FR-8.92 — Direct camera path.** Camera media can travel on a new direct `STREAM_CAMERA` p2p
stream between two 0.9.0 clients. The viewer's `cam-join` carries its media capability; the
sharer opens one stream per admitted viewer after the join. The stream uses the media document
and description of FR-7.89–FR-7.95, with video channels only. The description carries the camera
session id; a stream with a stale id is refused.

**FR-8.93 — Direct camera control.** Keyframe requests use the reverse stream's existing
one-byte request. `want` (tile height) uses a small reverse-stream message (one-byte kind + u16
height) or the `mumcord.camera` signalling; the spec fixes one. The direct camera has the
screen-share direct viewer limit (8); relay broadcasts keep the camera limit (25).

**FR-8.94 — Camera path choice.** A relay broadcast is used first when a new relay is
available (it scales to 25 viewers). Direct camera is used when the relay is old, missing, or
the user is in direct-only mode. The viewer list shows the path per viewer.

**FR-8.95 — Camera fallback.** A 0.8.x viewer of a new-format camera broadcast gets an
old-format direct camera copy when the 8-viewer direct limit allows; otherwise it shows "This
camera needs Mumcord 0.9.0". A 0.9.0 viewer of an 0.8.x camera uses the old relay path as today.
The camera room may mix direct and relay viewers.

**FR-8.96 — Camera with no relay.** Camera turns on and is watchable between two 0.9.0 clients
with no relay and no bot, using the camera signalling and the direct path. All other camera
rules (admission, same channel, blocked users, viewer limit, `full`, rekey) stay as they are.

## 5. Protocol additions

| Message / place | Change |
|---|---|
| Mumcord hello | New feature `media-v2`. |
| `Watch` | Optional `mv` (2 = new media format). |
| Relay announcement (`mumcord.peer` `relay`) | Optional `mv` (2 = the relay speaks the new format). |
| `bcast-req` | Optional `mv` (the publisher's chosen format). |
| `bcast`, `bcast-tok` | Optional `mv` (the broadcast's format). |
| Relay `bind` | Optional `"media": 2`; absent = old format. A new mumpeer checks it against the broadcast. |
| mumpeer `/v1/status` | `"media": 2` when the relay speaks the new format. |
| Direct `STREAM_SCREEN` | New `STREAM_CAMERA` kind = 5. |
| Direct reverse stream | `KEYFRAME_REQUEST` = 1 stays; optional `WANT` = 2 + u16 height for camera. |
| `mumcord.camera` `on` | Optional `mv` (the sender's media capability) and `mode` (`relay` or `direct`). |
| `mumcord.camera` `cam-join` | Optional `mv` (the viewer's media capability). |
| Media document | As FR-7.89–FR-7.95. |
| Relay broadcast units | New links use mux messages; old links keep the 14-byte header. |

Everything is client-only; the Mumble server sees only the existing plugin data.

## 6. Acceptance criteria (provisional)

**New format**
1. **AC-7.72** Two 0.9.0 clients and a new mumpeer share a screen; the packet inspector shows
   one endless `.media = \x00` document, the description on channel 0 and each frame on its
   layer channel; the relay capture shows only channel, flags and SFrame header, never a
   timestamp, resolution or frame sequence.
2. **AC-7.73** Share sound arrives on channel 5 with the same sound as today; a late joiner gets
   the description and starts at the next keyframe.
3. **AC-7.74** Quality and codec changes send a new description and take effect at a keyframe;
   the viewer never decodes a frame with the wrong description.
4. **AC-7.75** A viewer that caps its frame rate gets only its layer channels; the other viewers
   keep the full rate; the relay log shows the dropped channels.
5. **AC-7.76** A viewer that leaves makes the sharer rekey; the old key cannot decrypt the new
   frames; the new description is sent with the new key.
6. **AC-7.77** Malformed, truncated and oversized documents, descriptions, messages and chunks
   are refused with no crash, no hang and no media content in the log; a direct stream that
   ends at a message boundary ends cleanly.

**Fallback**
7. **AC-7.78** A 0.9.0 sharer with a 0.8.x viewer and an old relay shares in the old format; the
   0.8.x viewer sees the share; the inspector shows the old header.
8. **AC-7.79** A 0.9.0 sharer with a 0.8.x viewer and a new relay: the old viewer gets an
   old-format direct copy while the direct limit allows, otherwise "This share needs Mumcord
   0.9.0"; two old viewers and 8 direct slots are checked.
9. **AC-7.80** A 0.9.0 sharer with an old relay and a 0.9.0 viewer falls back to a new-format
   direct share; the share never stops; the client shows the old-relay notice.
10. **AC-7.81** An old relay serves 0.8.x clients exactly as before; a new relay serves both
    0.8.x and 0.9.0 clients, with the format named in the bind.

**Camera**
11. **AC-8.29** Two 0.9.0 clients with no relay and no bot turn the camera on and watch each
    other through the direct path; the camera room shows the direct path.
12. **AC-8.30** With an old relay, two 0.9.0 clients watch each other's camera through the
    direct path; with a new relay, the camera uses the relay broadcast and scales to 25 viewers.
13. **AC-8.31** A 0.8.x viewer of a new-format camera gets an old-format direct camera copy
    while the direct limit allows, otherwise "This camera needs Mumcord 0.9.0".
14. **AC-8.32** A mixed camera room (direct and relay viewers, several senders) keeps every tile
    correct; leaving, blocking and `full` behave as today.
15. **AC-8.33** Camera `want` changes the tile height on the direct path within 1 s; keyframe
    requests and rekey work as on the relay path.

**Non-functional**
16. **AC-7.82** The codec allocates nothing per message once warm; the relay path shows no
    regression against 0.8.0; the benchmark figures are recorded.
17. **AC-7.83** Conformance vectors pass against the C reference in both directions; fuzzing of
    every network decoder runs clean for the CI budget.
18. **AC-7.84** Dark and light screenshots exist for every changed UI surface; all texts are in
    24 locales; keyboard and screen-reader checks pass.

## 7. Open points

1. **Temporal layer to channel mapping (D3).** A spike must confirm the encoder can route every
   frame to its layer channel. If not, the layer moves into the flags byte or a short prefix and
   the relay reads it.
2. **Direct camera preference (D12).** Relay first when a new relay is available; direct
   otherwise. Confirm before the spec is frozen.
3. **`want` on the direct camera path (D5/FR-8.93).** Reverse-stream message or `mumcord.camera`
   signalling; pick one in the spec.
4. **Old-format lifetime (D14).** Keep through 0.9.x; remove in a later milestone once 0.8.x is
   unsupported.
5. **FR/AC numbers.** Provisional; assign after the 0.8.0 branch is merged.
