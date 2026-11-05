//! The plugin-data stream to and from one person (FR-9.59 1:1 part).
//!
//! Each side keeps one long-lived one-way QUIC stream ([`STREAM_PLUGIN_DATA`]) per person on
//! the P2P connection, opened through the relay with pair use `plg` when there is none yet. A
//! QUIC stream is reliable and ordered, so messages arrive in order; when a write fails, the
//! message is sent once more on a new stream (possibly a new connection) and the receiver drops
//! what it has seen already by the envelope's sequence (FR-9.62: once, in order per sender).
//!
//! The stream has the lowest QUIC priority of all streams: plugin data waits behind screen
//! frames, images and files (FR-9.60). Voice never shares the connection.

use std::collections::HashMap;
use std::time::Instant;

use mumcord_p2p::quinn::{RecvStream, SendStream};
use tokio::sync::mpsc;

use super::envelope::{self, Envelope};
use super::rate::{DEFAULT_KBPS, Pacer};
use crate::ServerId;
use crate::p2p::{DirectError, STREAM_PLUGIN_DATA};
use crate::peer::RelayUse;
use crate::session::{EngineEvent, WeakEngine};

/// A person on a server; sessions are only unique per server.
pub(super) type Key = (ServerId, u32);

/// QUIC stream priority of plugin data: below files (-1) and screen frames (0).
const PRIORITY: i32 = -2;

/// Encoded envelopes waiting for the stream to one person.
pub(super) type Queue = mpsc::UnboundedSender<Vec<u8>>;

/// What was delivered already, per sender and plugin.
#[derive(Debug, Default)]
pub(super) struct Order {
    last: HashMap<(Key, String), u64>,
}

impl Order {
    /// Whether `seq` from `key` for `plugin` is new; a repeat or an older one is dropped.
    pub(super) fn admit(&mut self, key: Key, plugin: &str, seq: u64) -> bool {
        match self.last.get_mut(&(key, plugin.to_owned())) {
            Some(last) if seq <= *last => false,
            Some(last) => {
                *last = seq;
                true
            }
            None => {
                self.last.insert((key, plugin.to_owned()), seq);
                true
            }
        }
    }

    /// The person left: their next session starts counting anew.
    pub(super) fn forget(&mut self, key: Key) {
        self.last.retain(|(k, _), _| *k != key);
    }
}

/// Writes the queued envelopes to `key` until the queue closes (switch-off or engine end).
pub(super) async fn run_outbox(
    engine: WeakEngine,
    key: Key,
    mut queue: mpsc::UnboundedReceiver<Vec<u8>>,
) {
    let mut stream: Option<SendStream> = None;
    let mut pacer: Option<Pacer> = None;
    while let Some(bytes) = queue.recv().await {
        let Some(kbps) = engine
            .upgrade()
            .map(|e| e.relays().plugin_data_kbps(key.0).unwrap_or(DEFAULT_KBPS))
        else {
            return;
        };
        let now = Instant::now();
        let wait = pacer
            .get_or_insert_with(|| Pacer::new(kbps, now))
            .delay(bytes.len() + 4, now);
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
        // Once on the open stream, once more on a fresh one.
        for _ in 0..2 {
            let open = match stream.as_mut() {
                Some(open) => open,
                None => match open(&engine, key).await {
                    Ok(new) => stream.insert(new),
                    Err(e) => {
                        tracing::debug!(error = %e, "plugin relay data not sent");
                        break;
                    }
                },
            };
            match write(open, &bytes).await {
                Ok(()) => break,
                Err(e) => {
                    tracing::debug!(error = %e, "plugin relay stream failed");
                    stream = None;
                }
            }
        }
    }
    if let Some(mut stream) = stream {
        let _ = stream.finish();
    }
}

async fn open(engine: &WeakEngine, key: Key) -> Result<SendStream, DirectError> {
    let engine = engine.upgrade().ok_or(DirectError::NotConnected)?;
    let conn = engine
        .p2p_relay_connection(key.0, key.1, RelayUse::PluginData)
        .await?;
    let mut send = conn
        .connection()
        .open_uni()
        .await
        .map_err(|e| DirectError::Stream(e.to_string()))?;
    send.set_priority(PRIORITY)
        .map_err(|e| DirectError::Stream(e.to_string()))?;
    send.write_all(&[STREAM_PLUGIN_DATA])
        .await
        .map_err(|e| DirectError::Stream(e.to_string()))?;
    Ok(send)
}

async fn write(stream: &mut SendStream, bytes: &[u8]) -> Result<(), DirectError> {
    let to_err = |e: mumcord_p2p::quinn::WriteError| DirectError::Stream(e.to_string());
    stream
        .write_all(&envelope::length_prefix(bytes))
        .await
        .map_err(to_err)?;
    stream.write_all(bytes).await.map_err(to_err)
}

/// Reads the plugin-data stream `key` opened to us and hands each new message to the app.
pub(crate) async fn accept(engine: WeakEngine, key: Key, mut recv: RecvStream) {
    loop {
        let mut prefix = [0u8; 4];
        if recv.read_exact(&mut prefix).await.is_err() {
            return;
        }
        let Some(len) = envelope::announced_len(prefix) else {
            tracing::debug!("plugin relay stream announced an oversized message");
            let _ = recv.stop(0u32.into());
            return;
        };
        let mut bytes = vec![0u8; len];
        if recv.read_exact(&mut bytes).await.is_err() {
            return;
        }
        let Some(engine) = engine.upgrade() else {
            return;
        };
        let relay = engine.direct().plugin_relay();
        if !relay.enabled() {
            let _ = recv.stop(0u32.into());
            return;
        }
        let Ok(Envelope {
            api_major,
            api_minor,
            seq,
            plugin,
            data,
        }) = envelope::decode(&bytes)
        else {
            tracing::debug!("dropping an invalid plugin relay envelope");
            continue;
        };
        if !relay.admit(key, &plugin, seq) {
            continue;
        }
        if let Ok(ctx) = engine.session_ctx_for(key.0) {
            ctx.events.emit(EngineEvent::PluginRelayData {
                session: key.1,
                plugin,
                api: (api_major, api_minor),
                data: super::Opaque(data),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delivers_each_sequence_once_and_in_order_per_sender_and_plugin() {
        let mut order = Order::default();
        let alice: Key = (1, 5);
        let bob: Key = (1, 6);
        assert!(order.admit(alice, "dice", 1));
        assert!(order.admit(alice, "dice", 2));
        // A resend after a broken stream, and anything older, is dropped.
        assert!(!order.admit(alice, "dice", 2));
        assert!(!order.admit(alice, "dice", 1));
        // Other senders and other plugins count on their own.
        assert!(order.admit(bob, "dice", 1));
        assert!(order.admit(alice, "poll", 1));
        assert!(order.admit(alice, "dice", 3));
        order.forget(alice);
        assert!(order.admit(alice, "dice", 1));
        assert!(!order.admit(bob, "dice", 1));
    }
}
