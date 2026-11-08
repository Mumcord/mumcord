//! Moving a shared file from sender to recipient (FR-7.31 – FR-7.34).
//!
//! After `go` the sender opens one unidirectional stream of kind [`STREAM_FILE`] on the
//! relay-only P2P connection to the recipient ([`Engine::p2p_relay_connection`]): a [`Header`]
//! (share id, offset, length) and then the file bytes. The recipient announced the download
//! beforehand ([`Engine::file_expect`]); a stream for a share id it did not ask that person for,
//! or with another length than the offer, is stopped unread.
//!
//! The stream has two formats; `req` and `go` negotiate which one (see [`super::wire`]):
//!
//! - **Format 1** (clients from 0.7.x): the fixed 32-byte header (share id, offset and length,
//!   big-endian) and then the raw bytes.
//! - **Format 2**: length-prefixed Bovnar documents, each a u32 big-endian byte length and then
//!   the document. First the header document (`v` = 2, `id` the share id as 32 hex characters,
//!   `off`, `len`; at most [`MAX_HEADER_DOC`] bytes), then data documents with one `octets` field
//!   `d` of 1 to 64 KiB each (at most [`MAX_CHUNK_DOC`] bytes), until `len` bytes arrived.
//!
//! The recipient puts `fmt` = [`STREAM_FORMAT`] in its `req`; the sender answers with
//! [`stream_format`] of it in `go`, and a missing `fmt` on either side means format 1. The stream
//! can arrive before its `go`, so the accept side waits for a `go` from that person before it
//! reads the header; all downloads from one person come from the same client, so the format of
//! any of their `go`s fits.
//!
//! Screen frames on the same connection keep priority: the file stream has a lower QUIC stream
//! priority, so the connection sends file bytes only while no screen data is waiting.
//!
//! Saving and the hash check are [`super::receive`]; the request flow (`req`, `go`, the answer
//! timeout) and the card state belong to the files integration.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mumcord_p2p::quinn::{RecvStream, SendStream};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{Notify, oneshot};

use crate::ServerId;
use crate::p2p::{DirectError, STREAM_FILE};
use crate::peer::{RelayUnavailable, RelayUse};
use crate::session::{Engine, lock};

use super::wire::{parse_document, str_field, u64_field, version_field, write_document};

/// No `go` or `queued` answer to a request within this → "{sender} did not answer" (FR-7.34).
pub const ANSWER_TIMEOUT: Duration = Duration::from_secs(10);
/// After `go`, the stream has to arrive within this, else the relay connection failed (FR-7.34).
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// No bytes for this long during a transfer → "The transfer stalled" (FR-7.34).
pub const STALL_TIMEOUT: Duration = Duration::from_secs(30);
/// How long the debug hook `file_stall_after_bytes` pauses the sender: twice [`STALL_TIMEOUT`].
/// It has to outlast the stall timeout, or the sender's broken stream would end the transfer and
/// the recipient's card would show "The connection was lost" instead of "The transfer stalled".
const HOOK_PAUSE: Duration = Duration::from_secs(60);

/// Highest file stream format this client reads and writes.
pub const STREAM_FORMAT: u8 = 2;
/// Bytes of the format 1 stream header after the kind byte.
pub(crate) const HEADER_LEN: usize = 32;
/// Read and write size of the file body; in format 2 the largest data document payload.
const CHUNK: usize = 64 * 1024;
/// Largest format 2 header document, checked before reading or parsing it.
pub(crate) const MAX_HEADER_DOC: usize = 1024;
/// Largest format 2 data document: one [`CHUNK`] plus room for the key and the octets framing.
pub(crate) const MAX_CHUNK_DOC: usize = CHUNK + 256;
/// QUIC stream priority of file data: below screen frames, which use the default 0 (FR-7.33).
const FILE_PRIORITY: i32 = -1;
/// Stop code for a file stream the recipient did not ask for.
const REFUSED: u32 = 1;

/// A person on a server, as the P2P connections key them.
type Key = (ServerId, u32);

/// Why a transfer did not complete. The UI shows the card text of FR-7.34 for each.
#[derive(Debug, thiserror::Error)]
pub enum TransferError {
    /// "Couldn't connect through the relay: {reason}"; the reason is one of FR-4.11.
    #[error("Couldn't connect through the relay: {0}")]
    Connect(DirectError),
    /// "The transfer stalled"
    #[error("The transfer stalled")]
    Stalled,
    /// The connection or the stream ended before all bytes arrived.
    #[error("The connection was lost")]
    ConnectionLost,
    /// The recipient stopped the stream: it did not ask for this share or this length.
    #[error("The other side refused the file")]
    Refused,
    /// Sender side: the shared file could not be read to its full announced length.
    #[error("Couldn't read the shared file")]
    Read(#[source] std::io::Error),
    /// Not a share id (32 lowercase hex characters).
    #[error("Not a share id")]
    BadShareId,
}

/// The stream header (FR-7.32): big-endian share id (16 bytes), offset (u64) and length (u64).
/// The offset is always 0; it is on the wire so a later version can resume.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Header {
    pub(crate) share: [u8; 16],
    pub(crate) offset: u64,
    pub(crate) length: u64,
}

impl Header {
    pub(crate) fn encode(&self) -> [u8; HEADER_LEN] {
        let mut out = [0u8; HEADER_LEN];
        out[..16].copy_from_slice(&self.share);
        out[16..24].copy_from_slice(&self.offset.to_be_bytes());
        out[24..].copy_from_slice(&self.length.to_be_bytes());
        out
    }

    pub(crate) fn decode(bytes: &[u8; HEADER_LEN]) -> Self {
        let mut share = [0u8; 16];
        share.copy_from_slice(&bytes[..16]);
        let mut offset = [0u8; 8];
        offset.copy_from_slice(&bytes[16..24]);
        let mut length = [0u8; 8];
        length.copy_from_slice(&bytes[24..]);
        Self {
            share,
            offset: u64::from_be_bytes(offset),
            length: u64::from_be_bytes(length),
        }
    }
}

