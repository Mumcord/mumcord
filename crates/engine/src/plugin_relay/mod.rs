//! Plugin data through the server's relay (spec 09 §4.9, FR-9.57 … FR-9.63), engine side:
//! whether the relay can carry plugin data, and the 1:1 transport over a relay pair session
//! with use `plg` ([`pair`]), and channel sends through one relay broadcast per sender
//! ([`broadcast`]).
//!
//! The plugin service decides what may be sent (permission, reach, size, the plugin's rate,
//! the Mumble fallback) and hands each 1:1 message here. Payloads are opaque bytes: never
//! parsed or logged (FR-9.94).

mod broadcast;
mod envelope;
mod pair;
mod rate;

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::mpsc;

pub use broadcast::PluginBroadcastState;
pub use envelope::MAX_DATA;
pub(crate) use pair::accept;
use pair::{Key, Order, Queue};

use crate::ServerId;
use crate::peer::{RelayUnavailable, RelayUse};
use crate::session::{Engine, lock};

/// Plugin bytes, shown as their length only so they never reach a log (FR-9.94).
#[derive(Clone, PartialEq, Eq)]
pub struct Opaque(pub Vec<u8>);

impl std::fmt::Debug for Opaque {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "<{} bytes>", self.0.len())
    }
}

/// What this side's relay situation allows for plugin data on one server; the plugin service
/// adds the user's switches and reports `relay.status()` in the FR-9.57 order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginRelayAvailability {
    /// No usable relay: none announced, the bot left, untrusted, down or not connected.
    NoRelay,
    /// The relay does not announce `plugin_data` (an older bot never does).
    UseNotAllowed,
    /// The user's P2P mode does not allow the relay, or there is no client certificate.
    NotAllowedHere,
    /// Usable; `kbps` is the relay's per-sender cap when announced (`pk`).
    Available { kbps: Option<u32> },
}

#[derive(Debug, Default)]
pub(crate) struct PluginRelay {
    /// The user switched relay plugin data off (FR-9.63).
    off: AtomicBool,
    state: Mutex<State>,
    broadcasts: Mutex<broadcast::Broadcasts>,
}

#[derive(Debug, Default)]
struct State {
    queues: HashMap<Key, Queue>,
    /// The next sequence per receiver and plugin.
    seqs: HashMap<(Key, String), u64>,
    order: Order,
}

impl State {
    /// Drops the streams and sequences on `server_id`: of one `session`, or of all.
    fn forget(&mut self, server_id: ServerId, session: Option<u32>) {
        let gone = |key: &Key| key.0 == server_id && session.is_none_or(|s| key.1 == s);
        self.queues.retain(|key, _| !gone(key));
        self.seqs.retain(|(key, _), _| !gone(key));
        self.order.forget_where(gone);
    }
}

impl PluginRelay {
    pub(crate) fn enabled(&self) -> bool {
        !self.off.load(Ordering::Relaxed)
    }

    fn admit(&self, key: Key, plugin: &str, seq: u64) -> bool {
        lock(&self.state).order.admit(key, plugin, seq)
    }
}

impl Engine {
    /// Whether the relay of `server_id` can carry plugin data for this user (FR-9.57 without
    /// the global switch and the plugin's permission, which the plugin service checks).
    pub async fn plugin_relay_availability(&self, server_id: ServerId) -> PluginRelayAvailability {
        match self
            .relays()
            .availability_for(server_id, RelayUse::PluginData)
        {
            Ok(()) => {}
            Err(RelayUnavailable::UseNotAllowed(_)) => {
                return PluginRelayAvailability::UseNotAllowed;
            }
            Err(_) => return PluginRelayAvailability::NoRelay,
        }
        let Ok(ctx) = self.session_ctx_for(server_id) else {
            return PluginRelayAvailability::NoRelay;
        };
        let (mode, _) = crate::p2p::mode_and_consent(&ctx.store(), server_id);
        if crate::p2p::relay_only_reveal(mode, Ok(())).is_err() {
            return PluginRelayAvailability::NotAllowedHere;
        }
        match self.stored_identity().await {
            Ok(Some(_)) => PluginRelayAvailability::Available {
                kbps: self.relays().plugin_data_kbps(server_id),
            },
            Ok(None) => PluginRelayAvailability::NotAllowedHere,
            Err(e) => {
                tracing::warn!(error = %e, "identity for relay plugin data not readable");
                PluginRelayAvailability::NotAllowedHere
            }
        }
    }

