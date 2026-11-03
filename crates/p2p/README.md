# mumcord-p2p

Direct encrypted connections between two [Mumcord](https://github.com/Mumcord/mumcord) clients, which speak the Mumble protocol and work with standard Mumble servers.

The transport is QUIC (quinn) over rustls with mutual TLS. Each side presents its Mumble client certificate and accepts the other side only if that certificate's SHA-1 hash equals the hash the Mumble server reports for that user, so the Mumble server acts as the trust anchor. Addresses come from the network interfaces (IPv6 and LAN) and a router port mapping (PCP, NAT-PMP, UPnP). When no direct path works, the attempt ends with `P2pError::NoDirectPath`; the caller can then use an optional community relay that a relay bot announces on the Mumble server (`P2p::connect_via_relay`). The relay is reached over QUIC (UDP) or TLS over TCP, pinned to the announced certificate fingerprint, and only forwards the same end-to-end encrypted QUIC packets, so it cannot read or alter them. `PeerConnection::path` tells whether a connection is direct or relayed. Everything works without a relay.

Signalling messages (`Signal`) are encoded here and carried by the caller, for example as Mumble plugin data with the id `DATA_ID`. An offer or answer reveals this machine's addresses, so ask the user for consent before calling `offer` or `answer`.

## Example

```rust
use mumcord_p2p::{P2p, P2pConfig};

// `identity` is the user's `mumcord_protocol::ClientIdentity`; `peer_hash` is the
// certificate hash the Mumble server reports for the other user.
let p2p = P2p::new(&identity, P2pConfig::default());
let pending = p2p.offer(&peer_hash).await?;
// Send `pending.signal()` to the peer, wait for their answer signal, then:
let connection = pending.complete(&answer_signal).await?;
```

The answering side calls `P2p::answer` with the received offer and `PendingAnswer::connect`.

## Relay discovery and broadcasts (`mumcord.peer`)

`PeerMessage` encodes the plugin data messages between Mumcord clients and the relay bot (id `mumcord.peer`, compact JSON, at most 1000 bytes, unknown fields ignored). Besides `relay`, `token-req`, `token` and `token-err`, it covers:

- `relay` + `u` (array of `img`, `file`, `scr`, `plg`; missing = all allowed, unknown names are dropped), `bc` (most viewers per broadcast; missing = no broadcasts) and `pk` (most kilobits per second one sender may send as plugin data; missing or `0` = no limit announced).
- `token-req` + `use` (`img`, `file`, `scr`, `plg`; a bot treats a missing `use` as `scr`), `token-err` + `r = "use-not-allowed"`.
- Screen-share broadcasts: `bcast-req {v,rid,max,k}` (`k` is `scr` or `plg`; missing = `scr`, so bots from before plugin data keep working), `bcast {v,rid,id,tok,ttl}`, `bcast-admit {v,rid,id,viewer}`, `bcast-tok {v,id,tok,ttl,sharer}`, `bcast-kick {v,id,viewer}`, `bcast-err {v,rid,r}` with `r` one of `denied`, `not-same-channel`, `full`, `relay-down`, `relay-full`, `use-not-allowed`, `rate-limited`. `rid` and `id` are 16 lowercase hex digits, tokens 43 characters of unpadded base64url, sessions are `u32`.

## Broadcast wire format (module `broadcast`)

A broadcast link binds with `{"v":1,"t":"bind","tok":"…","role":"pub"|"sub"}` (a bind without `role` is a pair bind). Besides the pair error codes, the relay may refuse with `use_not_allowed` or `broadcast_full`.

After `ready`, every unit on the link starts with a 14-byte clear header; integers are big-endian:

| Offset | Size | Field                                                                                                                      |
| ------ | ---- | -------------------------------------------------------------------------------------------------------------------------- |
| 0      | 1    | Version, `1`                                                                                                               |
| 1      | 1    | Kind: `1` video, `2` audio, `3` control, `4` plugin data                                                                   |
| 2      | 1    | Flags: bit 0 keyframe (video only); all other bits zero                                                                    |
| 3      | 1    | Temporal layer `0`–`3` (video only; `0` otherwise)                                                                         |
| 4      | 1    | Video: codec `1` VP9, `2` AV1. Audio: `3` Opus. Control: type `1` keyreq, `2` key, `3` kf, `4` layers, `5` gone. Data: `0` |
| 5      | 1    | Key id of the share key (`0` for control)                                                                                  |
| 6      | 4    | Sequence: counter of the publisher's media units, video and audio share one counter (`0` for control)                      |
| 10     | 4    | Payload size in bytes                                                                                                      |

Payload sizes: video 1 to 4 MiB + 64, audio 1 to 4 KiB + 64, plugin data 1 to 64 KiB + 64 (media and plugin data payloads are end-to-end encrypted and opaque to the relay). Control payloads:

| Type     | Direction             | Payload                                                                                                                                 |
| -------- | --------------------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| `keyreq` | sub → relay → pub     | Subscriber sends an opaque body (≤ 1024 bytes); the relay prepends the subscriber's slot (u16), so the publisher receives `slot ‖ body` |
| `key`    | pub → relay → one sub | Publisher sends `slot ‖ body` (body ≤ 1024 bytes); the relay strips the slot and forwards the body to that subscriber                   |
| `kf`     | sub → relay → pub     | Empty; keyframe request, rate-limited by the relay                                                                                      |
| `layers` | sub → relay           | One byte: the highest temporal layer the subscriber wants                                                                               |
| `gone`   | relay → pub           | Slot (u16) of the subscriber that left                                                                                                  |

A receiver checks the whole header before reading the payload and drops the link on an unknown version, kind, codec or control type, flags or layer out of range, a control unit with a key id or sequence, or a size outside the range above.

Test vectors (header bytes in hex):

| Unit                                                    | Header                         |
| ------------------------------------------------------- | ------------------------------ |
| AV1 keyframe, layer 0, key id 2, sequence 1, 1024 bytes | `0101010002020000000100000400` |
| VP9, layer 2, key id 7, sequence 42, 200 bytes          | `0101000201070000002a000000c8` |
| Opus, key id 2, sequence 2, 92 bytes                    | `010200000302000000020000005c` |
| Plugin data, key id 3, sequence 7, 350 bytes            | `010400000003000000070000015e` |
| `keyreq`, 98 bytes                                      | `0103000001000000000000000062` |
| `key`, 100 bytes                                        | `0103000002000000000000000064` |
| `kf`                                                    | `0103000003000000000000000000` |
| `layers`                                                | `0103000004000000000000000001` |
| `gone`                                                  | `0103000005000000000000000002` |

## Licence

AGPL-3.0-only.