/// The stream format a sender uses for a recipient that reads up to `theirs` (its `req`'s `fmt`).
pub(crate) fn stream_format(theirs: u8) -> u8 {
    theirs.clamp(1, STREAM_FORMAT)
}

/// Why a stream header was not read.
#[derive(Debug)]
enum BadHeader {
    /// The stream ended or broke before the header was complete.
    Ended(std::io::Error),
    /// Not asked for, too large, malformed or not a valid header: the stream is refused.
    Invalid,
}

impl Header {
    /// The format 2 header: one length-prefixed Bovnar document.
    fn encode_v2(&self) -> Option<Vec<u8>> {
        let id = share_hex(&self.share);
        let doc = write_document(MAX_HEADER_DOC, |w| {
            w.write_uint("v", 2, 8, bovnar::Base::DECIMAL)?;
            w.write_string("id", &id)?;
            w.write_uint("off", self.offset, 64, bovnar::Base::DECIMAL)?;
            w.write_uint("len", self.length, 64, bovnar::Base::DECIMAL)
        })?;
        let mut out = Vec::with_capacity(4 + doc.len());
        out.extend_from_slice(&u32::try_from(doc.len()).ok()?.to_be_bytes());
        out.extend_from_slice(&doc);
        Some(out)
    }

    /// The format 2 header document (without its length prefix).
    fn decode_v2(doc: &[u8]) -> Option<Self> {
        let doc = parse_document(doc, MAX_HEADER_DOC).ok()?;
        version_field(&doc, 2).ok()?;
        Some(Self {
            share: share_bytes(str_field(&doc, "id").ok()?)?,
            offset: u64_field(&doc, "off").ok()?,
            length: u64_field(&doc, "len").ok()?,
        })
    }

    /// Reads the header of a stream in `format`. In format 2 the length prefix is checked
    /// against [`MAX_HEADER_DOC`] before anything else is read.
    async fn read(recv: &mut (impl AsyncRead + Unpin), format: u8) -> Result<Self, BadHeader> {
        if format < 2 {
            let mut bytes = [0u8; HEADER_LEN];
            recv.read_exact(&mut bytes)
                .await
                .map_err(BadHeader::Ended)?;
            return Ok(Self::decode(&bytes));
        }
        let mut prefix = [0u8; 4];
        recv.read_exact(&mut prefix)
            .await
            .map_err(BadHeader::Ended)?;
        let len = usize::try_from(u32::from_be_bytes(prefix)).unwrap_or(usize::MAX);
        if len > MAX_HEADER_DOC {
            return Err(BadHeader::Invalid);
        }
        let mut doc = vec![0u8; len];
        recv.read_exact(&mut doc).await.map_err(BadHeader::Ended)?;
        Self::decode_v2(&doc).ok_or(BadHeader::Invalid)
    }
}

/// A share id as 32 lowercase hex characters.
fn share_hex(share: &[u8; 16]) -> String {
    share.iter().map(|b| format!("{b:02x}")).collect()
}

/// The 16 bytes of a share id given as 32 lowercase hex characters.
pub(crate) fn share_bytes(id: &str) -> Option<[u8; 16]> {
    if !super::is_share_id(id) {
        return None;
    }
    let mut out = [0u8; 16];
    for (byte, pair) in out.iter_mut().zip(id.as_bytes().as_chunks::<2>().0) {
        let pair = std::str::from_utf8(pair).ok()?;
        *byte = u8::from_str_radix(pair, 16).ok()?;
    }
    Some(out)
}

/// A share id from one person.
type ShareFrom = (Key, [u8; 16]);

/// Downloads waiting for their stream, by person and share id. `S` is the stream type, so the
/// routing can be tested without a connection.
pub(crate) struct Incoming<S> {
    waiting: Mutex<HashMap<ShareFrom, Waiting<S>>>,
    /// Woken when a `go` names the stream format of a download.
    started: Notify,
}

struct Waiting<S> {
    length: u64,
    /// The stream format from the `go`; `None` until it arrived.
    format: Option<u8>,
    stream: oneshot::Sender<S>,
}

impl<S> Default for Incoming<S> {
    fn default() -> Self {
        Self {
            waiting: Mutex::new(HashMap::new()),
            started: Notify::new(),
        }
    }
}

/// What the waiting downloads say about the stream format of a person's next stream.
#[derive(Debug, PartialEq, Eq)]
enum KnownFormat {
    /// Nothing is asked from that person: the stream is refused.
    NotAsked,
    /// Downloads wait, but no `go` arrived yet.
    Pending,
    Known(u8),
}

impl<S> Incoming<S> {
    /// Waits for `share` from `key` with exactly `length` bytes; a newer wait replaces an older
    /// one for the same share.
    fn expect(&self, key: Key, share: [u8; 16], length: u64) -> oneshot::Receiver<S> {
        let (tx, rx) = oneshot::channel();
        lock(&self.waiting).insert(
            (key, share),
            Waiting {
                length,
                format: None,
                stream: tx,
            },
        );
        rx
    }

    /// The `go` for `share` from `key` named stream format `format`.
    fn set_format(&self, key: Key, share: [u8; 16], format: u8) {
        if let Some(waiting) = lock(&self.waiting).get_mut(&(key, share)) {
            waiting.format = Some(format);
        }
        self.started.notify_waiters();
    }

    fn known_format(&self, key: Key) -> KnownFormat {
        let waiting = lock(&self.waiting);
        let mut from_key = waiting.iter().filter(|((k, _), _)| *k == key).peekable();
        if from_key.peek().is_none() {
            return KnownFormat::NotAsked;
        }
        from_key
            .find_map(|(_, w)| w.format)
            .map_or(KnownFormat::Pending, KnownFormat::Known)
    }

