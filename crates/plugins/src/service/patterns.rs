//! Chat patterns (`chat.patterns`): a matching message from the engine goes to every active
//! plugin whose pattern matches, and only when it was written in the user's current channel on
//! that server (any connected server). See [`crate::patterns`].

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
    /// messages do. `message` is the stored message id, if it was stored: the plugin may then
    /// replace it with a card (`chat.replace`).
    pub fn chat_match(
        &mut self,
        server: ServerId,
        sender: &PeerInfo,
        channel: u32,
        text: &str,
        message: Option<u64>,
    ) {
        self.chat_match_at(server, sender, channel, text, message, Instant::now());
    }

    fn chat_match_at(
        &mut self,
        server: ServerId,
        sender: &PeerInfo,
        channel: u32,
        text: &str,
        message: Option<u64>,
        now: Instant,
    ) {
        if !self.in_my_channel(server, channel) {
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
        let ctx = std::sync::Arc::clone(&self.ctx);
        let keys = ctx.peer_keys.as_ref();
        for (plugin, pattern_index) in matches {
            let event = Event::ChatMatch(ChatMatch {
                sender: wit_peer(keys, &plugin, sender),
                pattern_index,
                text: text.clone(),
            });
            let chat = self.live_chat_ref(&plugin, server, message);
            self.host.send_chat(&plugin, event, chat);
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
    pub(super) fn active_patterns(&self) -> Vec<(String, Vec<String>)> {
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
