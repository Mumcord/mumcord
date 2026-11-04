//! Full images over a direct connection (FR-2.37): one stream per image, opened by the
//! fetching side with the kind [`crate::p2p::STREAM_FULL_IMAGE`].
//!
//! Request: the image's SHA-256 (32 bytes). Reply: [`FOUND`] and the size (4 bytes, big
//! endian) followed by the image, or [`UNAVAILABLE`]. The same rules as for plugin data apply:
//! only own images are served while "Offer my full-size images" is on, and the received bytes
//! must match the announced size and hash.

use std::time::Duration;

use mumcord_p2p::quinn::{RecvStream, SendStream};

use super::check::MAX_BYTES;
use super::wire::FullRef;
use super::{DirectOutcome, originals, setting};
use crate::session::Engine;

const FOUND: u8 = 0;
const UNAVAILABLE: u8 = 1;
/// How long one read may stall; a direct path is fast, so a stall means it broke.
const STALL: Duration = Duration::from_secs(15);
/// Bytes between two progress reports.
const PROGRESS_STEP: usize = 32 * 1024;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub(crate) enum DirectFetchError {
    #[error("the sender does not have the image")]
    Unavailable,
    #[error("the direct transfer broke: {0}")]
    Broken(String),
    #[error("the data does not match the announced image")]
    Mismatch,
}

fn broken(e: impl std::fmt::Display) -> DirectFetchError {
    DirectFetchError::Broken(e.to_string())
}

async fn read_exact(recv: &mut RecvStream, buf: &mut [u8]) -> Result<(), DirectFetchError> {
    tokio::time::timeout(STALL, recv.read_exact(buf))
        .await
        .map_err(broken)?
        .map_err(broken)
}

/// Fetches `full` over the stream; reports `(received, total)` while loading.
pub(crate) async fn fetch(
    mut send: SendStream,
    mut recv: RecvStream,
    full: FullRef,
    progress: impl Fn(u32, u32),
) -> Result<Vec<u8>, DirectFetchError> {
    send.write_all(&full.hash).await.map_err(broken)?;
    send.finish().map_err(broken)?;
    let mut status = [0u8; 1];
    read_exact(&mut recv, &mut status).await?;
    match status[0] {
        FOUND => {}
        UNAVAILABLE => return Err(DirectFetchError::Unavailable),
        other => return Err(broken(format!("unknown status {other}"))),
    }
    let mut size = [0u8; 4];
    read_exact(&mut recv, &mut size).await?;
    let size = u32::from_be_bytes(size);
    if size != full.size {
        return Err(DirectFetchError::Mismatch);
    }
    let total = usize::try_from(size).map_err(broken)?;
    let mut bytes = vec![0u8; total];
    let mut received = 0;
    let mut reported = 0;
    while received < total {
        let read = tokio::time::timeout(STALL, recv.read(&mut bytes[received..]))
            .await
            .map_err(broken)?
            .map_err(broken)?;
        let Some(read) = read else {
            return Err(broken("stream ended early"));
        };
        received += read;
        if received - reported >= PROGRESS_STEP || received == total {
            reported = received;
            progress(u32::try_from(received).unwrap_or(size), size);
        }
    }
    if !full.matches(&bytes) {
        return Err(DirectFetchError::Mismatch);
    }
    Ok(bytes)
}

/// Answers one request on a stream the other side opened.
pub(crate) async fn serve(engine: &Engine, mut send: SendStream, mut recv: RecvStream) {
    let mut hash = [0u8; 32];
    if let Err(e) = read_exact(&mut recv, &mut hash).await {
        tracing::debug!(error = %e, "direct full image request incomplete");
        return;
    }
    let lookup = originals(
        engine.link_previews_arc(),
        setting(engine.settings(), |s| s.offer_full_images),
    );
    let image = tokio::task::spawn_blocking(move || lookup(&hash))
        .await
        .ok()
        .flatten()
        .filter(|bytes| bytes.len() <= MAX_BYTES);
    let result = match image.and_then(|bytes| Some((u32::try_from(bytes.len()).ok()?, bytes))) {
        Some((size, bytes)) => {
            let mut header = [FOUND, 0, 0, 0, 0];
            header[1..].copy_from_slice(&size.to_be_bytes());
            match send.write_all(&header).await {
                Ok(()) => send.write_all(&bytes).await,
                Err(e) => Err(e),
            }
        }
        None => send.write_all(&[UNAVAILABLE]).await,
    };
    match result
        .map_err(broken)
        .and_then(|()| send.finish().map_err(broken))
    {
        Ok(()) => tracing::debug!("full image request answered directly"),
        Err(e) => tracing::debug!(error = %e, "direct full image reply not sent"),
    }
}

