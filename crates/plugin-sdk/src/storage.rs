//! Key-value storage, plus helpers that store values as JSON.
//!
//! ```no_run
//! use mumcord_plugin_sdk::storage::{self, Scope};
//!
//! # fn demo() -> Result<(), mumcord_plugin_sdk::JsonError> {
//! let wins: u32 = storage::get_json(Scope::Identity, "wins")?.unwrap_or(0);
//! storage::set_json(Scope::Identity, "wins", &(wins + 1))?;
//! # Ok(())
//! # }
//! ```

use serde::{Serialize, de::DeserializeOwned};

use crate::JsonError;
pub use crate::bindings::mumcord::plugin::storage::*;

/// Reads `key` and decodes it from JSON. `Ok(None)` if the key does not exist.
pub fn get_json<T: DeserializeOwned>(scope: Scope, key: &str) -> Result<Option<T>, JsonError> {
    match get(scope, key)? {
        Some(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        None => Ok(None),
    }
}

/// Encodes `value` as JSON and stores it under `key`.
pub fn set_json<T: Serialize + ?Sized>(
    scope: Scope,
    key: &str,
    value: &T,
) -> Result<(), JsonError> {
    let bytes = serde_json::to_vec(value)?;
    set(scope, key, &bytes)?;
    Ok(())
}
