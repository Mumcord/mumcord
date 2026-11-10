//! Typed plugin peer data as Bovnar documents (since 1.1, spec FR-9.93 to FR-9.95).
//!
//! Bovnar is a self-describing format: every value carries its type. A message is any serde
//! type that implements [`Message`]; it travels as a document with two entries, the message
//! layout version `v` and the message itself `msg`. Decoding ignores fields the receiver does
//! not know, so a newer sender can add fields without breaking older receivers.
//!
//! The host passes payloads through untouched and only enforces sizes: at most
//! [`MUMBLE_LIMIT`] bytes on the Mumble path ([`crate::peers`] and the relay fallback) and
//! [`RELAY_LIMIT`] bytes over the relay. [`encode`] refuses a document above the limit before
//! anything is sent, and [`decode`] checks every incoming document: the bytes are untrusted.
//!
//! ```no_run
//! use mumcord_plugin_sdk::{bovnar::{self, Destination, Message}, peers::Target};
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Serialize, Deserialize)]
//! struct Move { x: u32, y: u32 }
//!
//! impl Message for Move {
//!     fn version() -> u32 { 1 }
//! }
//!
//! # fn demo() -> Result<(), bovnar::Error> {
//! bovnar::send_document(Destination::Peers { to: Target::Channel, kind: 1 }, &Move { x: 2, y: 3 })?;
//! # Ok(())
//! # }
//! fn on_payload(bytes: &[u8]) {
//!     // A bad document is logged (reason and size only) and dropped.
//!     if let Some(received) = bovnar::on_document::<Move>(bytes) {
//!         let _ = (received.version, received.message.x);
//!     }
//! }
//! ```
//!
//! The SDK pins exactly one Bovnar version (2.1.0) and, for the plugin target
//! `wasm32-unknown-unknown`, links a prebuilt Bovnar library bundled with the SDK, so building
//! a plugin needs no C compiler. A new Bovnar version only arrives with an SDK upgrade.

mod ffi;
mod libc;

use std::ffi::{CStr, c_void};
use std::fmt;
use std::ptr::NonNull;

use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Map, Number, Value};

use crate::{log, peers, relay};

// Natively the C library comes from the pinned `bovnar` crate, which compiles the same sources.
#[cfg(not(target_arch = "wasm32"))]
use bovnar as _;

/// The largest document on the Mumble path: [`crate::peers`] and the relay fallback.
pub const MUMBLE_LIMIT: usize = 997;
/// The largest document over the relay.
pub const RELAY_LIMIT: usize = 64 * 1024;
/// The deepest nesting of structs and lists a message may use, in both directions.
pub const MAX_DEPTH: usize = 32;

/// A message type a plugin sends as a Bovnar document.
pub trait Message: Serialize + DeserializeOwned {
    /// The layout version of this message type. Raise it when the meaning of existing fields
    /// changes; receivers see the sender's version in [`Received::version`].
    fn version() -> u32;
}

/// Why a document could not be encoded, decoded or sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The bytes are not a valid Bovnar document, are truncated, nest deeper than
    /// [`MAX_DEPTH`], or the value cannot be written as one.
    Malformed,
    /// The document is larger than the limit of its path.
    TooLarge,
    /// The document is valid but does not have the shape of the expected message.
    WrongType,
    /// The host refused to send the document.
    Host(crate::types::Error),
}

impl Error {
    /// The reason as the spec names it: `malformed`, `too-large`, `wrong-type` or `host`.
    pub fn reason(&self) -> &'static str {
        match self {
            Self::Malformed => "malformed",
            Self::TooLarge => "too-large",
            Self::WrongType => "wrong-type",
            Self::Host(_) => "host",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Host(e) => write!(f, "host error: {e:?}"),
            _ => f.write_str(self.reason()),
        }
    }
}

impl std::error::Error for Error {}

impl From<crate::types::Error> for Error {
    fn from(e: crate::types::Error) -> Self {
        Self::Host(e)
    }
}

/// A decoded message with the layout version its sender used.
#[derive(Debug, Clone, PartialEq)]
pub struct Received<M> {
    pub version: u32,
    pub message: M,
}