/// Fetches `full` with `open` (a new stream on the direct connection) and falls back when no
/// stream can be opened. A connection replaced mid-fetch (a file moved it to the relay) breaks
/// the stream while a new connection is already open: one more try on that one, but only while
/// `connected` says a connection exists.
pub(super) async fn fetch_retrying<O, F>(
    mut open: O,
    connected: impl Fn() -> bool,
    full: FullRef,
    progress: impl Fn(u32, u32),
) -> DirectOutcome
where
    O: FnMut() -> F,
    F: std::future::Future<Output = Result<(SendStream, RecvStream), crate::p2p::DirectError>>,
{
    let mut retried = false;
    loop {
        let (send, recv) = match open().await {
            Ok(stream) => stream,
            Err(e) => {
                tracing::info!(error = %e, "no direct connection; full image over plugin data");
                return DirectOutcome::Fallback;
            }
        };
        match fetch(send, recv, full, &progress).await {
            Ok(bytes) => {
                tracing::debug!(bytes = bytes.len(), "full image loaded directly");
                return DirectOutcome::Loaded(bytes);
            }
            Err(DirectFetchError::Unavailable) => return DirectOutcome::Stop,
            Err(e) if !retried && connected() => {
                tracing::info!(error = %e, "direct full image broke; trying once more");
                retried = true;
            }
            Err(e) => {
                tracing::info!(error = %e, "direct full image failed; trying plugin data");
                return DirectOutcome::Fallback;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use mumcord_p2p::{P2p, P2pConfig, PeerConnection, Signal};
    use mumcord_protocol::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
    use mumcord_protocol::{ClientIdentity, cert_hash};

    use super::*;

    fn identity() -> (ClientIdentity, String) {
        let generated = rcgen::generate_simple_self_signed(vec!["test".to_owned()]).expect("cert");
        let hash = cert_hash(generated.cert.der());
        let identity = ClientIdentity {
            chain: vec![CertificateDer::clone(generated.cert.der())],
            key: PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
                generated.signing_key.serialize_der(),
            )),
        };
        (identity, hash)
    }

    async fn connected() -> (PeerConnection, PeerConnection) {
        let config = P2pConfig {
            // Only an upper bound (a connection returns as soon as it is up). Generous because
            // the whole lib suite runs in parallel on loaded CI runners, where a 3 s budget
            // for the debug-build QUIC handshake ended in a spurious `NoDirectPath`.
            connect_timeout: Duration::from_secs(10),
            port_mapping: false,
            allow_loopback: true,
        };
        let carry = |s: &Signal| Signal::decode(&s.encode().expect("encode")).expect("decode");
        let ((a, a_hash), (b, b_hash)) = (identity(), identity());
        let offer = P2p::new(&a, config.clone())
            .offer(&b_hash)
            .await
            .expect("offer");
        let answer = P2p::new(&b, config)
            .answer(&carry(offer.signal()), &a_hash)
            .await
            .expect("answer");
        let reply = carry(answer.signal());
        let (a, b) = tokio::join!(offer.complete(&reply), answer.connect());
        (a.expect("a"), b.expect("b"))
    }

    /// Fetches `full` while the other side answers with `reply` after reading the request.
    async fn fetch_with_reply(full: FullRef, reply: Vec<u8>) -> Result<Vec<u8>, DirectFetchError> {
        let (a, b) = connected().await;
        let server = tokio::spawn(async move {
            let (mut send, mut recv) = b.connection().accept_bi().await.expect("accept");
            let mut hash = [0u8; 32];
            recv.read_exact(&mut hash).await.expect("request");
            send.write_all(&reply).await.expect("reply");
            send.finish().expect("finish");
            send.stopped().await.ok();
            hash
        });
        let (send, recv) = a.connection().open_bi().await.expect("open");
        let progress = std::sync::Mutex::new(Vec::new());
        let result = fetch(send, recv, full, |received, total| {
            progress.lock().expect("lock").push((received, total));
        })
        .await;
        assert_eq!(
            server.await.expect("server"),
            full.hash,
            "asks for the hash"
        );
        if result.is_ok() {
            assert_eq!(
                progress.lock().expect("lock").last(),
                Some(&(full.size, full.size))
            );
        }
        result
    }

    fn found(bytes: &[u8]) -> Vec<u8> {
        let mut reply = vec![FOUND];
        reply.extend_from_slice(&u32::try_from(bytes.len()).expect("size").to_be_bytes());
        reply.extend_from_slice(bytes);
        reply
    }

    #[tokio::test]
    async fn image_arrives_and_is_checked() {
        let image: Vec<u8> = (0..100_000u32).map(|i| (i % 251) as u8).collect();
        let full = FullRef::of(&image);
        assert_eq!(
            fetch_with_reply(full, found(&image)).await,
            Ok(image.clone())
        );

        let mut other = image.clone();
        other[10] ^= 1;
        assert_eq!(
            fetch_with_reply(full, found(&other)).await,
            Err(DirectFetchError::Mismatch)
        );
        assert_eq!(
            fetch_with_reply(full, found(&image[..10])).await,
            Err(DirectFetchError::Mismatch),
            "a different size is refused before reading"
        );
        assert_eq!(
            fetch_with_reply(full, vec![UNAVAILABLE]).await,
            Err(DirectFetchError::Unavailable)
        );
        assert!(matches!(
            fetch_with_reply(full, found(&image)[..500].to_vec()).await,
            Err(DirectFetchError::Broken(_))
        ));
    }

    /// Reads the kind byte and the request, then answers with `reply`.
    async fn serve_after_kind(b: &PeerConnection, reply: Vec<u8>) {
        let (mut send, mut recv) = b.connection().accept_bi().await.expect("accept");
        let mut kind = [0u8; 1];
        recv.read_exact(&mut kind).await.expect("kind");
        let mut hash = [0u8; 32];
        recv.read_exact(&mut hash).await.expect("request");
        send.write_all(&reply).await.expect("reply");
        send.finish().expect("finish");
        send.stopped().await.ok();
    }

    async fn run_retrying(
        a: &PeerConnection,
        connected: bool,
        full: FullRef,
    ) -> (DirectOutcome, usize) {
        let opened = std::sync::atomic::AtomicUsize::new(0);
        let outcome = fetch_retrying(
            || {
                opened.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let connection = a.connection().clone();
                async move {
                    let (mut send, recv) = connection
                        .open_bi()
                        .await
                        .map_err(|e| crate::p2p::DirectError::Stream(e.to_string()))?;
                    // Opening a stream sends nothing until written to; the kind byte makes
                    // the server's accept_bi return, as the real opener does.
                    send.write_all(&[crate::p2p::STREAM_FULL_IMAGE])
                        .await
                        .map_err(|e| crate::p2p::DirectError::Stream(e.to_string()))?;
                    Ok((send, recv))
                }
            },
            || connected,
            full,
            |_, _| {},
        )
        .await;
        (outcome, opened.load(std::sync::atomic::Ordering::SeqCst))
    }

    /// F3a(d): a stream that breaks while the connection is still open is retried once on it.
    #[tokio::test]
    async fn broken_stream_is_retried_once_while_connected() {
        let image: Vec<u8> = (0..50_000u32).map(|i| (i % 251) as u8).collect();
        let full = FullRef::of(&image);
        let (a, b) = connected().await;
        let server = tokio::spawn(async move {
            let mut kind = [0u8; 1];
            for reply in [found(&image)[..500].to_vec(), found(&image)] {
                let (mut send, mut recv) = b.connection().accept_bi().await.expect("accept");
                recv.read_exact(&mut kind).await.expect("kind");
                let mut hash = [0u8; 32];
                recv.read_exact(&mut hash).await.expect("request");
                send.write_all(&reply).await.expect("reply");
                send.finish().expect("finish");
                send.stopped().await.ok();
            }
            image
        });
        let (outcome, opened) = run_retrying(&a, true, full).await;
        let image = server.await.expect("server");
        assert!(matches!(outcome, DirectOutcome::Loaded(bytes) if bytes == image));
        assert_eq!(opened, 2, "one failed try and one retry");
    }

    /// F3a(d): without an open connection, or after the retry broke too, plugin data takes over.
    #[tokio::test]
    async fn retry_is_once_and_needs_a_connection() {
        let image: Vec<u8> = (0..50_000u32).map(|i| (i % 251) as u8).collect();
        let full = FullRef::of(&image);
        let cut = found(&image)[..500].to_vec();

        let (a, b) = connected().await;
        let server = tokio::spawn({
            let cut = cut.clone();
            async move {
                for _ in 0..2 {
                    serve_after_kind(&b, cut.clone()).await;
                }
            }
        });
        let (outcome, opened) = run_retrying(&a, true, full).await;
        server.await.expect("server");
        assert!(matches!(outcome, DirectOutcome::Fallback));
        assert_eq!(opened, 2, "never a third try");

        let (a, b) = connected().await;
        let server = tokio::spawn(async move { serve_after_kind(&b, cut).await });
        let (outcome, opened) = run_retrying(&a, false, full).await;
        server.await.expect("server");
        assert!(matches!(outcome, DirectOutcome::Fallback));
        assert_eq!(opened, 1, "no retry without a connection");
    }
}
