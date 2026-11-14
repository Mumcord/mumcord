//! "Send to your phone" (spec 05, FR-5.64 … FR-5.67): the format that carries identities and
//! optionally the server list from the desktop app to the phone app inside QR codes.
//!
//! Nothing here touches a network. The desktop builds a [`Transfer`]: a list of text parts,
//! each shown as one QR code, and a numeric code the user types on the phone. The phone feeds
//! every scanned text into a [`TransferReader`], in any order, and opens it with the typed code.
//!
//! # What is protected, and what is not
//!
//! The 8-digit code has about 27 bits. Someone who photographs all QR codes but not the number
//! tries all 10^8 codes in roughly three to four weeks on one desktop PC, or within a day on a
//! hundred rented machines, and then holds the identities for good. (With 6 digits, the first
//! draft of the spec, it would be 5–10 hours on one PC, which is why the default was raised.)
//! The three-attempt limit and the expiry do not bind such an attacker; the real protection is
//! that the codes and the number are only on the user's own screen for at most 2 minutes.
//!
//! So the slow key derivation only buys time against a camera that saw the QR codes and missed
//! the number. Nothing that travels inside the QR codes can add secrecy, because that attacker
//! sees it too; only a longer typed code raises the cost (each extra digit tenfold), which is
//! why the code length is a parameter of the writer ([`TransferOptions::code_digits`]).
//!
//! # Format (version 1)
//!
//! 1. **Payload** ([`TransferPayload`]): a hand-written, length-prefixed binary encoding with a
//!    version byte. No general-purpose serialiser, so every field has a hard limit on decode and
//!    the layout is easy to check against the test vectors in `vectors/`. An identity travels as
//!    its DER certificate and PKCS#8 key, the form the store validates on import; a server as
//!    name, address, port and user name. There is no field for a server password or a channel
//!    access token, so they cannot be transferred by accident (FR-5.66).
//! 2. **Compression**: raw DEFLATE before encryption. Compressing secrets is only dangerous
//!    when an attacker can mix chosen text into the same message and watch the length over many
//!    tries; here the sender writes the whole message once and nobody can ask for another, so
//!    only the rough amount of data shows. Keys are random, so the gain is modest (the server
//!    list and the repeated key modulus).
//! 3. **Key derivation**: Argon2id over the typed code with a random 16-byte salt per transfer
//!    ([`KdfParams`], default 128 MiB, 4 passes, 1 lane). It yields the 32-byte key and the
//!    12-byte nonce, both used for exactly one message.
//! 4. **Encryption**: ChaCha20-Poly1305 (ring). The whole 44-byte header is the associated
//!    data: magic, version, transfer id, salt, key derivation parameters, code length, creation
//!    time, part count and ciphertext length. Parts of two transfers cannot be combined and no
//!    parameter can be changed without the decryption failing.
//! 5. **Parts**: header and ciphertext are cut into chunks. Each part is
//!    `'M' | version | transfer id (4) | index | total | chunk | CRC-32 (4, little endian)`,
//!    written as Base45 text (RFC 9285). Text makes the QR code use its alphanumeric mode, which
//!    every scanner hands back as a plain string; binary QR content is mangled by several
//!    scanner libraries. The default part of 400 bytes is 600 characters, a version 15 QR code
//!    with error correction M (77 × 77 modules), which a phone reads reliably from a screen.
//!
//! # Validity
//!
//! The creation time is inside the authenticated header. The reader refuses a transfer older
//! than [`VALIDITY_SECS`] plus [`CLOCK_SKEW_SECS`], or created more than the skew in the future
//! (the two clocks are not synchronised with each other). This is a rule for the honest phone
//! only, an attacker sets their own clock. The desktop dialog therefore has to take the codes
//! off the screen when [`Transfer::seconds_left`] reaches zero and create a new transfer, which
//! has a new salt, id and code.

mod base45;
mod envelope;
mod import;
mod parts;
mod payload;
mod reader;
mod writer;

use thiserror::Error;

