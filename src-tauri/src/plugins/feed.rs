//! Engine ↔ plugin service: engine plugin events in, plugin messages, announces and chat
//! lines out.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use mumcord_engine::plugins::{PluginEngineEvent, PluginPeer};
use mumcord_engine::{ChatMessageView, ConversationRef, Engine, ServerId};
use mumcord_plugins::messaging::Outgoing;
use mumcord_plugins::messaging::announce::ANNOUNCE_DATA_ID;
use mumcord_plugins::service::{BotInfo, PeerInfo, PluginService, SelfInfo};
use mumcord_protocol::LowOutcome;

use tauri::AppHandle;
use tauri_specta::Event;

use crate::commands::types::to_api_server_id;

use super::{Plugins, suggestions};

/// Engine events handled per service call; a burst (e.g. joining a full channel) takes the
/// service lock once.
const BATCH: usize = 256;
/// How often due announces are checked at most.
const ANNOUNCE_POLL: Duration = Duration::from_secs(1);

/// Feeds the engine's plugin events into the service.
pub fn pump_engine_events(app: &AppHandle, plugins: &Arc<Plugins>, engine: &Engine) {
    let Some(mut events) = engine.plugin_events() else {
        tracing::error!("the plugin event stream is already taken");
        return;
    };
    let app = app.clone();
    let plugins = Arc::clone(plugins);
    let engine = engine.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(first) = events.recv().await {
            let mut batch = vec![first];
            while batch.len() < BATCH
                && let Ok(event) = events.try_recv()
            {
                batch.push(event);
            }
            for event in &batch {
                if let PluginEngineEvent::SharedState {
                    server_id,
                    supported,
                } = event
                {
                    let event = super::PluginsSharedState {
                        server_id: to_api_server_id(*server_id),
                        supported: *supported,
                    };
                    if let Err(e) = event.emit(&app) {
                        tracing::warn!(error = %e, "emitting the shared plugin state failed");
                    }
                }
            }
            // Server suggestions go to the notification centre, not to the plugins.
            let (announced, batch): (Vec<_>, Vec<_>) = batch
                .into_iter()
                .partition(|e| matches!(e, PluginEngineEvent::ServerPlugins { .. }));
            for event in announced {
                if let PluginEngineEvent::ServerPlugins {
                    server_id,
                    sender,
                    data,
                } = event
                {
                    suggestions::handle(&app, &plugins, &engine, server_id, sender, data).await;
                }
            }
            let friends = friend_hashes(&engine, &batch);
            let result = plugins
                .call(move |service| {
                    for event in batch {
                        feed(service, event, &friends);
                    }
                    Ok(())
                })
                .await;
            if let Err(e) = result {
                tracing::warn!(error = %e, "feeding plugin events failed");
            }
        }
    });
}

/// Sends the plugin announce (`mumcord.plugins`) when it is due on a connection.
pub fn pump_announces(plugins: &Arc<Plugins>, engine: &Engine) {
    let plugins = Arc::clone(plugins);
    let engine = engine.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            let polled = plugins
                .call(|service| Ok((service.poll_announces(), service.next_announce_at())))
                .await;
            let next = match polled {
                Ok((announces, next)) => {
                    for announce in announces {
                        if let Err(e) = engine.plugin_send(
                            announce.server,
                            &announce.receivers,
                            ANNOUNCE_DATA_ID,
                            announce.bytes,
                        ) {
                            tracing::debug!(error = %e, "plugin announce not sent");
                        }
                    }
                    next
                }
                Err(e) => {
                    tracing::warn!(error = %e, "checking plugin announces failed");
                    None
                }
            };
            let wait = next
                .map(|at| at.saturating_duration_since(Instant::now()))
                .unwrap_or(ANNOUNCE_POLL)
                .min(ANNOUNCE_POLL);
            tokio::time::sleep(wait).await;
        }
    });
}

/// A plugin message to the engine's low-priority lane; an expired one counts for the plugin's
/// diagnostics.
pub fn send(
    plugins: &Arc<Plugins>,
    engine: &Engine,
    server: ServerId,
    plugin: String,
    outgoing: Outgoing,
) {
    let ticket = match engine.plugin_send(
        server,
        &outgoing.receivers,
        &outgoing.data_id,
        outgoing.bytes,
    ) {
        Ok(ticket) => ticket,
        Err(e) => {
            tracing::debug!(plugin, error = %e, "plugin message not sent");
            return;
        }
    };
    let plugins = Arc::clone(plugins);
    tauri::async_runtime::spawn(async move {
        if ticket.outcome().await == LowOutcome::Expired {
            let result = plugins
                .call(move |service| {
                    service.message_expired(server, &plugin);
                    Ok(())
                })
                .await;
            if let Err(e) = result {
                tracing::warn!(error = %e, "counting an expired plugin message failed");
            }
        }
    });
}

/// Posts a plugin's `chat.send` line ("[Name] text") as plain text into the user's channel on
/// `server`, through the normal chat path. Returns the stored own message for the UI.
pub async fn post_line(
    engine: &Engine,
    server: ServerId,
    line: &str,
) -> Result<ChatMessageView, mumcord_engine::EngineError> {
    let channel = engine
        .snapshot_of(server)
        .and_then(|s| {
            s.users
                .iter()
                .find(|u| u.session == s.self_session)
                .map(|u| u.channel_id)
        })
        .ok_or_else(|| {
            mumcord_engine::EngineError::new(mumcord_engine::ErrorKind::NotConnected, "no channel")
        })?;
    engine
        .send_channel_message(server, channel, &escape_html(line))
        .await
}