    /// The stream format of the next stream from `key`, waiting for a `go` if needed; `None`
    /// when nothing is asked from that person.
    async fn format(&self, key: Key) -> Option<u8> {
        loop {
            let notified = self.started.notified();
            tokio::pin!(notified);
            // Registered before the check, so a `go` between check and wait is not missed.
            notified.as_mut().enable();
            match self.known_format(key) {
                KnownFormat::NotAsked => return None,
                KnownFormat::Known(format) => return Some(format),
                KnownFormat::Pending => notified.await,
            }
        }
    }

    fn forget(&self, key: Key, share: [u8; 16]) {
        lock(&self.waiting).remove(&(key, share));
    }

    /// Hands a stream from `key` to the download waiting for it. Gives the stream back when no
    /// download from that person waits for that share, or the length differs from the offer
    /// (FR-7.32); a wrong length also ends the wait, as the sender is not sending the offer.
    pub(crate) fn route(&self, key: Key, header: Header, stream: S) -> Result<(), S> {
        if header.offset != 0 {
            return Err(stream);
        }
        let Some(waiting) = lock(&self.waiting).remove(&(key, header.share)) else {
            return Err(stream);
        };
        if waiting.length != header.length {
            return Err(stream);
        }
        waiting.stream.send(stream)
    }
}

/// Reads one file stream after its kind byte and hands it to the waiting download, or stops it
/// (accept side of [`STREAM_FILE`]).
pub(crate) async fn accept(incoming: &Incoming<RecvStream>, key: Key, mut recv: RecvStream) {
    let read = async {
        let format = incoming.format(key).await.ok_or(BadHeader::Invalid)?;
        Header::read(&mut recv, format).await
    };
    let header = match tokio::time::timeout(STALL_TIMEOUT, read).await {
        Ok(Ok(header)) => header,
        Ok(Err(BadHeader::Ended(e))) => {
            return tracing::debug!(error = %e, "file stream ended before its header");
        }
        Ok(Err(BadHeader::Invalid)) => {
            tracing::debug!("refusing a file stream that was not asked for or has a bad header");
            let _ = recv.stop(REFUSED.into());
            return;
        }
        Err(_) => {
            tracing::debug!("file stream sent no header");
            let _ = recv.stop(REFUSED.into());
            return;
        }
    };
    if let Err(mut refused) = incoming.route(key, header, recv) {
        tracing::debug!("refusing a file stream that was not asked for");
        let _ = refused.stop(REFUSED.into());
    }
}

/// A download announced with [`Engine::file_expect`], waiting for its stream.
pub struct Expected {
    incoming: Arc<Incoming<RecvStream>>,
    engine: Engine,
    key: Key,
    share: [u8; 16],
    length: u64,
    format: u8,
    stream: oneshot::Receiver<RecvStream>,
}

impl Expected {
    /// The sender's `go` named stream `format`; the stream is read in it. A format this client
    /// never offered → [`TransferError::Refused`].
    pub fn started(&mut self, format: u8) -> Result<(), TransferError> {
        if !(1..=STREAM_FORMAT).contains(&format) {
            return Err(TransferError::Refused);
        }
        self.format = format;
        self.incoming.set_format(self.key, self.share, format);
        Ok(())
    }

    /// The stream, once the sender opened it. Nothing within [`CONNECT_TIMEOUT`] →
    /// [`TransferError::Connect`] with the relay's current reason (or "not reachable").
    pub async fn stream(mut self) -> Result<IncomingStream<RecvStream>, TransferError> {
        match tokio::time::timeout(CONNECT_TIMEOUT, &mut self.stream).await {
            Ok(Ok(recv)) => Ok(IncomingStream::new(recv, self.length, self.format)),
            Ok(Err(_)) => Err(TransferError::Refused),
            Err(_) => Err(connect_failed(&self.engine, self.key.0)),
        }
    }
}

impl Drop for Expected {
    fn drop(&mut self) {
        self.incoming.forget(self.key, self.share);
    }
}

/// The body of an incoming file stream, exactly the announced length.
pub struct IncomingStream<R> {
    recv: R,
    format: u8,
    /// Announced bytes that have not arrived yet.
    remaining: u64,
    received: u64,
    /// Format 2: the current data document, reused for every one.
    frame: Vec<u8>,
    /// Format 2: the payload of the current data document and how much of it was handed out.
    chunk: Vec<u8>,
    chunk_pos: usize,
}

impl<R: AsyncRead + Unpin> IncomingStream<R> {
    pub(crate) fn new(recv: R, length: u64, format: u8) -> Self {
        Self {
            recv,
            format,
            remaining: length,
            received: 0,
            frame: Vec::new(),
            chunk: Vec::new(),
            chunk_pos: 0,
        }
    }

    /// Bytes received so far.
    pub fn received(&self) -> u64 {
        self.received
    }

    /// Reads the next bytes into `buf`; `Ok(0)` once all announced bytes arrived and the sender
    /// ended the stream. No bytes for [`STALL_TIMEOUT`] → [`TransferError::Stalled`]; an early
    /// end or extra bytes → [`TransferError::ConnectionLost`].
    ///
    /// In format 2 a data document that is too large, malformed, empty or holds more than the
    /// announced rest also ends the transfer with [`TransferError::ConnectionLost`].
    pub async fn read(&mut self, buf: &mut [u8]) -> Result<usize, TransferError> {
        if self.format >= 2 {
            return self.read_v2(buf).await;
        }
        if self.remaining == 0 {
            return self.end().await;
        }
        let want = buf
            .len()
            .min(usize::try_from(self.remaining).unwrap_or(usize::MAX));
        let read = timed_read(&mut self.recv, &mut buf[..want]).await?;
        if read == 0 {
            return Err(TransferError::ConnectionLost);
        }
        self.remaining -= read as u64;
        self.received += read as u64;
        Ok(read)
    }

    async fn read_v2(&mut self, buf: &mut [u8]) -> Result<usize, TransferError> {
        if self.chunk_pos == self.chunk.len() {
            if self.remaining == 0 {
                return self.end().await;
            }
            self.next_chunk().await?;
        }
        let n = buf.len().min(self.chunk.len() - self.chunk_pos);
        buf[..n].copy_from_slice(&self.chunk[self.chunk_pos..self.chunk_pos + n]);
        self.chunk_pos += n;
        self.received += n as u64;
        Ok(n)
    }