pub use envelope::KdfParams;
pub use import::{ImportReport, SkipReason, SkippedIdentity, SkippedServer};
pub use parts::MAX_PART_BYTES;
pub use payload::{
    MAX_CERT_BYTES, MAX_IDENTITIES, MAX_KEY_BYTES, MAX_PLAINTEXT_BYTES, MAX_SERVERS,
    MAX_STRING_BYTES, TransferIdentity, TransferPayload, TransferServer,
};
pub use reader::{Progress, TransferReader};
pub use writer::{DEFAULT_PART_BYTES, MIN_PART_BYTES, Transfer, TransferOptions, unix_now};

/// How long a transfer may be shown, in seconds (FR-5.64).
pub const VALIDITY_SECS: u64 = 120;

/// Allowance for the difference between the clock of the desktop and of the phone, in seconds.
/// Both normally follow network time within a second or two; a larger allowance would stretch
/// the two minutes of FR-5.64 noticeably.
pub const CLOCK_SKEW_SECS: u64 = 30;

/// Wrong codes after which the scanned parts are discarded (FR-5.65).
pub const MAX_ATTEMPTS: u8 = 3;

/// Shortest and longest typed code a reader accepts.
pub const MIN_CODE_DIGITS: u8 = 6;
pub const MAX_CODE_DIGITS: u8 = 10;
/// The code length of the spec (FR-5.64); the apps show it as two groups of four.
pub const DEFAULT_CODE_DIGITS: u8 = 8;

/// Most parts one transfer can have (the index is one byte).
pub const MAX_PARTS: usize = 255;

/// Why a transfer could not be created or read. No variant carries content of the transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum TransferError {
    /// Writer: the selection does not fit into [`MAX_PARTS`] QR codes or exceeds a size limit.
    #[error("the selection is too large for a transfer")]
    PayloadTooLarge,
    /// Writer: a field cannot be encoded (the text says which rule).
    #[error("this cannot be transferred: {0}")]
    InvalidPayload(&'static str),
    /// Writer: an option is outside its allowed range.
    #[error("invalid transfer option: {0}")]
    InvalidOptions(&'static str),
    /// The random generator of the operating system failed.
    #[error("the system's random generator failed")]
    Random,
    /// The key derivation could not get its working memory.
    #[error("not enough memory to derive the transfer key")]
    OutOfMemory,
    /// The scanned text is not a part of a Mumcord transfer.
    #[error("this QR code is not part of a Mumcord transfer")]
    MalformedPart,
    /// The scanned part is damaged; scanning it again helps.
    #[error("this QR code was not read correctly")]
    ChecksumMismatch,
    /// The part belongs to another transfer than the parts collected so far (for example the
    /// desktop created a new one). [`TransferReader::reset`] starts over.
    #[error("this QR code belongs to another transfer")]
    OtherTransfer,
    /// The transfer was made by a newer version of the format.
    #[error("this transfer needs a newer version of the app")]
    UnsupportedVersion,
    /// Older than two minutes (plus the clock allowance). The collected parts are discarded.
    #[error("the transfer has expired; create a new one on the computer")]
    Expired,
    /// Created in the future by more than the clock allowance: one of the two clocks is wrong.
    /// The collected parts are discarded.
    #[error("the clocks of the computer and the phone differ too much; check date and time")]
    ClockSkew,
    /// [`TransferReader::open`] before all parts were scanned.
    #[error("not all QR codes have been scanned yet")]
    Incomplete,
    /// The typed code does not have `digits` digits. Not counted as an attempt.
    #[error("the code has {digits} digits")]
    InvalidCode { digits: u8 },
    /// The code is wrong; `attempts_left` more tries before the scan must be repeated.
    #[error("wrong code ({attempts_left} attempts left)")]
    WrongCode { attempts_left: u8 },
    /// The code was wrong [`MAX_ATTEMPTS`] times; the collected parts are discarded.
    #[error("wrong code; scan the QR codes again")]
    RescanRequired,
    /// All parts were read, but their content is not a valid transfer. The collected parts are
    /// discarded.
    #[error("the transfer is damaged; create a new one on the computer")]
    Malformed,
}
