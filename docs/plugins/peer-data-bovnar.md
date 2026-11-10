# Sending data to other players: Bovnar documents

Plugins that talk to other players' copies of themselves (a shared board, a game move, a presence
ping) send their data as **[Bovnar](https://github.com/sothis/bovnar) documents**. This page is for
plugin authors. It needs API 1.1 (`features = ["1.1"]` on the SDK, `"api": "^1.1"` in the manifest).

## Why Bovnar

- **Self-describing.** Every value carries its type, so a receiver can tell a number from a string
  and reject a document that does not have the shape it expects. There is no schema file to keep
  in sync between plugin versions.
- **Compact enough for chat-sized data.** A small document fits the 997-byte Mumble path.
- **One format for everyone.** All plugins use the same helpers, so the SDK validates every
  incoming document in the same way and your plugin never parses untrusted bytes by hand.

You are not forced to use it. A plugin can send raw bytes with `peers::send` or `relay::send_*`
(for example plugins on SDK 1.0, or plugins that want another format). Only Bovnar is documented,
supported and tested.

## The document shape

A message is any serde type that implements `bovnar::Message`. The SDK wraps it in a document with
two entries:

| Entry | Meaning                                                             |
| ----- | ------------------------------------------------------------------- |
| `v`   | The layout version of your message type, from `Message::version()`. |
| `msg` | Your message: a struct becomes a nested set of named values.        |

Numbers, strings, booleans, optional values, nested structs and lists work. Maps with non-string
keys do not. Nesting is limited to `bovnar::MAX_DEPTH` (32) levels in both directions.

```rust
use mumcord_plugin_sdk::bovnar::Message;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct Move {
    x: u32,
    y: u32,
    note: Option<String>,
}

impl Message for Move {
    fn version() -> u32 {
        1
    }
}
```

## The versioning rule

- **Adding a field is not a new version.** A receiver ignores fields it does not know, so a newer
  plugin can add fields and older copies keep working. Give an added field a default on the
  receiving side (`Option<T>` or `#[serde(default)]`), because messages from older senders do not
  have it.
- **Raise `version()` when the meaning of an existing field changes**, or a field is removed or
  changes type. `Received::version` tells the receiver which layout the sender used, so it can
  decode both or ignore the newer one:

```rust
use mumcord_plugin_sdk::bovnar::{self, Message};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct Move {
    x: u32,
    y: u32,
}

impl Message for Move {
    fn version() -> u32 {
        1
    }
}

fn on_payload(bytes: &[u8]) {
    let Some(received) = bovnar::on_document::<Move>(bytes) else {
        return; // already logged (reason and size) and dropped
    };
    if received.version > Move::version() {
        // A newer layout may mean something different by the same field names: skip it.
        return;
    }
    let _ = (received.message.x, received.message.y);
}
```

## Limits per path

The host only enforces sizes; it does not look inside.

| Path                                             | SDK call                                    | Largest document                   |
| ------------------------------------------------ | ------------------------------------------- | ---------------------------------- |
| Mumble plugin data                               | `Destination::Peers { to, kind }`           | 997 bytes (`bovnar::MUMBLE_LIMIT`) |
| Relay, channel or one person                     | `Destination::RelayChannel` / `RelayDirect` | 64 KiB (`bovnar::RELAY_LIMIT`)     |
| Relay with `fallback: true`, relay not available | the Mumble path                             | 997 bytes                          |

`send_document` refuses a document above the limit of its destination before anything leaves your
plugin (`Error::TooLarge`). With `fallback: true` the SDK checks the relay limit, but when the
relay is not available the host can only use the Mumble path: a document above 997 bytes then
comes back as `Error::Host(RelayUnavailable)`. Send small documents with `fallback: true` and big
ones without, and show the user something sensible when the relay is missing.

## Sending

```rust
use mumcord_plugin_sdk::{
    bovnar::{self, Destination, Message},
    log,
    peers::Target,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct Board {
    text: String,
}

impl Message for Board {
    fn version() -> u32 {
        1
    }
}

fn share(text: &str) {
    let board = Board {
        text: text.to_owned(),
    };

    // Small and for everyone in the channel: Mumble plugin data, kind 1.
    if let Err(e) = bovnar::send_document(
        Destination::Peers {
            to: Target::Channel,
            kind: 1,
        },
        &board,
    ) {
        log::warn(&format!("could not send the board: {e}"));
    }

    // Up to 64 KiB over the relay, falling back to Mumble when a small document has no relay.
    if let Err(e) = bovnar::send_document(Destination::RelayChannel { fallback: true }, &board) {
        log::warn(&format!("could not send the board: {e}"));
    }
}
```

## Receiving

Incoming bytes are untrusted. They arrive as `Event::PeerMessage` (Mumble path, with the sender's
`kind`) or `Event::RelayMessage` (relay, with the sender):

```rust
use mumcord_plugin_sdk::{bovnar, events::Event};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct Board {
    text: String,
}

impl bovnar::Message for Board {
    fn version() -> u32 {
        1
    }
}

fn on_event(event: Event) {
    let bytes = match event {
        Event::PeerMessage(message) if message.kind == 1 => message.payload,
        Event::RelayMessage((_peer, data)) => data,
        _ => return,
    };
    // A document that does not decode is logged by the SDK and dropped.
    if let Some(received) = bovnar::on_document::<Board>(&bytes) {
        let _ = received.message.text;
    }
}
```

## Error handling

`bovnar::Error` has four cases. `Error::reason()` gives the short name the SDK logs.

| Case        | Reason       | When                                                                                                        |
| ----------- | ------------ | ----------------------------------------------------------------------------------------------------------- |
| `Malformed` | `malformed`  | Not a Bovnar document, truncated, nested too deeply, or the value cannot be written as one.                 |
| `TooLarge`  | `too-large`  | Larger than the limit of its path.                                                                          |
| `WrongType` | `wrong-type` | A valid document that is not shaped like your message (missing `v` or `msg`, or a field of the wrong type). |
| `Host(e)`   | `host`       | The host refused to send it, for example `RelayUnavailable` or a rate limit.                                |

Use `bovnar::encode` and `bovnar::decode` directly when you want the error yourself instead of the
log line:

```rust
use mumcord_plugin_sdk::bovnar::{self, Error, Message};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct Hello {
    name: String,
}

impl Message for Hello {
    fn version() -> u32 {
        1
    }
}

fn check(bytes: &[u8]) -> &'static str {
    match bovnar::decode::<Hello>(bytes) {
        Ok(_) => "ok",
        Err(Error::TooLarge) => "too large",
        Err(Error::Malformed) => "not a document",
        Err(Error::WrongType) => "a document, but not a Hello",
        Err(Error::Host(_)) => "unreachable for decoding",
    }
}

fn encode_small(hello: &Hello) -> Result<Vec<u8>, Error> {
    bovnar::encode(hello, bovnar::MUMBLE_LIMIT)
}
```

A bad incoming document never stops your plugin: `on_document` returns `None` and your next
message is handled normally. The SDK logs the reason and the size only (for example `dropped a
Bovnar document: malformed (300 bytes)`), never the content.

## The host doesn't read your payload

The host passes your bytes through unchanged. It enforces the sizes and rates above, and nothing
else: it never parses, rewrites or logs a payload, and it does not know whether you used Bovnar. The
**SDK does the validation on the receiving side**, inside your plugin. That has two consequences:

- A plugin that skips the helpers and handles raw bytes must validate them itself.
- Do not put secrets into a document unless you protect them yourself. Anyone who can read the
  channel's plugin data can read the bytes.

## What the SDK bundles

Each SDK version pins exactly one Bovnar version (2.1.0 today); a new Bovnar version only arrives
with an SDK upgrade, so the format your plugin speaks does not change underneath you. For the
plugin target `wasm32-unknown-unknown` the SDK ships a prebuilt Bovnar library, so building a
plugin needs no C compiler, no Python and no wasi-sdk. (Building the SDK itself for the host, as its
own tests do, compiles Bovnar from the crates.io crate and does need a C compiler and Python 3.)

A complete plugin is `crates/plugin-sdk/examples/stage_board.rs`. The E2E test plugin
`e2e/fixtures/plugins/test-relay` sends 64 KiB and 900-byte documents and one deliberately
malformed payload.
