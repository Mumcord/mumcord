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
use super::{originals, setting};
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
            connect_timeout: Duration::from_secs(3),
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
}
