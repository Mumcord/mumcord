//! The private messages overview (issue #173): the newest message of every stored private
//! conversation on a server, and pinning and muting conversations. Pin and mute state is saved
//! per server and person; the UI applies it (sort order, grey counts, no notifications).

use mumcord_store::{MUTED_FOREVER, PmConversationPrefs, ServerId, UnixMillis};

use crate::chat::{ChatMessageView, history_view, now_millis};
use crate::session::{ConversationRef, Engine, EngineEvent, EngineResult, lock, store_error};

/// The newest stored message of a private conversation, with the name the other person last
/// wrote under (`None` when only own messages are stored).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivateConversationView {
    pub last: ChatMessageView,
    pub peer_name: Option<String>,
}

const MINUTE_MS: UnixMillis = 60_000;

/// End of a mute of `minutes` from `now`; `None` mutes until it is turned back on.
fn mute_until(now: UnixMillis, minutes: Option<u32>) -> UnixMillis {
    match minutes {
        Some(m) => now.saturating_add(UnixMillis::from(m).saturating_mul(MINUTE_MS)),
        None => MUTED_FOREVER,
    }
}

impl Engine {
    /// The newest stored message of every private conversation on a connected server, newest
    /// first. Conversations of this session only (people without a certificate) are not stored.
    pub fn private_conversations(
        &self,
        server_id: ServerId,
    ) -> EngineResult<Vec<PrivateConversationView>> {
        let ctx = self.session_ctx_for(server_id)?;
        let rows = ctx
            .store()
            .private_conversation_summaries(ctx.server_id)
            .map_err(store_error)?;
        let own_hash = ctx
            .handle
            .with_state(|s| s.own_user().and_then(|u| u.cert_hash.clone()));
        Ok(rows
            .into_iter()
            .filter_map(|row| {
                let mumcord_store::Conversation::Private(cert_hash) = &row.last.conversation else {
                    return None;
                };
                let conversation = ConversationRef::Private {
                    cert_hash: cert_hash.clone(),
                };
                Some(PrivateConversationView {
                    last: history_view(
                        row.last,
                        &conversation,
                        own_hash.as_deref(),
                        Vec::new(),
                        None,
                    ),
                    peer_name: row.peer_name,
                })
            })
            .collect())
    }

    /// Pin and mute state of every private conversation that has any.
    pub fn pm_conversation_prefs(&self) -> EngineResult<Vec<PmConversationPrefs>> {
        lock(&self.store_handle())
            .pm_conversation_prefs()
            .map_err(store_error)
    }

    /// Pins a private conversation to the top of the overview, or unpins it.
    pub fn pm_conversation_set_pinned(
        &self,
        server_id: ServerId,
        cert_hash: &str,
        pinned: bool,
    ) -> EngineResult<()> {
        self.change_prefs(|store| store.set_pm_conversation_pinned(server_id, cert_hash, pinned))
    }

    /// Mutes a private conversation for `minutes`, or until turned back on (`None`).
    pub fn pm_conversation_mute(
        &self,
        server_id: ServerId,
        cert_hash: &str,
        minutes: Option<u32>,
    ) -> EngineResult<()> {
        let until = mute_until(now_millis(), minutes);
        self.change_prefs(|store| {
            store.set_pm_conversation_muted(server_id, cert_hash, Some(until))
        })
    }

    /// Turns notifications of a muted private conversation back on.
    pub fn pm_conversation_unmute(&self, server_id: ServerId, cert_hash: &str) -> EngineResult<()> {
        self.change_prefs(|store| store.set_pm_conversation_muted(server_id, cert_hash, None))
    }

    fn change_prefs(
        &self,
        change: impl FnOnce(&mumcord_store::Store) -> mumcord_store::Result<()>,
    ) -> EngineResult<()> {
        let list = {
            let handle = self.store_handle();
            let store = lock(&handle);
            change(&store).map_err(store_error)?;
            store.pm_conversation_prefs().map_err(store_error)?
        };
        self.events()
            .emit(EngineEvent::PmConversationPrefsChanged(list));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mute_ends_after_the_minutes_or_never() {
        assert_eq!(mute_until(1_000, Some(60)), 1_000 + 60 * 60_000);
        assert_eq!(mute_until(1_000, Some(8 * 60)), 1_000 + 8 * 60 * 60_000);
        assert_eq!(mute_until(1_000, None), MUTED_FOREVER);
        assert_eq!(mute_until(UnixMillis::MAX - 1, Some(1)), UnixMillis::MAX);
    }
}
