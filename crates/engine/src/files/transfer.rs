//! Moving a shared file from sender to recipient (FR-7.31 – FR-7.34).
//!
//! After `go` the sender opens one unidirectional stream of kind [`STREAM_FILE`] on the
//! relay-only P2P connection to the recipient ([`Engine::p2p_relay_connection`]): a fixed
//! [`Header`] (share id, offset, length) and then the raw bytes. The recipient announced the
//! download beforehand ([`Engine::file_expect`]); a stream for a share id it did not ask that
//! person for, or with another length than the offer, is stopped unread.
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
use tokio::sync::oneshot;

use crate::ServerId;
use crate::p2p::{DirectError, STREAM_FILE};
use crate::peer::{RelayUnavailable, RelayUse};
use crate::session::{Engine, lock};

/// No `go` or `queued` answer to a request within this → "{sender} did not answer" (FR-7.34).
pub const ANSWER_TIMEOUT: Duration = Duration::from_secs(10);
/// After `go`, the stream has to arrive within this, else the relay connection failed (FR-7.34).
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// No bytes for this long during a transfer → "The transfer stalled" (FR-7.34).
pub const STALL_TIMEOUT: Duration = Duration::from_secs(30);

/// Bytes of the stream header after the kind byte.
pub(crate) const HEADER_LEN: usize = 32;
/// Read and write size of the file body.
const CHUNK: usize = 64 * 1024;
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
}

struct Waiting<S> {
    length: u64,
    stream: oneshot::Sender<S>,
}

impl<S> Default for Incoming<S> {
    fn default() -> Self {
        Self {
            waiting: Mutex::new(HashMap::new()),
        }
    }
}

impl<S> Incoming<S> {
    /// Waits for `share` from `key` with exactly `length` bytes; a newer wait replaces an older
    /// one for the same share.
    fn expect(&self, key: Key, share: [u8; 16], length: u64) -> oneshot::Receiver<S> {
        let (tx, rx) = oneshot::channel();
        lock(&self.waiting).insert((key, share), Waiting { length, stream: tx });
        rx
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
    let mut header = [0u8; HEADER_LEN];
    match tokio::time::timeout(STALL_TIMEOUT, recv.read_exact(&mut header)).await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => return tracing::debug!(error = %e, "file stream ended before its header"),
        Err(_) => {
            tracing::debug!("file stream sent no header");
            let _ = recv.stop(REFUSED.into());
            return;
        }
    }
    if let Err(mut refused) = incoming.route(key, Header::decode(&header), recv) {
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
    stream: oneshot::Receiver<RecvStream>,
}

impl Expected {
    /// The stream, once the sender opened it. Nothing within [`CONNECT_TIMEOUT`] →
    /// [`TransferError::Connect`] with the relay's current reason (or "not reachable").
    pub async fn stream(mut self) -> Result<IncomingStream<RecvStream>, TransferError> {
        match tokio::time::timeout(CONNECT_TIMEOUT, &mut self.stream).await {
            Ok(Ok(recv)) => Ok(IncomingStream::new(recv, self.length)),
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
    remaining: u64,
    received: u64,
}

impl<R: AsyncRead + Unpin> IncomingStream<R> {
    pub(crate) fn new(recv: R, length: u64) -> Self {
        Self {
            recv,
            remaining: length,
            received: 0,
        }
    }

    /// Bytes received so far.
    pub fn received(&self) -> u64 {
        self.received
    }

    /// Reads the next bytes into `buf`; `Ok(0)` once all announced bytes arrived and the sender
    /// ended the stream. No bytes for [`STALL_TIMEOUT`] → [`TransferError::Stalled`]; an early
    /// end or extra bytes → [`TransferError::ConnectionLost`].
    pub async fn read(&mut self, buf: &mut [u8]) -> Result<usize, TransferError> {
        if self.remaining == 0 {
            // The stream has to end here; anything more is not the offered file.
            let mut extra = [0u8; 1];
            return match self.timed_read(&mut extra).await? {
                0 => Ok(0),
                _ => Err(TransferError::ConnectionLost),
            };
        }
        let want = buf
            .len()
            .min(usize::try_from(self.remaining).unwrap_or(usize::MAX));
        let read = self.timed_read(&mut buf[..want]).await?;
        if read == 0 {
            return Err(TransferError::ConnectionLost);
        }
        self.remaining -= read as u64;
        self.received += read as u64;
        Ok(read)
    }

    async fn timed_read(&mut self, buf: &mut [u8]) -> Result<usize, TransferError> {
        match tokio::time::timeout(STALL_TIMEOUT, self.recv.read(buf)).await {
            Ok(Ok(read)) => Ok(read),
            Ok(Err(e)) => {
                tracing::debug!(error = %e, "file stream broke");
                Err(TransferError::ConnectionLost)
            }
            Err(_) => Err(TransferError::Stalled),
        }
    }
}

/// Writes the header and exactly `length` bytes of `file` to `out`, reporting the bytes sent so
/// far. A write blocked for [`STALL_TIMEOUT`] → [`TransferError::Stalled`].
pub(crate) async fn send_body(
    out: &mut (impl AsyncWrite + Unpin),
    header: Header,
    file: &mut (impl AsyncRead + Unpin),
    progress: impl FnMut(u64),
) -> Result<(), TransferError> {
    let corrupt = crate::test_hooks::file_corrupt();
    send_body_as(out, header, file, progress, corrupt).await
}

/// [`send_body`]; `corrupt` flips the first body byte (the AC-7.11 test hook), so the recipient's
/// checksum must catch it.
async fn send_body_as(
    out: &mut (impl AsyncWrite + Unpin),
    header: Header,
    file: &mut (impl AsyncRead + Unpin),
    mut progress: impl FnMut(u64),
    corrupt: bool,
) -> Result<(), TransferError> {
    write_timed(out, &header.encode()).await?;
    let mut buf = vec![0u8; CHUNK];
    let mut sent = 0u64;
    while sent < header.length {
        let want = buf
            .len()
            .min(usize::try_from(header.length - sent).unwrap_or(usize::MAX));
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
        write_timed(out, &buf[..read]).await?;
        sent += read as u64;
        progress(sent);
    }
    Ok(())
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

/// [`TransferError::Connect`] with the relay's current reason for `server_id`.
fn connect_failed(engine: &Engine, server_id: ServerId) -> TransferError {
    let reason = engine
        .relays()
        .availability_for(server_id, RelayUse::Files)
        .err()
        .unwrap_or(RelayUnavailable::Unreachable);
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
            stream,
        })
    }