/// Where [`send_document`] sends a document.
#[derive(Debug, Clone, Copy)]
pub enum Destination {
    /// Mumble plugin data via [`crate::peers::send`]; at most [`MUMBLE_LIMIT`] bytes.
    Peers { to: peers::Target, kind: u8 },
    /// The relay, to the whole channel; at most [`RELAY_LIMIT`] bytes. With `fallback` the host
    /// may use the Mumble path when no relay is available, where [`MUMBLE_LIMIT`] applies.
    RelayChannel { fallback: bool },
    /// The relay, to one peer; limits as for `RelayChannel`.
    RelayDirect {
        peer: relay::Session,
        fallback: bool,
    },
}

impl Destination {
    /// The size limit [`send_document`] enforces before sending.
    pub fn limit(self) -> usize {
        match self {
            Self::Peers { .. } => MUMBLE_LIMIT,
            Self::RelayChannel { .. } | Self::RelayDirect { .. } => RELAY_LIMIT,
        }
    }
}

/// Encodes `message` and sends it. Nothing leaves the plugin when encoding fails or the
/// document is larger than the limit of `to`.
pub fn send_document<M: Message>(to: Destination, message: &M) -> Result<(), Error> {
    let bytes = encode(message, to.limit())?;
    match to {
        Destination::Peers { to, kind } => peers::send(to, kind, &bytes)?,
        Destination::RelayChannel { fallback } => relay::send_channel(&bytes, fallback)?,
        Destination::RelayDirect { peer, fallback } => relay::send_direct(peer, &bytes, fallback)?,
    }
    Ok(())
}

/// Decodes an incoming payload (from a `PeerMessage` or a relay message). A document that
/// does not decode is logged with the reason and its size, never its content, and dropped.
pub fn on_document<M: Message>(bytes: &[u8]) -> Option<Received<M>> {
    match decode(bytes) {
        Ok(received) => Some(received),
        Err(e) => {
            log::warn(&format!(
                "dropped a Bovnar document: {} ({} bytes)",
                e.reason(),
                bytes.len()
            ));
            None
        }
    }
}

/// Encodes `message` as a Bovnar document of at most `limit` bytes.
pub fn encode<M: Message>(message: &M, limit: usize) -> Result<Vec<u8>, Error> {
    let value = serde_json::to_value(message).map_err(|_| Error::Malformed)?;
    let mut w = Writer::new(limit)?;
    w.assignment("v")?;
    w.scalar(ffi::TOKEN_NUMBER, &M::version().to_string())?;
    w.entry("msg", &value, 0)?;
    let bytes = w.finish()?;
    // The writer accepts some shapes the reader refuses (lists of rows with different lengths,
    // lists mixing types). Reading the document back guarantees receivers accept what we send.
    Doc::parse(&bytes)?;
    Ok(bytes)
}

/// Decodes a Bovnar document into `M`. Unknown fields are ignored.
pub fn decode<M: Message>(bytes: &[u8]) -> Result<Received<M>, Error> {
    if bytes.len() > RELAY_LIMIT {
        return Err(Error::TooLarge);
    }
    let doc = Doc::parse(bytes)?;
    let (mut version, mut message) = (None, None);
    for (key, node) in doc.entries() {
        match key {
            b"v" => version = Some(node_value(node, 0)?),
            b"msg" => message = Some(node_value(node, 0)?),
            _ => {}
        }
    }
    let version = version
        .and_then(|v| v.as_u64())
        .and_then(|v| u32::try_from(v).ok())
        .ok_or(Error::WrongType)?;
    let message =
        serde_json::from_value(message.ok_or(Error::WrongType)?).map_err(|_| Error::WrongType)?;
    Ok(Received { version, message })
}

/// A Bovnar writer into a buffer of `limit` bytes. Running out of room is [`Error::TooLarge`].
struct Writer {
    handle: NonNull<c_void>,
    // The library writes into this buffer, so it lives (unmoved) as long as the writer.
    buf: Vec<u8>,
}

