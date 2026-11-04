//! A versioned Bovnar message, used to prove that the SDK's `bovnar` helpers work inside this
//! plugin's wasm build (spec 09 FR-9.93 to FR-9.95). The plugin has no peer-data permission, so
//! the document is encoded and read back locally instead of being sent.

use mumcord_plugin_sdk::bovnar::{self, MUMBLE_LIMIT, Message};
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ping {
    pub n: u32,
    pub label: String,
}

impl Message for Ping {
    fn version() -> u32 {
        1
    }
}

/// Encodes a ping, reads it back through the SDK and describes the result for the event log.
pub fn round_trip(n: u32) -> String {
    let ping = Ping {
        n,
        label: "ping".to_owned(),
    };
    let data = match bovnar::encode(&ping, MUMBLE_LIMIT) {
        Ok(data) => data,
        Err(e) => return format!("encode failed: {e}"),
    };
    match bovnar::on_document::<Ping>(&data) {
        Some(received) if received.message == ping => {
            format!("bovnar v{} {} B ok", received.version, data.len())
        }
        Some(_) => "bovnar changed in transit".to_owned(),
        None => "bovnar dropped".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ping_survives_encoding() {
        let ping = Ping {
            n: 3,
            label: "ping".to_owned(),
        };
        let data = bovnar::encode(&ping, MUMBLE_LIMIT).expect("encodes");
        assert_eq!(
            bovnar::decode::<Ping>(&data).expect("decodes").message,
            ping
        );
    }
}