    /// Sends the file at `path` (exactly `length` bytes, the offer's size) for share `share_id`
    /// to `session` on `server_id`, through the relay only (FR-7.31, FR-7.32). `progress` gets
    /// the bytes sent so far. Returns once the recipient acknowledged every byte.
    pub async fn file_send(
        &self,
        server_id: ServerId,
        session: u32,
        share_id: &str,
        path: &Path,
        length: u64,
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
        send_body(&mut send, header, &mut file, progress).await?;
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
        send_body(&mut wire, header(200), &mut body.as_slice(), |n| {
            seen.push(n)
        })
        .await
        .expect("sent");
        assert_eq!(wire[..HEADER_LEN], header(200).encode());
        assert_eq!(&wire[HEADER_LEN..], body.as_slice());
        assert_eq!(seen.last(), Some(&200));

        let mut stream = IncomingStream::new(&wire[HEADER_LEN..], 200);
        assert_eq!(read_all(&mut stream).await.expect("read"), body);
        assert_eq!(stream.received(), 200);
    }

    #[tokio::test]
    async fn the_corrupt_hook_flips_the_first_body_byte() {
        let body = [7u8; 20];
        let mut wire = Vec::new();
        send_body_as(&mut wire, header(20), &mut body.as_slice(), |_| {}, true)
            .await
            .expect("sent");
        assert_eq!(wire[HEADER_LEN], 7 ^ 0xFF);
        assert_eq!(&wire[HEADER_LEN + 1..], &body[1..]);
    }

    #[tokio::test]
    async fn a_shorter_file_than_announced_is_a_read_error() {
        let mut wire = Vec::new();
        let err = send_body(&mut wire, header(10), &mut [1u8; 4].as_slice(), |_| {})
            .await
            .expect_err("short file");
        assert!(matches!(err, TransferError::Read(_)));
    }

    #[tokio::test]
    async fn early_end_or_extra_bytes_lose_the_transfer() {
        let mut short = IncomingStream::new(&[1u8; 5][..], 10);
        assert!(matches!(
            read_all(&mut short).await,
            Err(TransferError::ConnectionLost)
        ));
        let mut long = IncomingStream::new(&[1u8; 11][..], 10);
        assert!(matches!(
            read_all(&mut long).await,
            Err(TransferError::ConnectionLost)
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn no_data_for_30_seconds_stalls() {
        let (_writer, reader) = tokio::io::duplex(64);
        let mut stream = IncomingStream::new(reader, 10);
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
        let err = send_body(&mut writer, header(64), &mut body.as_slice(), |_| {})
            .await
            .expect_err("stalled");
        assert!(matches!(err, TransferError::Stalled));
    }
}