impl Writer {
    fn new(limit: usize) -> Result<Self, Error> {
        // SAFETY: plain constructor; null means out of memory.
        let handle = NonNull::new(unsafe { ffi::bvnr_writer_create() }).ok_or(Error::Malformed)?;
        let mut w = Self {
            handle,
            buf: vec![0; limit],
        };
        let mut flags = ffi::WriteFlags::zeroed();
        // One level for `msg` itself on top of the message's own nesting.
        flags.max_struct_nesting = (MAX_DEPTH + 1) as u8;
        flags.max_array_nesting = (MAX_DEPTH + 1) as u8;
        // SAFETY: the writer is live, `buf` holds `limit` bytes for the writer's lifetime and
        // `flags` is only read during the call.
        let ok = unsafe {
            ffi::bvnr_open_write_mem(
                w.handle.as_ptr(),
                w.buf.as_mut_ptr().cast(),
                limit as u64,
                false,
                &mut flags,
            )
        };
        if ok { Ok(w) } else { Err(Error::Malformed) }
    }

    fn event(&mut self, event: i32, token: i32, text: &str) -> Result<(), Error> {
        // A token as long as the buffer can never fit with its framing. The library refuses
        // tokens of 64 KiB and more with a format error, which would read as `malformed`.
        if text.len() >= self.buf.len() {
            return Err(Error::TooLarge);
        }
        let mut d = ffi::Data::zeroed();
        d.token_type = token;
        d.data = text.as_ptr().cast();
        d.length = u32::try_from(text.len()).map_err(|_| Error::TooLarge)?;
        // SAFETY: the writer is live; `d` and `text` are valid for the call and the library
        // copies what it keeps.
        if unsafe { ffi::bvnr_write_event(self.handle.as_ptr(), event, &mut d) } {
            Ok(())
        } else {
            Err(self.failure())
        }
    }

    /// A refused event: a full buffer means the document is too large, anything else that the
    /// value cannot be written.
    fn failure(&self) -> Error {
        // SAFETY: the writer is live.
        match unsafe { ffi::bvnr_writer_get_error(self.handle.as_ptr()) } {
            ffi::ERROR_SINK_BUFFER_EXHAUSTED => Error::TooLarge,
            _ => Error::Malformed,
        }
    }

    fn assignment(&mut self, key: &str) -> Result<(), Error> {
        if !key_is_valid(key) {
            return Err(Error::Malformed);
        }
        self.event(ffi::EV_ASSIGNMENT_START, ffi::TOKEN_IDENTIFIER, key)
    }

    fn scalar(&mut self, token: i32, text: &str) -> Result<(), Error> {
        self.event(ffi::EV_DATA, token, text)
    }

    /// `.key = value;`
    fn entry(&mut self, key: &str, value: &Value, depth: usize) -> Result<(), Error> {
        self.assignment(key)?;
        match value {
            Value::Array(items) => self.array(items, depth + 1),
            Value::Object(members) => self.object(members, depth + 1),
            scalar => self.literal(scalar, false),
        }
    }

    fn object(&mut self, members: &Map<String, Value>, depth: usize) -> Result<(), Error> {
        if depth > MAX_DEPTH {
            return Err(Error::Malformed);
        }
        self.event(ffi::EV_STRUCT_START, ffi::TOKEN_IDENTIFIER, "")?;
        for (key, value) in members {
            self.entry(key, value, depth)?;
        }
        self.event(ffi::EV_STRUCT_END, ffi::TOKEN_IDENTIFIER, "")
    }

    /// A list is one bracket row; nested lists are nested rows.
    fn array(&mut self, items: &[Value], depth: usize) -> Result<(), Error> {
        if depth > MAX_DEPTH {
            return Err(Error::Malformed);
        }
        self.event(ffi::EV_ARRAY_ROW_START, ffi::TOKEN_IDENTIFIER, "")?;
        for item in items {
            match item {
                Value::Array(inner) => self.array(inner, depth + 1)?,
                Value::Object(members) => self.object(members, depth + 1)?,
                scalar => self.literal(scalar, true)?,
            }
        }
        self.event(ffi::EV_ARRAY_ROW_END, ffi::TOKEN_IDENTIFIER, "")
    }