    /// Reads the next data document into `chunk`; its size is checked before it is read.
    async fn next_chunk(&mut self) -> Result<(), TransferError> {
        let mut prefix = [0u8; 4];
        read_full(&mut self.recv, &mut prefix).await?;
        let len = usize::try_from(u32::from_be_bytes(prefix)).unwrap_or(usize::MAX);
        if len > MAX_CHUNK_DOC {
            tracing::debug!(len, "file stream data document too large");
            return Err(TransferError::ConnectionLost);
        }
        self.frame.resize(len, 0);
        read_full(&mut self.recv, &mut self.frame).await?;
        let doc = parse_document(&self.frame, MAX_CHUNK_DOC).map_err(|reason| {
            tracing::debug!(%reason, len, "file stream data document refused");
            TransferError::ConnectionLost
        })?;
        let data = doc.get("d").and_then(|d| d.as_bytes()).unwrap_or_default();
        if data.is_empty()
            || data.len() > CHUNK
            || u64::try_from(data.len()).map_or(true, |n| n > self.remaining)
        {
            // Empty documents would keep a stream alive without progress; more than announced
            // is not the offered file.
            tracing::debug!(bytes = data.len(), "file stream data document refused");
            return Err(TransferError::ConnectionLost);
        }
        self.chunk.clear();
        self.chunk.extend_from_slice(data);
        self.chunk_pos = 0;
        self.remaining -= data.len() as u64;
        Ok(())
    }

    /// All announced bytes arrived: the stream has to end here; anything more is not the offered
    /// file.
    async fn end(&mut self) -> Result<usize, TransferError> {
        let mut extra = [0u8; 1];
        match timed_read(&mut self.recv, &mut extra).await? {
            0 => Ok(0),
            _ => Err(TransferError::ConnectionLost),
        }
    }
}

/// One read; no bytes for [`STALL_TIMEOUT`] → [`TransferError::Stalled`].
async fn timed_read(
    recv: &mut (impl AsyncRead + Unpin),
    buf: &mut [u8],
) -> Result<usize, TransferError> {
    match tokio::time::timeout(STALL_TIMEOUT, recv.read(buf)).await {
        Ok(Ok(read)) => Ok(read),
        Ok(Err(e)) => {
            tracing::debug!(error = %e, "file stream broke");
            Err(TransferError::ConnectionLost)
        }
        Err(_) => Err(TransferError::Stalled),
    }
}

/// Fills `buf` with the stall timeout per read, as in format 1; an early end →
/// [`TransferError::ConnectionLost`].
async fn read_full(
    recv: &mut (impl AsyncRead + Unpin),
    buf: &mut [u8],
) -> Result<(), TransferError> {
    let mut at = 0;
    while at < buf.len() {
        match timed_read(recv, &mut buf[at..]).await? {
            0 => return Err(TransferError::ConnectionLost),
            n => at += n,
        }
    }
    Ok(())
}

/// Writes the header and exactly `length` bytes of `file` to `out` in stream `format`, reporting
/// the bytes sent so far. A write blocked for [`STALL_TIMEOUT`] → [`TransferError::Stalled`].
pub(crate) async fn send_body(
    out: &mut (impl AsyncWrite + Unpin),
    header: Header,
    format: u8,
    file: &mut (impl AsyncRead + Unpin),
    progress: impl FnMut(u64),
) -> Result<(), TransferError> {
    send_body_as(
        out,
        header,
        format,
        file,
        progress,
        crate::test_hooks::file_corrupt(),
        crate::test_hooks::file_stall_after_bytes(),
    )
    .await
}

/// [`send_body`]; `corrupt` flips the first body byte (the AC-7.11 test hook), so the recipient's
/// checksum must catch it. `stall_after` stops the sender after that many bytes (the F3a (e) test
/// hook): it waits [`HOOK_PAUSE`] without writing — the client stays connected, so only the
/// recipient's stall timer can end the transfer — and then reports [`TransferError::Stalled`].
async fn send_body_as(
    out: &mut (impl AsyncWrite + Unpin),
    header: Header,
    format: u8,
    file: &mut (impl AsyncRead + Unpin),
    mut progress: impl FnMut(u64),
    corrupt: bool,
    stall_after: Option<u64>,
) -> Result<(), TransferError> {
    if format >= 2 {
        let encoded = header.encode_v2().ok_or_else(encode_failed)?;
        write_timed(out, &encoded).await?;
    } else {
        write_timed(out, &header.encode()).await?;
    }
    let mut buf = vec![0u8; CHUNK];
    // Format 2: the data document of the current chunk, reused for every one.
    let mut frame = Vec::new();
    let mut sent = 0u64;
    while sent < header.length {
        // The hook stops at exactly its byte count, so the recipient sees progress first and then
        // nothing but the stall.
        let hook_cap =
            usize::try_from(stall_after.unwrap_or(u64::MAX) - sent).unwrap_or(usize::MAX);
        let want = buf
            .len()
            .min(usize::try_from(header.length - sent).unwrap_or(usize::MAX))
            .min(hook_cap);
        let read = file
            .read(&mut buf[..want])
            .await
            .map_err(TransferError::Read)?;
        if read == 0 {
            return Err(TransferError::Read(
                std::io::ErrorKind::UnexpectedEof.into(),
            ));
        }
        if corrupt && sent == 0 {
            buf[0] ^= 0xFF;
        }
        if format >= 2 {
            chunk_frame(&mut frame, &buf[..read]).ok_or_else(encode_failed)?;
            write_timed(out, &frame).await?;
        } else {
            write_timed(out, &buf[..read]).await?;
        }
        sent += read as u64;
        progress(sent);
        if stall_after.is_some_and(|after| sent >= after) {
            tracing::debug!(bytes = sent, "test hook stops the file stream");
            tokio::time::sleep(HOOK_PAUSE).await;
            return Err(TransferError::Stalled);
        }
    }
    Ok(())
}

