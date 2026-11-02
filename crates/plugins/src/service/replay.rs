//! Chat messages a plugin may replace with a card (FR-6.45): the opaque message token that goes
//! with a live `chat-match` / `chat-trigger`, and the render-only replay of stored history, so
//! old messages (also ones posted before the plugin was installed) get their cards too.
//!
//! A replay goes only to plugins that import `chat.is-replay`; older plugins could not tell it
//! from a live message and would count a vote twice. It is bounded: the app hands in at most a
//! page of stored lines, each plugin gets at most [`REPLAY_PER_PLUGIN`] events per call, and a
//! message a plugin already saw (live or replayed) is never sent to it again.

use std::collections::{HashMap, HashSet, VecDeque};

use mumcord_store::ServerId;
use regex::Regex;

use super::PluginService;
use super::plugin_io::{PeerInfo, wit_peer};
use crate::chat::{self, ChatRef, OWN_LINE};
use crate::host::RunState;
use crate::patterns::{clip_text, compile};
use crate::triggers::clip_args;
use crate::wit::events::{ChatMatch, ChatTrigger, Event};

/// Most replayed events one plugin gets per replay call.
pub const REPLAY_PER_PLUGIN: usize = 20;
/// Messages remembered as already seen, over all plugins; the oldest go first.
const MAX_SEEN: usize = 4096;
const SEND_PERMISSION: &str = "chat.send";

/// A stored channel message the engine let through for a replay (pattern match, trigger shape or
/// a "[Name] " prefix). The sender may have left: session 0.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredLine {
    pub message: u64,
    pub sender: PeerInfo,
    /// Plain text with `\n` line breaks.
    pub text: String,
    /// `(trigger word, args)` if the message has the trigger shape.
    pub trigger: Option<(String, String)>,
}

/// Which (plugin, server, message) pairs were already delivered.
#[derive(Debug, Default)]
pub(super) struct Seen {
    set: HashSet<(String, ServerId, u64)>,
    order: VecDeque<(String, ServerId, u64)>,
}

impl Seen {
    fn contains(&self, plugin: &str, server: ServerId, message: u64) -> bool {
        self.set.contains(&(plugin.to_owned(), server, message))
    }

    fn insert(&mut self, plugin: &str, server: ServerId, message: u64) {
        let key = (plugin.to_owned(), server, message);
        if !self.set.insert(key.clone()) {
            return;
        }
        self.order.push_back(key);
        if self.order.len() > MAX_SEEN
            && let Some(oldest) = self.order.pop_front()
        {
            self.set.remove(&oldest);
        }
    }
}

/// What a plugin is sent for one stored line.
struct Planned {
    plugin: String,
    message: u64,
    event: Event,
}

impl PluginService {
    /// Whether `channel` is the user's current channel on `server`, any connected server: the
    /// only chat plugins ever see.
    pub(super) fn in_my_channel(&self, server: ServerId, channel: u32) -> bool {
        self.io
            .peers()
            .connections
            .get(&server)
            .and_then(|c| c.me.as_ref())
            .and_then(|me| me.channel.as_ref())
            .is_some_and(|(id, _)| *id == channel)
    }

    /// The chat reference for a live event: a token when the message was stored. The message
    /// counts as seen, so a later replay does not send it again.
    pub(super) fn live_chat_ref(
        &mut self,
        plugin: &str,
        server: ServerId,
        message: Option<u64>,
    ) -> ChatRef {
        let token = message.map(|message| {
            self.replayed.insert(plugin, server, message);
            self.io.issue_token(plugin, server, message)
        });
        ChatRef {
            token,
            replay: false,
            server: Some(server),
        }
    }

