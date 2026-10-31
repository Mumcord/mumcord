# mumcord-protocol

The Mumble protocol layer of [Mumcord](https://github.com/Mumcord/mumcord), a modern voice chat client. It speaks the standard Mumble protocol and works with unmodified Mumble servers (murmur / mumble-server).

It covers the TLS control channel with the protobuf messages, server state tracking, and the UDP voice transport with its OCB2-AES128 crypto and TCP tunnel fallback. Audio capture, playback and codecs are not part of this crate.

## Example

```rust,no_run
use std::sync::Arc;
use std::time::Duration;
use mumcord_protocol::{ConnectConfig, Event, connect};

#[tokio::main]
async fn main() {
    let config = ConnectConfig {
        host: "localhost".into(),
        port: 64738,
        username: "bot".into(),
        password: None,
        tokens: vec![],
        identity: None,
        // Local test server only: a real client pins or asks the user about the certificate.
        verify_cert: Arc::new(|_cert| true),
        login_timeout: Duration::from_secs(10),
    };
    let mut connection = connect(config).await.expect("connect");
    while let Some(event) = connection.events.recv().await {
        if let Event::Disconnected(_) = event {
            break;
        }
    }
}
```

## Querying a channel's ACL

`Handle::query_acl(channel_id)` asks the server for a channel's groups and ACL entries. The answer arrives as `Event::Acl(Box<proto::Acl>)`, passed through as the server sent it (groups with members and inherited flags, ACL entries, `inherit_acls`). The server only answers users with the Write permission in that channel; everyone else gets `Event::PermissionDenied` with `kind == DenyKind::Permission`, the `channel_id` and `permission == permissions::WRITE`. Names in the reply come from the server and are untrusted. Added in a minor release; nothing existing changed.

## Rate limits and disconnecting

Standard servers drop messages that arrive faster than their rate limit allows, so the client paces chat, own user state, channel, ACL and plugin data messages: a short burst goes out at once, the rest waits in a queue. `Handle::disconnect` sends what is still queued first, at the allowed pace, and closes once the server has confirmed it (a ping answered after the last message), or after 2 s at the latest; anything left over is logged by message kind and count. If the connection is lost instead, the queue is dropped.

- `Handle::flush().await` waits until everything queued so far has been sent and processed by the server, without closing.
- `Handle::closed().await` waits until the connection has stopped sending, e.g. to delay exiting the process after `disconnect`.

## Licence

AGPL-3.0-only. The vendored Mumble `.proto` files keep their original BSD licence (`proto/LICENSE.mumble`).
