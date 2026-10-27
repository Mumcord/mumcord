# mumcord-store

The local persistence layer of [Mumcord](https://github.com/Mumcord/mumcord), a modern voice chat client for standard Mumble servers. It is useful for any Mumble client that needs to remember servers, trust decisions and the user's identity.

It provides a SQLite database (saved servers, trust decisions, followed channels, chat history, per-person and app settings), generation and PKCS#12 import and export of the Mumble client certificate (the user's identity), and a `SecretStore` on top of the OS secure storage (Secret Service, Keychain, Credential Manager). Secrets such as the identity key and server passwords only ever go to the OS secure storage; nothing secret is written to SQLite or plain files.

The `test-util` feature adds `SecretStore::in_memory()` for tests in other crates.

## Example

```rust
use mumcord_store::{Identity, NewServer, Store};

let store = Store::open(std::path::Path::new("/path/to/data-dir"))?;
let saved = store.add_server(&NewServer::new("Friends", "mumble.example.org", "alice"))?;
assert_eq!(store.servers()?.len(), 1);

let identity = Identity::generate("alice")?;
println!("{}: {}", saved.label, mumcord_store::cert_hash(identity.cert_der()));
# Ok::<(), mumcord_store::Error>(())
```

## Licence

AGPL-3.0-only.
