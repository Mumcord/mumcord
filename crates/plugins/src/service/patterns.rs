//! Chat patterns (`chat.patterns`): a matching message from the engine goes to every active
//! plugin whose pattern matches, and only when it was written in the user's current channel on
//! the focused server. See [`crate::patterns`].

use std::time::Instant;

use mumcord_store::ServerId;
use regex::RegexSet;

use super::PluginService;
use super::plugin_io::{PeerInfo, wit_peer};
use crate::host::RunState;
use crate::patterns::{clip_text, first_match, pattern_set};
use crate::wit::events::{ChatMatch, Event};

const PATTERNS_PERMISSION: &str = "chat.patterns";

impl PluginService {
    /// A channel message from `sender` in `channel` whose plain text matched an active pattern
    /// in the engine. Blocked senders never get here (the engine drops them); the user's own
    /// messages do.
    pub fn chat_match(&mut self, server: ServerId, sender: &PeerInfo, channel: u32, text: &str) {
        self.chat_match_at(server, sender, channel, text, Instant::now());
    }

    fn chat_match_at(
        &mut self,
        server: ServerId,
        sender: &PeerInfo,
        channel: u32,
        text: &str,
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
        if !in_my_channel {
            return;
        }
        let matches: Vec<(String, u8)> = self
            .active_patterns()
            .into_iter()
            .filter_map(|(plugin, patterns)| Some((plugin, first_match(&patterns, text)?)))
            .collect();
        if matches.is_empty() || !self.match_limit.allow((server, sender.session), now) {
            return;
        }
        let text = clip_text(text);
        let keys = self.ctx.peer_keys.as_ref();
        for (plugin, pattern_index) in matches {
            let event = Event::ChatMatch(ChatMatch {
                sender: wit_peer(keys, &plugin, sender),
                pattern_index,
                text: text.clone(),
            });
            self.host.send(&plugin, event);
        }
    }

    /// Every active pattern in one set, for the engine's filter (`None`: no plugin listens).
    /// The app pushes it to the engine whenever the plugin list or a plugin's state changes.
    pub fn chat_pattern_set(&self) -> Option<RegexSet> {
        let active = self.active_patterns();
        pattern_set(
            active
                .iter()
                .flat_map(|(_, p)| p.iter().map(String::as_str)),
        )
    }

    /// The running, enabled plugins with `chat.patterns` and their pattern sources. Rebuilt per
    /// message like the trigger owners, so it never goes stale.
    fn active_patterns(&self) -> Vec<(String, Vec<String>)> {
        let records = match self.store().plugins() {
            Ok(records) => records,
            Err(e) => {
                tracing::warn!(error = %e, "reading plugins for chat patterns failed");
                return Vec::new();
            }
        };
        records
            .iter()
            .filter(|r| {
                r.removed_at.is_none()
                    && r.enabled
                    && r.granted.iter().any(|p| p == PATTERNS_PERMISSION)
                    && matches!(self.host.state(&r.id), Some(RunState::Running))
            })
            .filter_map(|r| {
                let manifest = self.host.manifest(&r.id)?;
                let patterns: Vec<String> = manifest
                    .chat_patterns
                    .iter()
                    .map(|p| p.pattern.clone())
                    .collect();
                (!patterns.is_empty()).then(|| (r.id.clone(), patterns))
            })
            .collect()
    }
}