/// Puts `data` into `frame` as one length-prefixed data document.
fn chunk_frame(frame: &mut Vec<u8>, data: &[u8]) -> Option<()> {
    let mut w = bovnar::Writer::to_mem(MAX_CHUNK_DOC, false).ok()?;
    w.write_octets("d", data).and_then(|()| w.finish()).ok()?;
    let doc = w.get_output();
    frame.clear();
    frame.extend_from_slice(&u32::try_from(doc.len()).ok()?.to_be_bytes());
    frame.extend_from_slice(doc);
    Some(())
}

/// Bovnar could not encode a header or data document. That does not happen for valid input, so
/// the transfer just ends.
fn encode_failed() -> TransferError {
    tracing::warn!("couldn't encode the file stream");
    TransferError::ConnectionLost
}

async fn write_timed(
    out: &mut (impl AsyncWrite + Unpin),
    bytes: &[u8],
) -> Result<(), TransferError> {
    match tokio::time::timeout(STALL_TIMEOUT, out.write_all(bytes)).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => {
            tracing::debug!(error = %e, "file stream to the recipient broke");
            Err(TransferError::ConnectionLost)
        }
        Err(_) => Err(TransferError::Stalled),
    }
}

/// [`TransferError::Connect`] with the relay's current reason for `server_id`. With a relay in
/// place, a guest's transfer that never connected was almost always refused by the relay's
/// default "registered users only" rule (#406); the bot tells only the requester, so say that
/// instead of "not reachable".
fn connect_failed(engine: &Engine, server_id: ServerId) -> TransferError {
    let reason = engine
        .relays()
        .availability_for(server_id, RelayUse::Files)
        .err()
        .unwrap_or_else(|| {
            let guest = engine
                .session_ctx_for(server_id)
                .is_ok_and(|ctx| !crate::peer::self_registered(&ctx));
            if guest {
                RelayUnavailable::NotRegistered
            } else {
                RelayUnavailable::Unreachable
            }
        });
    TransferError::Connect(DirectError::Relay(reason))
}

impl Engine {
    /// Announces that `session` on `server_id` will send share `share_id` with `length` bytes
    /// (the offer's size); call it before sending `req`. Only an announced stream is accepted;
    /// dropping the result stops waiting.
    pub fn file_expect(
        &self,
        server_id: ServerId,
        session: u32,
        share_id: &str,
        length: u64,
    ) -> Result<Expected, TransferError> {
        let share = share_bytes(share_id).ok_or(TransferError::BadShareId)?;
        let key = (server_id, session);
        let incoming = self.direct().files().clone();
        let stream = incoming.expect(key, share, length);
        Ok(Expected {
            incoming,
            engine: self.clone(),
            key,
            share,
            length,
            format: 1,
            stream,
        })
    }

    /// Sends the file at `path` (exactly `length` bytes, the offer's size) for share `share_id`
    /// to `session` on `server_id` in stream `format` (the one its `go` named), through the relay
    /// only (FR-7.31, FR-7.32). `progress` gets the bytes sent so far. Returns once the recipient
    /// acknowledged every byte.
    #[allow(clippy::too_many_arguments)]
    pub async fn file_send(
        &self,
        server_id: ServerId,
        session: u32,
        share_id: &str,
        path: &Path,
        length: u64,
        format: u8,
        progress: impl FnMut(u64),
    ) -> Result<(), TransferError> {
        let share = share_bytes(share_id).ok_or(TransferError::BadShareId)?;
        let mut file = tokio::fs::File::open(path)
            .await
            .map_err(TransferError::Read)?;
        let mut send =
            match tokio::time::timeout(CONNECT_TIMEOUT, self.file_stream(server_id, session)).await
            {
                Ok(Ok(send)) => send,
                Ok(Err(e)) => return Err(TransferError::Connect(e)),
                Err(_) => return Err(connect_failed(self, server_id)),
            };
        let header = Header {
            share,
            offset: 0,
            length,
        };
        send_body(&mut send, header, format, &mut file, progress).await?;
        if send.finish().is_err() {
            return Err(TransferError::ConnectionLost);
        }
        match tokio::time::timeout(STALL_TIMEOUT, send.stopped()).await {
            Ok(Ok(None)) => Ok(()),
            Ok(Ok(Some(_))) => Err(TransferError::Refused),
            Ok(Err(_)) => Err(TransferError::ConnectionLost),
            Err(_) => Err(TransferError::Stalled),
        }
    }

