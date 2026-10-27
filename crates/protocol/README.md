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

## Licence

AGPL-3.0-only. The vendored Mumble `.proto` files keep their original BSD licence (`proto/LICENSE.mumble`).
