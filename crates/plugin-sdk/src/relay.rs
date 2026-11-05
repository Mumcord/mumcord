//! Plugin data over the relay (since 1.1) for traffic that is too large or too frequent for
//! [`crate::peers`]. Check [`status`] first; with `fallback` the host may use the Mumble server
//! when no relay is available.
//!
//! ```no_run
//! use mumcord_plugin_sdk::relay::{self, RelayStatus};
//!
//! # fn demo() -> Result<(), mumcord_plugin_sdk::JsonError> {
//! if relay::status() == RelayStatus::Available {
//!     relay::send_channel_json(&[1, 2, 3], true)?;
//! }
//! # Ok(())
//! # }
//! // Incoming data arrives as `Event::RelayMessage((peer, bytes))`; decode it with
//! // `relay::decode_json`.
//! ```

use serde::{Serialize, de::DeserializeOwned};

use crate::JsonError;
pub use crate::bindings::mumcord::plugin::relay::*;

/// Encodes `value` as JSON and sends it to the channel.
pub fn send_channel_json<T: Serialize + ?Sized>(
    value: &T,
    fallback: bool,
) -> Result<(), JsonError> {
    send_channel(&serde_json::to_vec(value)?, fallback)?;
    Ok(())
}

/// Encodes `value` as JSON and sends it to one peer.
pub fn send_direct_json<T: Serialize + ?Sized>(
    peer: Session,
    value: &T,
    fallback: bool,
) -> Result<(), JsonError> {
    send_direct(peer, &serde_json::to_vec(value)?, fallback)?;
    Ok(())
}

/// Decodes the bytes of an incoming relay message. The data is untrusted.
pub fn decode_json<T: DeserializeOwned>(data: &[u8]) -> Result<T, serde_json::Error> {
    serde_json::from_slice(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trips() {
        let bytes = serde_json::to_vec(&[1u8, 2]).expect("encodes");
        assert_eq!(decode_json::<Vec<u8>>(&bytes).expect("decodes"), vec![1, 2]);
        assert!(decode_json::<Vec<u8>>(b"nope").is_err());
    }
}
