//! File sharing core (spec 07, FR-7.12 – FR-7.24, section 6.4): pure functions and types for the
//! offer message, the `mumcord.file` plugin data, file-name sanitizing and the risky-type list.
//! Everything here comes from or goes to other clients, so decoding validates every field.

mod motw;
pub mod offer;
pub mod prepare;
pub mod queue;
pub mod receive;
pub mod risky;
pub mod sanitize;
pub mod share;
pub mod wire;

/// Hello feature a client announces when it understands file offers (section 6.4).
pub const FEATURE: &str = "files";

/// Largest single file or zipped folder: 2 GiB (FR-7.12).
pub const MAX_FILE_SIZE: u64 = 2 * 1024 * 1024 * 1024;

/// Whether `id` is a share id: 32 lowercase hex characters.
pub(crate) fn is_share_id(id: &str) -> bool {
    id.len() == 32 && is_lower_hex(id)
}

pub(crate) fn is_lower_hex(s: &str) -> bool {
    s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}
