//! Chat triggers (`chat.triggers`): a trigger line from the engine goes to the one plugin that
//! owns the word, and only when it was written in the user's current channel on the focused
//! server. See [`crate::triggers`].

use std::time::Instant;

use mumcord_store::ServerId;

use super::PluginService;
use super::plugin_io::{PeerInfo, wit_peer};
use crate::host::RunState;
use crate::manifest::is_trigger;
use crate::triggers::{Candidate, clip_args, owner};
use crate::wit::events::{ChatTrigger, Event};

const TRIGGERS_PERMISSION: &str = "chat.triggers";

impl PluginService {
    /// A channel message of the trigger shape (`!word args`) from `sender` in `channel`.
    /// Blocked senders never get here (the engine drops them); the user's own lines do.
    pub fn chat_trigger(
        &mut self,
        server: ServerId,
        sender: &PeerInfo,
        channel: u32,
        trigger: &str,
        args: &str,
    ) {
        self.chat_trigger_at(server, sender, channel, trigger, args, Instant::now());
    }

    fn chat_trigger_at(
        &mut self,
        server: ServerId,
        sender: &PeerInfo,
        channel: u32,
        trigger: &str,
        args: &str,
        now: Instant,
    ) {
        let in_my_channel = {
            let state = self.io.peers();
            state.focused == Some(server)
                && state
                    .connections
                    .get(&server)
                    .and_then(|c| c.me.as_ref())
                    .and_then(|me| me.channel.as_ref())
                    .is_some_and(|(id, _)| *id == channel)
        };
        if !in_my_channel || !is_trigger(trigger) {
            return;
        }
        let Some(plugin) = self.trigger_owner(trigger) else {
            return;
        };
        if !self.trigger_limit.allow((server, sender.session), now) {
            return;
        }
        let keys = self.ctx.peer_keys.as_ref();
        let event = Event::ChatTrigger(ChatTrigger {
            sender: wit_peer(keys, &plugin, sender),
            trigger: trigger.to_owned(),
            args: clip_args(args),
        });
        self.host.send(&plugin, event);
    }

    /// The running, enabled plugin with `chat.triggers` that owns `trigger` (first installed
    /// wins). Rebuilt per line like the slash command registry, so it never goes stale.
    fn trigger_owner(&self, trigger: &str) -> Option<String> {
        let records = match self.store().plugins() {
            Ok(records) => records,
            Err(e) => {
                tracing::warn!(error = %e, "reading plugins for a chat trigger failed");
                return None;
            }
        };
        let manifests: Vec<_> = records
            .iter()
            .filter(|r| r.removed_at.is_none())
            .filter_map(|r| Some((r, self.host.manifest(&r.id)?)))
            .collect();
        let candidates: Vec<Candidate<'_>> = manifests
            .iter()
            .map(|(record, manifest)| Candidate {
                plugin: &record.id,
                installed_at: record.installed_at,
                active: record.enabled
                    && record.granted.iter().any(|p| p == TRIGGERS_PERMISSION)
                    && matches!(self.host.state(&record.id), Some(RunState::Running)),
                triggers: &manifest.triggers,
            })
            .collect();
        owner(&candidates, trigger).map(str::to_owned)
    }
}