    fn literal(&mut self, value: &Value, in_array: bool) -> Result<(), Error> {
        let (number, string) = if in_array {
            (ffi::TOKEN_ARRAY_NUMBER, ffi::TOKEN_ARRAY_STRING)
        } else {
            (ffi::TOKEN_NUMBER, ffi::TOKEN_STRING)
        };
        match value {
            Value::Null => self.scalar(ffi::TOKEN_NULL_VALUE, ""),
            Value::Bool(b) => self.scalar(ffi::TOKEN_BOOL, if *b { "true" } else { "false" }),
            Value::Number(n) => match (n.as_u64(), n.as_i64(), n.as_f64()) {
                (Some(u), _, _) => self.scalar(number, &u.to_string()),
                (_, Some(i), _) => self.scalar(number, &i.to_string()),
                // `{:?}` always has a '.' or an exponent, so the literal reads back as a float.
                (_, _, Some(f)) if f.is_finite() => self.scalar(number, &format!("{f:?}")),
                _ => Err(Error::Malformed),
            },
            Value::String(s) if string_is_valid(s) => self.scalar(string, s),
            _ => Err(Error::Malformed),
        }
    }

    fn finish(mut self) -> Result<Vec<u8>, Error> {
        // SAFETY: the writer is live.
        if !unsafe { ffi::bvnr_write_finish(self.handle.as_ptr()) } {
            return Err(self.failure());
        }
        // SAFETY: the writer is live.
        let written = unsafe { ffi::bvnr_writer_bytes_written(self.handle.as_ptr()) };
        let mut buf = std::mem::take(&mut self.buf);
        buf.truncate(written as usize);
        Ok(buf)
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        // SAFETY: created by bvnr_writer_create and destroyed once, here.
        unsafe { ffi::bvnr_writer_destroy(self.handle.as_ptr()) }
    }
}

/// A Bovnar identifier: a letter or `_` (or a non-ASCII letter), then letters, digits, `_`,
/// `+` or `-` (the rule of the Bovnar crate's JSON conversion).
fn key_is_valid(key: &str) -> bool {
    let b = key.as_bytes();
    let start_ok = |c: u8| c.is_ascii_alphabetic() || c == b'_' || (0xC3..=0xF4).contains(&c);
    let body_ok = |c: u8| {
        c.is_ascii_alphanumeric() || matches!(c, b'_' | b'+' | b'-') || (c >= 0x80 && c != 0xC2)
    };
    b.first().is_some_and(|&c| start_ok(c)) && b[1..].iter().all(|&c| body_ok(c))
}

/// A Bovnar string cannot carry the C0 controls other than `\t` to `\r`, nor DEL.
fn string_is_valid(s: &str) -> bool {
    !s.bytes()
        .any(|c| c <= 0x08 || (0x0E..=0x1F).contains(&c) || c == 0x7F)
}

/// A parsed document (the C DOM).
struct Doc(NonNull<c_void>);

impl Doc {
    fn parse(bytes: &[u8]) -> Result<Self, Error> {
        let len = u32::try_from(bytes.len()).map_err(|_| Error::TooLarge)?;
        // SAFETY: `bytes` is valid for `len` bytes and only read during the call.
        let doc = unsafe { ffi::bvn_dom_parse(bytes.as_ptr().cast_mut().cast(), len) };
        let doc = Doc(NonNull::new(doc).ok_or(Error::Malformed)?);
        // SAFETY: the document is live.
        if unsafe { ffi::bvn_dom_doc_get_parse_error(doc.0.as_ptr()) } != 0 {
            return Err(Error::Malformed);
        }
        Ok(doc)
    }

    fn entries(&self) -> Vec<(&[u8], *mut c_void)> {
        let doc = self.0.as_ptr();
        // SAFETY: the document is live and owns its entry array.
        unsafe { entries(ffi::bvn_dom_doc_entries(doc), ffi::bvn_dom_doc_count(doc)) }
    }
}