    /// A new file stream to `session` on the relay-only connection, kind byte sent.
    async fn file_stream(
        &self,
        server_id: ServerId,
        session: u32,
    ) -> Result<SendStream, DirectError> {
        let conn = self
            .p2p_relay_connection(server_id, session, RelayUse::Files)
            .await?;
        let mut send = conn
            .connection()
            .open_uni()
            .await
            .map_err(|e| DirectError::Stream(e.to_string()))?;
        send.set_priority(FILE_PRIORITY)
            .map_err(|e| DirectError::Stream(e.to_string()))?;
        send.write_all(&[STREAM_FILE])
            .await
            .map_err(|e| DirectError::Stream(e.to_string()))?;
        Ok(send)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "00112233445566778899aabbccddeeff";
    const KEY: Key = (1, 7);

    fn header(length: u64) -> Header {
        Header {
            share: share_bytes(ID).expect("share id"),
            offset: 0,
            length,
        }
    }

    #[test]
    fn header_round_trips_big_endian() {
        let h = Header {
            share: share_bytes(ID).expect("share id"),
            offset: 0,
            length: 0x0102_0304_0506_0708,
        };
        let bytes = h.encode();
        assert_eq!(
            bytes[..16],
            [
                0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
                0xee, 0xff
            ]
        );
        assert_eq!(bytes[16..24], [0; 8]);
        assert_eq!(bytes[24..], [1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(Header::decode(&bytes), h);
    }

    #[test]
    fn share_ids_must_be_lowercase_hex() {
        assert!(share_bytes(ID).is_some());
        assert!(share_bytes(&ID.to_uppercase()).is_none());
        assert!(share_bytes(&ID[1..]).is_none());
        assert!(share_bytes("g0112233445566778899aabbccddeeff").is_none());
    }

    #[test]
    fn routes_only_the_requested_share_from_that_person_with_its_length() {
        let incoming = Incoming::<u8>::default();
        let mut rx = incoming.expect(KEY, header(10).share, 10);
        // Another person, another share, an offset or another length are refused.
        assert_eq!(incoming.route((1, 8), header(10), 1), Err(1));
        assert_eq!(incoming.route((2, 7), header(10), 2), Err(2));
        let mut other = header(10);
        other.share[0] = 0xff;
        assert_eq!(incoming.route(KEY, other, 3), Err(3));
        let mut resumed = header(10);
        resumed.offset = 5;
        assert_eq!(incoming.route(KEY, resumed, 4), Err(4));
        assert!(rx.try_recv().is_err(), "still waiting");
        assert_eq!(incoming.route(KEY, header(10), 5), Ok(()));
        assert_eq!(rx.try_recv(), Ok(5));
        // Each wait takes one stream.
        assert_eq!(incoming.route(KEY, header(10), 6), Err(6));
    }

    #[test]
    fn a_wrong_length_ends_the_wait() {
        let incoming = Incoming::<u8>::default();
        let mut rx = incoming.expect(KEY, header(10).share, 10);
        assert_eq!(incoming.route(KEY, header(11), 1), Err(1));
        assert!(rx.try_recv().is_err());
        assert_eq!(incoming.route(KEY, header(10), 2), Err(2));
    }

    #[test]
    fn forgotten_waits_refuse_the_stream() {
        let incoming = Incoming::<u8>::default();
        let _rx = incoming.expect(KEY, header(10).share, 10);
        incoming.forget(KEY, header(10).share);
        assert_eq!(incoming.route(KEY, header(10), 1), Err(1));
    }

    async fn read_all(
        stream: &mut IncomingStream<impl AsyncRead + Unpin>,
    ) -> Result<Vec<u8>, TransferError> {
        let mut out = Vec::new();
        let mut buf = [0u8; 4];
        loop {
            let n = stream.read(&mut buf).await?;
            if n == 0 {
                return Ok(out);
            }
            out.extend_from_slice(&buf[..n]);
        }
    }

    #[tokio::test]
    async fn sends_header_and_exact_body() {
        let body: Vec<u8> = (0..200u8).collect();
        let mut wire = Vec::new();
        let mut seen = Vec::new();
        send_body(&mut wire, header(200), 1, &mut body.as_slice(), |n| {
            seen.push(n)
        })
        .await
        .expect("sent");
        assert_eq!(wire[..HEADER_LEN], header(200).encode());
        assert_eq!(&wire[HEADER_LEN..], body.as_slice());
        assert_eq!(seen.last(), Some(&200));

        let mut stream = IncomingStream::new(&wire[HEADER_LEN..], 200, 1);
        assert_eq!(read_all(&mut stream).await.expect("read"), body);
        assert_eq!(stream.received(), 200);
    }

    #[tokio::test]
    async fn the_corrupt_hook_flips_the_first_body_byte() {
        let body = [7u8; 20];
        let mut wire = Vec::new();
        send_body_as(
            &mut wire,
            header(20),
            1,
            &mut body.as_slice(),
            |_| {},
            true,
            None,
        )
        .await
        .expect("sent");
        assert_eq!(wire[HEADER_LEN], 7 ^ 0xFF);
        assert_eq!(&wire[HEADER_LEN + 1..], &body[1..]);
    }

    #[tokio::test]
    async fn a_shorter_file_than_announced_is_a_read_error() {
        let mut wire = Vec::new();
        let err = send_body(&mut wire, header(10), 1, &mut [1u8; 4].as_slice(), |_| {})
            .await
            .expect_err("short file");
        assert!(matches!(err, TransferError::Read(_)));
    }

    #[tokio::test]
    async fn early_end_or_extra_bytes_lose_the_transfer() {
        let mut short = IncomingStream::new(&[1u8; 5][..], 10, 1);
        assert!(matches!(
            read_all(&mut short).await,
            Err(TransferError::ConnectionLost)
        ));
        let mut long = IncomingStream::new(&[1u8; 11][..], 10, 1);
        assert!(matches!(
            read_all(&mut long).await,
            Err(TransferError::ConnectionLost)
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn no_data_for_30_seconds_stalls() {
        let (_writer, reader) = tokio::io::duplex(64);
        let mut stream = IncomingStream::new(reader, 10, 1);
        let mut buf = [0u8; 4];
        let started = tokio::time::Instant::now();
        assert!(matches!(
            stream.read(&mut buf).await,
            Err(TransferError::Stalled)
        ));
        assert_eq!(started.elapsed(), STALL_TIMEOUT);
    }

    #[tokio::test(start_paused = true)]
    async fn a_blocked_write_stalls() {
        // A full pipe nobody reads: the sender's write waits until the stall timeout.
        let (mut writer, _reader) = tokio::io::duplex(8);
        let body = [0u8; 64];
        let err = send_body(&mut writer, header(64), 1, &mut body.as_slice(), |_| {})
            .await
            .expect_err("stalled");
        assert!(matches!(err, TransferError::Stalled));
    }

    #[tokio::test(start_paused = true)]
    async fn the_stall_hook_stops_sending_after_its_byte_count() {
        let body = [7u8; 200];
        let mut wire = Vec::new();
        let started = tokio::time::Instant::now();
        let err = send_body_as(
            &mut wire,
            header(200),
            1,
            &mut body.as_slice(),
            |_| {},
            false,
            Some(100),
        )
        .await
        .expect_err("stalled by the hook");
        assert!(matches!(err, TransferError::Stalled));
        // The recipient saw progress before the pause, so its card shows a stall, not a lost
        // connection: the pause outlasts the recipient's stall timer (issue #420).
        assert_eq!(&wire[HEADER_LEN..], &body[..100]);
        assert_eq!(started.elapsed(), HOOK_PAUSE);
        assert!(HOOK_PAUSE > STALL_TIMEOUT);
    }

    /// A format 2 stream: the header from `wire` and the body reader after it.
    async fn open_v2(wire: &[u8], length: u64) -> (Header, IncomingStream<&[u8]>) {
        let mut rest = wire;
        let header = Header::read(&mut rest, 2).await.expect("header");
        (header, IncomingStream::new(rest, length, 2))
    }

    /// A format 2 stream with a valid header for `length` bytes and then `docs` raw.
    fn v2_wire(length: u64, docs: &[u8]) -> Vec<u8> {
        let mut wire = header(length).encode_v2().expect("header");
        wire.extend_from_slice(docs);
        wire
    }

    fn data_doc(data: &[u8]) -> Vec<u8> {
        let mut frame = Vec::new();
        chunk_frame(&mut frame, data).expect("encoded");
        frame
    }

    fn framed(doc: &[u8]) -> Vec<u8> {
        let mut out = u32::try_from(doc.len())
            .expect("small")
            .to_be_bytes()
            .to_vec();
        out.extend_from_slice(doc);
        out
    }

    #[test]
    fn the_sender_uses_the_highest_format_both_read() {
        assert_eq!(stream_format(STREAM_FORMAT), 2);
        // A recipient from 0.7.x sends no `fmt`, which reads as 1.
        assert_eq!(stream_format(1), 1);
        // A newer recipient that reads more still gets what this client writes.
        assert_eq!(stream_format(9), 2);
        assert_eq!(stream_format(0), 1);
    }

    #[tokio::test]
    async fn format_1_to_format_1() {
        let body: Vec<u8> = (0..200u8).collect();
        let mut wire = Vec::new();
        send_body(&mut wire, header(200), 1, &mut body.as_slice(), |_| {})
            .await
            .expect("sent");
        let mut rest = wire.as_slice();
        assert_eq!(
            Header::read(&mut rest, 1).await.expect("header"),
            header(200)
        );
        let mut stream = IncomingStream::new(rest, 200, 1);
        assert_eq!(read_all(&mut stream).await.expect("read"), body);
    }

    #[tokio::test]
    async fn format_2_to_format_2_in_several_chunks() {
        let length = CHUNK * 2 + 1000;
        let body: Vec<u8> = (0..length).map(|i| (i % 251) as u8).collect();
        let mut wire = Vec::new();
        let mut seen = Vec::new();
        send_body(
            &mut wire,
            header(length as u64),
            2,
            &mut body.as_slice(),
            |n| seen.push(n),
        )
        .await
        .expect("sent");
        assert_eq!(seen.last(), Some(&(length as u64)));
        // Header document plus three data documents, a few bytes of framing each.
        assert!(wire.len() < length + MAX_HEADER_DOC + 3 * 64);

        let (h, mut stream) = open_v2(&wire, length as u64).await;
        assert_eq!(h, header(length as u64));
        let mut out = Vec::new();
        let mut buf = vec![0u8; 256 * 1024];
        loop {
            let n = stream.read(&mut buf).await.expect("read");
            if n == 0 {
                break;
            }
            out.extend_from_slice(&buf[..n]);
        }
        assert_eq!(out, body);
        assert_eq!(stream.received(), length as u64);
    }

    #[tokio::test]
    async fn format_2_header_is_a_bovnar_document() {
        let wire = header(5).encode_v2().expect("header");
        let len = u32::from_be_bytes(wire[..4].try_into().expect("prefix")) as usize;
        assert_eq!(wire.len(), 4 + len);
        let doc = bovnar::DomDoc::parse(&wire[4..]).expect("bovnar");
        assert_eq!(doc.get("v").and_then(|n| n.as_u8()), Some(2));
        assert_eq!(doc.get("id").and_then(|n| n.as_str()), Some(ID));
        assert_eq!(doc.get("off").and_then(|n| n.as_u64()), Some(0));
        assert_eq!(doc.get("len").and_then(|n| n.as_u64()), Some(5));
    }

    #[tokio::test]
    async fn an_old_recipient_gets_format_1_from_a_new_sender() {
        // A 0.7.x `req` has no `fmt`.
        let req = br#".v = 1; .t = "req"; .id = "00112233445566778899aabbccddeeff";"#;
        let Some(super::super::wire::FileMessage::Request { fmt, .. }) =
            super::super::wire::decode(req)
        else {
            panic!("a request");
        };
        let format = stream_format(fmt);
        assert_eq!(format, 1);
        let body = [9u8; 40];
        let mut wire = Vec::new();
        send_body(&mut wire, header(40), format, &mut body.as_slice(), |_| {})
            .await
            .expect("sent");
        // Exactly what a 0.7.x recipient reads: the fixed header and then the raw bytes.
        let fixed: &[u8; HEADER_LEN] = wire[..HEADER_LEN].try_into().expect("header");
        assert_eq!(Header::decode(fixed), header(40));
        assert_eq!(&wire[HEADER_LEN..], &body);
    }

    #[tokio::test]
    async fn a_new_recipient_reads_format_1_from_an_old_sender() {
        // A 0.7.x `go` has no `fmt`: the stream is read as format 1.
        let go = br#".v = 1; .t = "go"; .id = "00112233445566778899aabbccddeeff";"#;
        let Some(super::super::wire::FileMessage::Go { fmt, .. }) = super::super::wire::decode(go)
        else {
            panic!("a go");
        };
        assert_eq!(fmt, 1);
        let incoming = Incoming::<u8>::default();
        let _rx = incoming.expect(KEY, header(4).share, 4);
        incoming.set_format(KEY, header(4).share, fmt);
        assert_eq!(incoming.format(KEY).await, Some(1));
        // What a 0.7.x sender writes.
        let mut wire = header(4).encode().to_vec();
        wire.extend_from_slice(&[1, 2, 3, 4]);
        let mut rest = wire.as_slice();
        assert_eq!(
            Header::read(&mut rest, fmt).await.expect("header"),
            header(4)
        );
        let mut stream = IncomingStream::new(rest, 4, fmt);
        assert_eq!(read_all(&mut stream).await.expect("read"), [1, 2, 3, 4]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_stream_before_its_go_waits_for_the_format() {
        let incoming = Arc::new(Incoming::<u8>::default());
        assert_eq!(incoming.known_format(KEY), KnownFormat::NotAsked);
        assert_eq!(incoming.format(KEY).await, None, "nothing asked: refused");
        let _rx = incoming.expect(KEY, header(4).share, 4);
        assert_eq!(incoming.known_format(KEY), KnownFormat::Pending);
        let waiter = tokio::spawn({
            let incoming = incoming.clone();
            async move { incoming.format(KEY).await }
        });
        tokio::task::yield_now().await;
        assert!(!waiter.is_finished());
        incoming.set_format(KEY, header(4).share, 2);
        assert_eq!(waiter.await.expect("joined"), Some(2));
        // Another person's downloads say nothing about this one.
        assert_eq!(incoming.known_format((1, 8)), KnownFormat::NotAsked);
    }

    #[tokio::test]
    async fn bad_format_2_headers_are_refused() {
        let invalid = |wire: Vec<u8>| async move {
            matches!(
                Header::read(&mut wire.as_slice(), 2).await,
                Err(BadHeader::Invalid)
            )
        };
        // Oversized: refused from the prefix alone, before anything else is read.
        let mut oversized = u32::try_from(MAX_HEADER_DOC + 1)
            .expect("small")
            .to_be_bytes()
            .to_vec();
        oversized.extend_from_slice(&[b' '; 8]);
        assert!(invalid(oversized).await);
        assert!(invalid(framed(b"not bovnar ;;;")).await);
        assert!(
            invalid(framed(
                br#".v = 1; .id = "00112233445566778899aabbccddeeff"; .off = 0; .len = 4;"#
            ))
            .await
        );
        assert!(invalid(framed(br#".v = 2; .id = "abc"; .off = 0; .len = 4;"#)).await);
        assert!(
            invalid(framed(
                br#".v = 2; .id = "00112233445566778899aabbccddeeff"; .len = 4;"#
            ))
            .await
        );
        assert!(
            invalid(framed(
                br#".v = 2; .id = "00112233445566778899aabbccddeeff"; .off = 0; .len = "4";"#
            ))
            .await
        );
        // A cut-off header just ends.
        let whole = header(4).encode_v2().expect("header");
        assert!(matches!(
            Header::read(&mut &whole[..whole.len() - 1], 2).await,
            Err(BadHeader::Ended(_))
        ));
        // Newer header versions with unknown fields are read.
        let newer = framed(
            br#".v = 3; .id = "00112233445566778899aabbccddeeff"; .off = 0; .len = 4; .x = 1;"#,
        );
        assert_eq!(
            Header::read(&mut newer.as_slice(), 2)
                .await
                .expect("header"),
            header(4)
        );
    }

    #[tokio::test]
    async fn bad_format_2_data_documents_lose_the_transfer() {
        let mut oversized = u32::try_from(MAX_CHUNK_DOC + 1)
            .expect("small")
            .to_be_bytes()
            .to_vec();
        oversized.extend_from_slice(&[0; 16]);
        let too_much = vec![1u8; CHUNK + 1];
        let cases = [
            oversized,
            framed(b"not bovnar ;;;"),
            framed(br#".x = 1;"#),
            framed(br#".d = "text";"#),
            data_doc(&[]),
            data_doc(&too_much),
            data_doc(&[1, 2, 3])[..5].to_vec(),
        ];
        for (i, docs) in cases.into_iter().enumerate() {
            let length = (CHUNK + 1) as u64;
            let wire = v2_wire(length, &docs);
            let (_, mut stream) = open_v2(&wire, length).await;
            assert!(
                matches!(
                    read_all(&mut stream).await,
                    Err(TransferError::ConnectionLost)
                ),
                "case {i}"
            );
        }
    }

    #[tokio::test]
    async fn format_2_needs_exactly_the_announced_length() {
        let lost = |length: u64, docs: Vec<u8>| async move {
            let wire = v2_wire(length, &docs);
            let (_, mut stream) = open_v2(&wire, length).await;
            matches!(
                read_all(&mut stream).await,
                Err(TransferError::ConnectionLost)
            )
        };
        // Fewer bytes than announced, then the end.
        assert!(lost(10, data_doc(&[1; 5])).await);
        // One document with more than announced.
        assert!(lost(4, data_doc(&[1; 5])).await);
        // All bytes, then one more document.
        let mut extra = data_doc(&[1; 4]);
        extra.extend_from_slice(&data_doc(&[2]));
        assert!(lost(4, extra).await);
        // Exactly the announced bytes in two documents.
        let mut exact = data_doc(&[1; 3]);
        exact.extend_from_slice(&data_doc(&[2]));
        let wire = v2_wire(4, &exact);
        let (_, mut stream) = open_v2(&wire, 4).await;
        assert_eq!(read_all(&mut stream).await.expect("read"), [1, 1, 1, 2]);
    }

    #[tokio::test]
    async fn the_corrupt_hook_also_works_in_format_2() {
        let body = [7u8; 20];
        let mut wire = Vec::new();
        send_body_as(
            &mut wire,
            header(20),
            2,
            &mut body.as_slice(),
            |_| {},
            true,
            None,
        )
        .await
        .expect("sent");
        let (_, mut stream) = open_v2(&wire, 20).await;
        let got = read_all(&mut stream).await.expect("read");
        assert_eq!(got[0], 7 ^ 0xFF);
        assert_eq!(&got[1..], &body[1..]);
    }
}