/// The cert hashes of the user's friends, only looked up when the batch names people.
fn friend_hashes(engine: &Engine, batch: &[PluginEngineEvent]) -> HashSet<String> {
    let names_people = batch.iter().any(|e| {
        matches!(
            e,
            PluginEngineEvent::Joined { .. }
                | PluginEngineEvent::SelfChanged { .. }
                | PluginEngineEvent::PeerData { .. }
        )
    });
    if !names_people {
        return HashSet::new();
    }
    engine.friends().into_iter().map(|f| f.cert_hash).collect()
}

fn peer(peer: PluginPeer, friends: &HashSet<String>) -> PeerInfo {
    let is_friend = peer
        .cert_hash
        .as_ref()
        .is_some_and(|hash| friends.contains(hash));
    PeerInfo {
        session: peer.session,
        name: peer.name,
        cert_hash: peer.cert_hash,
        is_friend,
    }
}

fn feed(service: &mut PluginService, event: PluginEngineEvent, friends: &HashSet<String>) {
    match event {
        PluginEngineEvent::SharedState {
            server_id,
            supported,
        } => service.shared_state(server_id, supported),
        PluginEngineEvent::Disconnected { server_id } => service.disconnected(server_id),
        PluginEngineEvent::PeerCapable {
            server_id,
            session,
            plugins,
        } => service.peer_capable(server_id, session, plugins),
        PluginEngineEvent::PeerBot {
            server_id,
            session,
            bot,
        } => service.peer_bot(
            server_id,
            session,
            bot.map(|bot| BotInfo {
                name: bot.name,
                modules: bot.modules,
            }),
        ),
        PluginEngineEvent::Joined { server_id, peer: p } => {
            service.peer_joined(server_id, peer(p, friends));
        }
        PluginEngineEvent::Left { server_id, peer: p } => {
            service.peer_left(server_id, &peer(p, friends));
        }
        PluginEngineEvent::Talking {
            server_id,
            peer: p,
            talking,
        } => service.peer_talking(server_id, &peer(p, friends), talking),
        PluginEngineEvent::ScreenShareStarted { server_id, peer: p } => {
            service.screen_share_started(server_id, &peer(p, friends));
        }
        PluginEngineEvent::ChatActivity {
            server_id,
            sender,
            channel,
        } => service.chat_activity(
            server_id,
            &peer(sender, friends),
            (channel.id, channel.name),
        ),
        PluginEngineEvent::ChatTrigger {
            server_id,
            sender,
            channel,
            trigger,
            args,
            message_id,
        } => service.chat_trigger(
            server_id,
            &peer(sender, friends),
            channel.id,
            &trigger,
            &args,
            message_id,
        ),
        PluginEngineEvent::ChatMatch {
            server_id,
            sender,
            channel,
            text,
            message_id,
        } => service.chat_match(
            server_id,
            &peer(sender, friends),
            channel.id,
            &text,
            message_id,
        ),
        PluginEngineEvent::Mentioned { server_id, sender } => {
            service.mentioned(server_id, &peer(sender, friends));
        }
        PluginEngineEvent::SelfChanged { server_id, state } => service.self_changed(
            server_id,
            SelfInfo {
                me: peer(state.me, friends),
                channel: state.channel.map(|c| (c.id, c.name)),
                muted: state.muted,
                deafened: state.deafened,
            },
        ),
        PluginEngineEvent::PeerData {
            server_id,
            sender,
            data_id,
            data,
        } => service.peer_data(server_id, &peer(sender, friends), &data_id, &data),
        // Split off before feeding: suggestions are for the notification centre.
        PluginEngineEvent::ServerPlugins { .. } => {}
    }
}

/// A channel message for the supersede check (FR-6.45): its plain text as official clients show
/// it. Private messages never supersede. Own messages can: a plugin's own "[Name] " line (the
/// only kind a card may hide) is the user's own message on the sender's client.
pub fn incoming_line(message: &ChatMessageView) -> Option<(u64, u32, String)> {
    if !matches!(message.conversation, ConversationRef::Channel { .. }) {
        return None;
    }
    let id = message.id.parse().ok()?;
    let sender = message.sender_session?;
    Some((
        id,
        sender,
        mumcord_engine::plain_text::plain_text(&message.body),
    ))
}

/// Plugin text is plain text: it must never become markup in anyone's client.
pub fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_lines_are_escaped_and_read_back_unchanged() {
        let line = "[Poll] <b>Pizza</b> & \"pasta\" 'tonight'?";
        let html = escape_html(line);
        assert!(!html.contains('<'));
        assert_eq!(mumcord_engine::plain_text::plain_text(&html), line);
    }

    #[test]
    fn friends_are_matched_by_cert_hash() {
        let friends = HashSet::from(["aa".to_owned()]);
        let p = |hash: Option<&str>| PluginPeer {
            session: 1,
            name: "x".into(),
            cert_hash: hash.map(str::to_owned),
        };
        assert!(peer(p(Some("aa")), &friends).is_friend);
        assert!(!peer(p(Some("bb")), &friends).is_friend);
        assert!(!peer(p(None), &friends).is_friend);
    }
}