    /// Replays stored messages of the user's current channel (`lines`, oldest first) to the
    /// plugins that would have got them live: pattern matches, trigger lines to the word's owner,
    /// and each plugin's own "[Name] " lines (as `chat-match` with `pattern-index` 255).
    pub fn replay_chat(&mut self, server: ServerId, channel: u32, lines: &[StoredLine]) {
        if lines.is_empty() || !self.in_my_channel(server, channel) {
            return;
        }
        let planned = self.plan_replay(server, lines);
        for Planned {
            plugin,
            message,
            event,
        } in planned
        {
            let token = self.io.issue_token(&plugin, server, message);
            if self.host.send_chat(
                &plugin,
                event,
                ChatRef {
                    token: Some(token),
                    replay: true,
                    server: Some(server),
                },
            ) {
                self.replayed.insert(&plugin, server, message);
            }
        }
    }

    /// Newest lines first so the cap keeps the most recent ones; returned oldest first.
    fn plan_replay(&self, server: ServerId, lines: &[StoredLine]) -> Vec<Planned> {
        let patterns: Vec<(String, Vec<Regex>)> = self
            .active_patterns()
            .into_iter()
            .filter(|(plugin, _)| self.host.takes_replays(plugin))
            .map(|(plugin, sources)| {
                let compiled = sources.iter().filter_map(|p| compile(p).ok()).collect();
                (plugin, compiled)
            })
            .collect();
        let posters = self.replay_posters();
        let mut owners: HashMap<String, Option<String>> = HashMap::new();
        let mut counts: HashMap<String, usize> = HashMap::new();
        let keys = self.ctx.peer_keys.as_ref();
        let mut planned = Vec::new();
        for line in lines.iter().rev() {
            let mut candidates: Vec<(String, Event)> = Vec::new();
            let sender = |plugin: &str| wit_peer(keys, plugin, &line.sender);
            for (plugin, regexes) in &patterns {
                if let Some(index) = regexes.iter().position(|re| re.is_match(&line.text))
                    && let Ok(index) = u8::try_from(index)
                {
                    let event = match_event(sender(plugin), index, &line.text);
                    candidates.push((plugin.clone(), event));
                }
            }
            if let Some((word, args)) = &line.trigger {
                let owner = owners
                    .entry(word.clone())
                    .or_insert_with(|| self.trigger_owner(word))
                    .clone();
                if let Some(owner) = owner.filter(|o| self.host.takes_replays(o)) {
                    let event = Event::ChatTrigger(ChatTrigger {
                        sender: sender(&owner),
                        trigger: word.clone(),
                        args: clip_args(args),
                    });
                    candidates.push((owner, event));
                }
            }
            if let Some(name) = chat::poster(&line.text) {
                for (plugin, _) in posters.iter().filter(|(_, n)| n == name) {
                    let event = match_event(sender(plugin), OWN_LINE, &line.text);
                    candidates.push((plugin.clone(), event));
                }
            }
            for (plugin, event) in candidates {
                let count = counts.entry(plugin.clone()).or_default();
                if *count >= REPLAY_PER_PLUGIN
                    || self.replayed.contains(&plugin, server, line.message)
                {
                    continue;
                }
                *count += 1;
                planned.push(Planned {
                    plugin,
                    message: line.message,
                    event,
                });
            }
        }
        planned.reverse();
        planned
    }

    /// Running, enabled plugins with `chat.send` that take replays, with their display names
    /// (the "[Name] " prefix of their lines).
    fn replay_posters(&self) -> Vec<(String, String)> {
        let records = match self.store().plugins() {
            Ok(records) => records,
            Err(e) => {
                tracing::warn!(error = %e, "reading plugins for a chat replay failed");
                return Vec::new();
            }
        };
        records
            .iter()
            .filter(|r| {
                r.removed_at.is_none()
                    && r.enabled
                    && r.granted.iter().any(|p| p == SEND_PERMISSION)
                    && matches!(self.host.state(&r.id), Some(RunState::Running))
                    && self.host.takes_replays(&r.id)
            })
            .filter_map(|r| {
                let name = self.host.manifest(&r.id)?.name.clone();
                Some((r.id.clone(), name))
            })
            .collect()
    }
}

fn match_event(sender: crate::wit::types::Peer, pattern_index: u8, text: &str) -> Event {
    Event::ChatMatch(ChatMatch {
        sender,
        pattern_index,
        text: clip_text(text),
    })
}