    /// Queues one plugin message to `session` on `server_id` through the relay pair session
    /// (opened when needed). `api` is the plugin's API major.minor for the envelope. The
    /// caller checked permission, reach, size and rate; a message that cannot be delivered is
    /// dropped with a debug line (no payload).
    pub fn plugin_relay_send(
        &self,
        server_id: ServerId,
        session: u32,
        plugin: &str,
        api: (u8, u8),
        data: Vec<u8>,
    ) {
        let relay = self.direct().plugin_relay();
        if !relay.enabled() {
            return;
        }
        let key = (server_id, session);
        let mut state = lock(&relay.state);
        let seq = state.seqs.entry((key, plugin.to_owned())).or_insert(0);
        *seq += 1;
        let bytes = match envelope::encode(&envelope::Envelope {
            api_major: api.0,
            api_minor: api.1,
            seq: *seq,
            plugin: plugin.to_owned(),
            data,
        }) {
            Ok(bytes) => bytes,
            Err(e) => {
                tracing::debug!(error = %e, "plugin relay message not sent");
                return;
            }
        };
        let queue = state
            .queues
            .get(&key)
            .filter(|q| !q.is_closed())
            .cloned()
            .unwrap_or_else(|| {
                let (tx, rx) = mpsc::unbounded_channel();
                self.runtime()
                    .spawn(pair::run_outbox(self.downgrade(), key, rx));
                state.queues.insert(key, tx.clone());
                tx
            });
        drop(state);
        if queue.send(bytes).is_err() {
            tracing::debug!("plugin relay queue closed");
        }
    }

    /// The global switch "Let plugins use a server's relay" (FR-9.63). Off ends every
    /// plugin-data stream at once; incoming ones are stopped as their next message arrives.
    pub fn plugin_relay_set_enabled(&self, enabled: bool) {
        let relay = self.direct().plugin_relay();
        relay.off.store(!enabled, Ordering::Relaxed);
        if !enabled {
            let mut state = lock(&relay.state);
            // Dropping the senders ends each stream after what is already queued. The
            // sequences keep counting: receivers remember the last one they delivered.
            state.queues.clear();
            state.order = Order::default();
            drop(state);
            self.end_plugin_broadcasts(None);
        }
    }

    /// `session` left `server_id`: its 1:1 stream and sequence state go, so its next session
    /// starts counting anew on both ends.
    pub(crate) fn plugin_relay_forget(&self, server_id: ServerId, session: u32) {
        lock(&self.direct().plugin_relay().state).forget(server_id, Some(session));
    }

    /// The connection to `server_id` (re)synced or ended: every session there is new, so the
    /// 1:1 streams and sequences of the old ones go. Otherwise a person who sent to us before
    /// would have their first messages to our new session dropped as repeats.
    pub(crate) fn plugin_relay_pairs_reset(&self, server_id: ServerId) {
        lock(&self.direct().plugin_relay().state).forget(server_id, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_connection_accepts_a_sender_counting_from_one_again() {
        let mut state = State::default();
        let alice = (1, 5);
        assert!(state.order.admit(alice, "dice", 1));
        assert!(!state.order.admit(alice, "dice", 1), "a repeat is dropped");
        state.forget(2, None);
        assert!(
            !state.order.admit(alice, "dice", 1),
            "another server keeps its order"
        );
        state.forget(1, None);
        assert!(state.order.admit(alice, "dice", 1));
    }

    #[test]
    fn a_person_who_left_starts_counting_anew() {
        let mut state = State::default();
        let (bob, carol) = ((1, 6), (1, 7));
        *state.seqs.entry((bob, "dice".into())).or_insert(0) += 3;
        *state.seqs.entry((carol, "dice".into())).or_insert(0) += 2;
        assert!(state.order.admit(bob, "dice", 3));
        state.forget(1, Some(6));
        assert!(!state.seqs.contains_key(&(bob, "dice".into())));
        assert_eq!(state.seqs.get(&(carol, "dice".into())), Some(&2));
        assert!(state.order.admit(bob, "dice", 1));
    }
}
