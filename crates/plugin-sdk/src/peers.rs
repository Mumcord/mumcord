//! Messages to the same plugin of other users in the channel, plus JSON helpers.
//!
//! ```no_run
//! use mumcord_plugin_sdk::{events::PeerMessage, peers::{self, Target}};
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Serialize, Deserialize)]
//! struct Vote { option: u8 }
//!
//! const VOTE: u8 = 1;
//!
//! # fn demo() -> Result<(), mumcord_plugin_sdk::JsonError> {
//! peers::send_json(Target::Channel, VOTE, &Vote { option: 2 })?;
//! # Ok(())
//! # }
//! fn on_message(message: &PeerMessage) {
//!     // Payloads are untrusted: a failed decode just drops the message.
//!     if message.kind == VOTE {
//!         if let Ok(vote) = peers::decode_json::<Vote>(message) {
//!             let _ = vote.option;
//!         }
//!     }
//! }
//! ```

use serde::{Serialize, de::DeserializeOwned};

pub use crate::bindings::mumcord::plugin::peers::*;
use crate::{JsonError, events::PeerMessage};

/// Encodes `value` as JSON and sends it. The encoded payload must fit the size limit of
/// [`send`]; otherwise the host answers `too-large`.
pub fn send_json<T: Serialize + ?Sized>(to: Target, kind: u8, value: &T) -> Result<(), JsonError> {
    let payload = serde_json::to_vec(value)?;
    send(to, kind, &payload)?;
    Ok(())
}

/// Decodes the payload of an incoming message from JSON.
pub fn decode_json<T: DeserializeOwned>(message: &PeerMessage) -> Result<T, serde_json::Error> {
    serde_json::from_slice(&message.payload)
}
