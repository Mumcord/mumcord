# mumcord-p2p

Direct encrypted connections between two [Mumcord](https://github.com/Mumcord/mumcord) clients, which speak the Mumble protocol and work with standard Mumble servers.

The transport is QUIC (quinn) over rustls with mutual TLS. Each side presents its Mumble client certificate and accepts the other side only if that certificate's SHA-1 hash equals the hash the Mumble server reports for that user, so the Mumble server acts as the trust anchor. Addresses come from the network interfaces (IPv6 and LAN) and a router port mapping (PCP, NAT-PMP, UPnP). There is no STUN, TURN or relay server by design: when no direct path works, the attempt ends with `P2pError::NoDirectPath`.

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

## Licence

AGPL-3.0-only.