impl Drop for Doc {
    fn drop(&mut self) {
        // SAFETY: created by bvn_dom_parse and destroyed once, here.
        unsafe { ffi::bvn_dom_doc_destroy(self.0.as_ptr()) }
    }
}

/// The (key, node) pairs of an entry array.
///
/// # Safety
/// `list` is null or points at `count` entries with NUL-terminated keys, all owned by a
/// document that outlives the result.
unsafe fn entries<'a>(list: *const ffi::DomEntry, count: u32) -> Vec<(&'a [u8], *mut c_void)> {
    if list.is_null() {
        return Vec::new();
    }
    // SAFETY: guaranteed by the caller.
    let list = unsafe { std::slice::from_raw_parts(list, count as usize) };
    list.iter()
        .filter(|e| !e.key.is_null())
        // SAFETY: the keys are NUL-terminated and owned by the document.
        .map(|e| (unsafe { CStr::from_ptr(e.key) }.to_bytes(), e.value))
        .collect()
}

/// Converts a DOM node into a JSON value for serde. Types JSON cannot carry (symbols,
/// references, octets, datetimes) are [`Error::WrongType`].
fn node_value(node: *mut c_void, depth: usize) -> Result<Value, Error> {
    if depth > MAX_DEPTH {
        return Err(Error::Malformed);
    }
    // SAFETY (all calls below): `node` belongs to a live document; the getters only write
    // through the out pointers we pass.
    unsafe {
        match ffi::bvn_dom_node_type(node) {
            ffi::DOM_NULL => Ok(Value::Null),
            ffi::DOM_BOOL => {
                let mut b = false;
                ffi::bvn_dom_get_bool(node, &mut b)
                    .then_some(Value::Bool(b))
                    .ok_or(Error::WrongType)
            }
            ffi::DOM_INT => {
                let family = ffi::bvn_dom_get_value_type(node).family;
                let (mut u, mut i) = (0u64, 0i64);
                if family != ffi::VT_SINT && ffi::bvn_dom_get_u64(node, &mut u) {
                    Ok(Value::from(u))
                } else if family != ffi::VT_UINT && ffi::bvn_dom_get_i64(node, &mut i) {
                    Ok(Value::from(i))
                } else {
                    Err(Error::WrongType)
                }
            }
            ffi::DOM_FLOAT => {
                let mut f = 0.0;
                if !ffi::bvn_dom_get_float(node, &mut f) {
                    return Err(Error::WrongType);
                }
                Number::from_f64(f)
                    .map(Value::Number)
                    .ok_or(Error::WrongType)
            }
            ffi::DOM_STRING => {
                let (mut p, mut len) = (std::ptr::null(), 0u32);
                if !ffi::bvn_dom_get_string(node, &mut p, &mut len) || p.is_null() {
                    return Err(Error::WrongType);
                }
                let bytes = std::slice::from_raw_parts(p.cast::<u8>(), len as usize);
                let s = std::str::from_utf8(bytes).map_err(|_| Error::Malformed)?;
                Ok(Value::String(s.to_owned()))
            }
            ffi::DOM_STRUCT => {
                let list = entries(
                    ffi::bvn_dom_struct_entries(node),
                    ffi::bvn_dom_struct_count(node),
                );
                let mut map = Map::new();
                for (key, child) in list {
                    let key = std::str::from_utf8(key).map_err(|_| Error::Malformed)?;
                    map.insert(key.to_owned(), node_value(child, depth + 1)?);
                }
                Ok(Value::Object(map))
            }
            ffi::DOM_ARRAY => {
                // Multi-dimensional (`/`-separated) arrays have no message shape.
                if ffi::bvn_dom_array_dims(node) > 1 {
                    return Err(Error::WrongType);
                }
                let count = ffi::bvn_dom_array_count(node);
                let mut items = Vec::with_capacity(count as usize);
                for index in 0..count {
                    let child = ffi::bvn_dom_array_at(node, index);
                    if child.is_null() {
                        return Err(Error::Malformed);
                    }
                    items.push(node_value(child, depth + 1)?);
                }
                Ok(Value::Array(items))
            }
            _ => Err(Error::WrongType),
        }
    }
}

#[cfg(test)]
mod tests;
