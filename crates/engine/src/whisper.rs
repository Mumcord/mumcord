//! Whisper and shout (issue #67) through Mumble's standard voice targets, so it works on any
//! server and with official clients.
//!
//! Sending: a saved target names people or a channel ([`WhisperReceivers`]). Starting a whisper
//! looks them up on the voice server, defines them as a `VoiceTarget` and then sets the target
//! id in [`DspControls`](mumcord_audio::dsp::DspControls), which the capture thread reads with
//! one atomic load per frame. Like Mumble's client, each start defines a fresh id (cycling
//! through 1..=30), so audio still in flight for an older definition never reaches the new
//! receivers.
//!
//! The server checks the Whisper permission for every receiver (in the target channel, or in
//! the channel a named person is in) and silently drops the ones it does not allow. Where the
//! own permissions are known, the client refuses up front with a reason the UI can show.
//!
//! Receiving: [`IncomingWhispers`] turns the context of received packets into "whispers to
//! you" / "shouts to you" marks per speaker.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

pub use mumble_client_protocol::WhisperKind;
use mumble_client_protocol::proto::{self, voice_target};
use mumble_client_protocol::{ServerState, VOICE_TARGET_MAX, permissions};
use mumcord_audio::dsp::DspControls;
use mumcord_store::{ServerId, WhisperReceivers};

use crate::session::convert::{channel_path, resolve_path};
use crate::session::{Engine, EngineEvent, Events, SessionCtx, lock};

/// A mark ends this long after the speaker's last whispered packet if the terminator was lost.
const MARK_TIMEOUT: Duration = Duration::from_secs(1);

/// Why a whisper or shout could not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WhisperDenied {
    /// Not talking on any server (every connection is read-only or none is open), or the
    /// target belongs to a server that does not carry voice.
    NoVoiceServer,
    /// Self-muted or deafened: nothing would be sent.
    Muted,
    /// Muted or suppressed by the server, which then drops all own audio.
    ServerMuted,
    /// None of the target's people are on the voice server.
    NobodyHere,
    /// A saved name fits more than one person on the server. Whispering to all of them could
    /// reach somebody the user never chose, so nothing is sent.
    AmbiguousName,
    /// The target's channel does not exist on the voice server.
    ChannelNotFound,
    /// The server does not allow whispering into this channel.
    NoPermission { channel: String },
}

impl std::fmt::Display for WhisperDenied {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoVoiceServer => f.write_str("not talking on any server"),
            Self::Muted => f.write_str("muted"),
            Self::ServerMuted => f.write_str("muted by the server"),
            Self::NobodyHere => f.write_str("nobody of the target is on this server"),
            Self::AmbiguousName => f.write_str("a name of the target fits several people"),
            Self::ChannelNotFound => f.write_str("the target channel is not on this server"),
            Self::NoPermission { .. } => f.write_str("no whisper permission"),
        }
    }
}

/// Who a whisper or shout goes to, in the terms of the server: the identities Mumble's
/// `VoiceTarget` addresses. Unlike a saved [`WhisperReceivers`] nothing here is looked up by
/// name, so a rename or a second person with the same name cannot change who hears it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WhisperTo {
    /// These people, by their session on the server.
    Sessions(Vec<u32>),
    /// Everyone in a channel.
    Channel {
        channel_id: u32,
        children: bool,
        links: bool,
        /// Only members of this ACL group, e.g. `admin`.
        group: Option<String>,
    },
}

/// The whisper or shout going on right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActiveWhisper {
    server_id: ServerId,
    /// The voice target id the audio is sent with.
    id: u8,
    /// The people it is bound to; empty for a channel.
    sessions: Vec<u32>,
}

/// Voice target ids handed out so far; the next start uses the following id.
static TARGET_IDS: AtomicU32 = AtomicU32::new(0);

fn next_target_id() -> u8 {
    let n = TARGET_IDS.fetch_add(1, Ordering::Relaxed) % u32::from(VOICE_TARGET_MAX);
    n as u8 + 1
}

impl Engine {
    /// Starts whispering or shouting to a saved target on the voice server; audio goes there
    /// until [`Engine::whisper_stop`]. Holding a target transmits in either talk mode.
    ///
    /// A saved target names people, so they are looked up now and the whisper is then bound
    /// to the sessions found, exactly as with [`Engine::whisper_start_to`]. A name that fits
    /// more than one person is refused ([`WhisperDenied::AmbiguousName`]).
    pub fn whisper_start(&self, receivers: &WhisperReceivers) -> Result<(), WhisperDenied> {
        let server_id = self.voice_server().ok_or(WhisperDenied::NoVoiceServer)?;
        let ctx = self
            .session_ctx_for(server_id)
            .map_err(|_| WhisperDenied::NoVoiceServer)?;
        let to = ctx.handle.with_state(|s| resolve(s, receivers))?;
        self.whisper_begin(&ctx, &to)
    }

    /// Starts whispering to the given sessions, or shouting to the given channel, on
    /// `server_id`, which must be the voice server; audio goes there until
    /// [`Engine::whisper_stop`].
    ///
    /// The whisper stays bound to these sessions: a person who leaves is taken out, and when
    /// the last one left it ends with [`EngineEvent::WhisperEnded`]. Nobody else can come to
    /// hear it, whatever their name is.
    pub fn whisper_start_to(
        &self,
        server_id: ServerId,
        to: &WhisperTo,
    ) -> Result<(), WhisperDenied> {
        if self.voice_server() != Some(server_id) {
            return Err(WhisperDenied::NoVoiceServer);
        }
        let ctx = self
            .session_ctx_for(server_id)
            .map_err(|_| WhisperDenied::NoVoiceServer)?;
        self.whisper_begin(&ctx, to)
    }

    fn whisper_begin(&self, ctx: &SessionCtx, to: &WhisperTo) -> Result<(), WhisperDenied> {
        let id = next_target_id();
        // Held from the lookup on, so a person leaving at this moment is taken out afterwards.
        let mut active = lock(self.active_whisper());
        let (target, sessions) = ctx.handle.with_state(|s| voice_target(s, to, id))?;
        ctx.handle.send_voice_target(&target).map_err(|e| {
            tracing::warn!(error = %e, "sending the voice target failed");
            WhisperDenied::NoVoiceServer
        })?;
        *active = Some(ActiveWhisper {
            server_id: ctx.server_id,
            id,
            sessions,
        });
        self.dsp_controls().set_voice_target(id);
        Ok(())
    }

    /// Ends a whisper or shout; talking goes back to the own channel.
    pub fn whisper_stop(&self) {
        *lock(self.active_whisper()) = None;
        self.dsp_controls().set_voice_target(0);
    }
}

/// A person left the server: a whisper bound to them no longer reaches that session. The
/// server hands session numbers out again, so the target is defined anew without it; with
/// nobody left the whisper ends.
pub(crate) fn on_user_removed(
    active: &Mutex<Option<ActiveWhisper>>,
    controls: &DspControls,
    ctx: &SessionCtx,
    session: u32,
) {
    let mut active = lock(active);
    let Some(whisper) = active
        .as_mut()
        .filter(|w| w.server_id == ctx.server_id && w.sessions.contains(&session))
    else {
        return;
    };
    whisper.sessions.retain(|s| *s != session);
    let rest = proto::VoiceTarget {
        id: Some(u32::from(whisper.id)),
        targets: vec![voice_target::Target {
            session: whisper.sessions.clone(),
            ..Default::default()
        }],
    };
    let ended = whisper.sessions.is_empty();
    if ended {
        // Before the definition below: no more audio is addressed to the target.
        controls.set_voice_target(0);
        *active = None;
    }
    if let Err(e) = ctx.handle.send_voice_target(&rest) {
        tracing::warn!(error = %e, "redefining the voice target failed");
    }
    if ended {
        ctx.events.emit(EngineEvent::WhisperEnded);
    }
}

/// The server `server_id` no longer carries the user's voice (its connection ended or voice
/// moved to another server): a whisper going there ends.
pub(crate) fn on_voice_lost(
    active: &Mutex<Option<ActiveWhisper>>,
    controls: &DspControls,
    events: &Events,
    server_id: ServerId,
) {
    let mut active = lock(active);
    if active.as_ref().is_some_and(|w| w.server_id == server_id) {
        *active = None;
        controls.set_voice_target(0);
        events.for_server(server_id).emit(EngineEvent::WhisperEnded);
    }
}

/// Looks a saved target up on the server in `state`: the people by name, the channel by its
/// path. A name that fits several people is refused rather than whispered to all of them.
pub(crate) fn resolve(
    state: &ServerState,
    receivers: &WhisperReceivers,
) -> Result<WhisperTo, WhisperDenied> {
    match receivers {
        WhisperReceivers::People { names } => {
            let mut sessions = Vec::new();
            for name in names {
                let mut found = state
                    .users
                    .values()
                    .filter(|u| Some(u.session) != state.session)
                    .filter(|u| name.eq_ignore_ascii_case(&u.name));
                let first = found.next();
                if found.next().is_some() {
                    return Err(WhisperDenied::AmbiguousName);
                }
                sessions.extend(first.map(|u| u.session));
            }
            Ok(WhisperTo::Sessions(sessions))
        }
        WhisperReceivers::Channel {
            path,
            children,
            links,
            group,
        } => Ok(WhisperTo::Channel {
            channel_id: resolve_path(state, path).ok_or(WhisperDenied::ChannelNotFound)?,
            children: *children,
            links: *links,
            group: group.clone(),
        }),
    }
}

/// The `VoiceTarget` message for `to` on the server in `state` and the sessions it is bound
/// to (none for a channel), or why the user cannot whisper there.
pub(crate) fn voice_target(
    state: &ServerState,
    to: &WhisperTo,
    id: u8,
) -> Result<(proto::VoiceTarget, Vec<u32>), WhisperDenied> {
    let own = state.own_user().ok_or(WhisperDenied::NoVoiceServer)?;
    if own.self_mute || own.self_deaf {
        return Err(WhisperDenied::Muted);
    }
    if own.mute || own.deaf || own.suppress {
        return Err(WhisperDenied::ServerMuted);
    }
    // Unknown permissions count as allowed: the server only answers for queried channels, and
    // it checks every receiver anyway.
    let may_whisper = |channel: u32| {
        state
            .permissions
            .get(&channel)
            .is_none_or(|bits| bits & permissions::WHISPER != 0)
    };
    let denied = |channel: u32| WhisperDenied::NoPermission {
        channel: channel_path(state, channel)
            .and_then(|path| path.last().cloned())
            .or_else(|| state.channels.get(&channel).map(|c| c.name.clone()))
            .unwrap_or_default(),
    };
    let (target, sessions) = match to {
        WhisperTo::Sessions(sessions) => {
            let mut wanted: Vec<u32> = sessions.clone();
            wanted.sort_unstable();
            wanted.dedup();
            let people: Vec<_> = wanted
                .iter()
                .filter(|session| Some(**session) != state.session)
                .filter_map(|session| state.users.get(session))
                .collect();
            let allowed: Vec<u32> = people
                .iter()
                .filter(|u| may_whisper(u.channel_id))
                .map(|u| u.session)
                .collect();
            if allowed.is_empty() {
                return Err(people
                    .first()
                    .map_or(WhisperDenied::NobodyHere, |u| denied(u.channel_id)));
            }
            let target = voice_target::Target {
                session: allowed.clone(),
                ..Default::default()
            };
            (target, allowed)
        }
        WhisperTo::Channel {
            channel_id,
            children,
            links,
            group,
        } => {
            if !state.channels.contains_key(channel_id) {
                return Err(WhisperDenied::ChannelNotFound);
            }
            if !may_whisper(*channel_id) {
                return Err(denied(*channel_id));
            }
            let target = voice_target::Target {
                channel_id: Some(*channel_id),
                group: group.clone().filter(|g| !g.trim().is_empty()),
                links: Some(*links),
                children: Some(*children),
                ..Default::default()
            };
            (target, Vec::new())
        }
    };
    let message = proto::VoiceTarget {
        id: Some(u32::from(id)),
        targets: vec![target],
    };
    Ok((message, sessions))
}

/// Who is whispering or shouting to us right now, from the context of received packets.
#[derive(Debug, Default)]
pub(crate) struct IncomingWhispers {
    speakers: BTreeMap<u32, (WhisperKind, Instant)>,
}

impl IncomingWhispers {
    /// A voice packet of `session` arrived; returns the speaker's new mark if it changed.
    pub fn on_packet(
        &mut self,
        session: u32,
        kind: Option<WhisperKind>,
        is_last: bool,
        now: Instant,
    ) -> Option<Option<WhisperKind>> {
        match kind.filter(|_| !is_last) {
            Some(kind) => {
                let previous = self.speakers.insert(session, (kind, now));
                (previous.map(|(k, _)| k) != Some(kind)).then_some(Some(kind))
            }
            None => self.speakers.remove(&session).map(|_| None),
        }
    }

    /// Speakers whose mark timed out (their terminator never arrived).
    pub fn expire(&mut self, now: Instant) -> Vec<u32> {
        let expired: Vec<u32> = self
            .speakers
            .iter()
            .filter(|(_, (_, at))| now.saturating_duration_since(*at) >= MARK_TIMEOUT)
            .map(|(session, _)| *session)
            .collect();
        for session in &expired {
            self.speakers.remove(session);
        }
        expired
    }
}

#[cfg(test)]
mod tests {
    use mumble_client_protocol::{Channel, User};
    use prost::Message;

    use super::*;

    const OWN: u32 = 1;

    fn state() -> ServerState {
        let mut s = ServerState {
            session: Some(OWN),
            ..ServerState::default()
        };
        for (id, parent, name) in [
            (0, None, "Root"),
            (5, Some(0), "Raid Night"),
            (6, Some(5), "Team"),
        ] {
            s.channels.insert(
                id,
                Channel {
                    id,
                    parent,
                    name: name.into(),
                    ..Channel::default()
                },
            );
        }
        for (session, name, channel) in [(OWN, "Zoe", 0), (2, "Maya", 0), (3, "Priya", 5)] {
            s.users.insert(
                session,
                User {
                    session,
                    name: name.into(),
                    channel_id: channel,
                    ..User::default()
                },
            );
        }
        s
    }

    fn people(names: &[&str]) -> WhisperReceivers {
        WhisperReceivers::People {
            names: names.iter().map(|n| (*n).to_owned()).collect(),
        }
    }

    /// The message for a saved target, as [`Engine::whisper_start`] builds it.
    fn saved_target(
        state: &ServerState,
        receivers: &WhisperReceivers,
        id: u8,
    ) -> Result<proto::VoiceTarget, WhisperDenied> {
        let to = resolve(state, receivers)?;
        voice_target(state, &to, id).map(|(message, _)| message)
    }

    fn raid(children: bool, links: bool, group: Option<&str>) -> WhisperReceivers {
        WhisperReceivers::Channel {
            path: vec!["Raid Night".into()],
            children,
            links,
            group: group.map(str::to_owned),
        }
    }

    #[test]
    fn whisper_to_people_names_their_sessions() {
        let target = saved_target(&state(), &people(&["maya", "Priya", "Gone"]), 3).expect("ok");
        assert_eq!(target.id, Some(3));
        assert_eq!(target.targets.len(), 1);
        assert_eq!(target.targets[0].session, vec![2, 3]);
        assert_eq!(target.targets[0].channel_id, None);
    }

    #[test]
    fn a_name_that_fits_two_people_is_refused() {
        let mut s = state();
        s.users.insert(
            4,
            User {
                session: 4,
                name: "MAYA".into(),
                channel_id: 5,
                ..User::default()
            },
        );
        assert_eq!(
            saved_target(&s, &people(&["Maya"]), 1),
            Err(WhisperDenied::AmbiguousName)
        );
        assert_eq!(
            saved_target(&s, &people(&["Priya", "maya"]), 1),
            Err(WhisperDenied::AmbiguousName)
        );
        // By session there is no doubt: only the chosen one of the two hears it.
        let (target, bound) = voice_target(&s, &WhisperTo::Sessions(vec![4]), 1).expect("ok");
        assert_eq!(target.targets[0].session, vec![4]);
        assert_eq!(bound, vec![4]);
    }

    #[test]
    fn whisper_by_session_ignores_names_and_people_who_are_gone() {
        let mut s = state();
        // A rename between choosing and sending changes nothing.
        s.users.get_mut(&2).expect("maya").name = "Someone Else".into();
        let to = WhisperTo::Sessions(vec![3, 2, 2, 99, OWN]);
        let (target, bound) = voice_target(&s, &to, 1).expect("ok");
        assert_eq!(target.targets[0].session, vec![2, 3]);
        assert_eq!(bound, vec![2, 3]);
        assert_eq!(
            voice_target(&s, &WhisperTo::Sessions(vec![99]), 1),
            Err(WhisperDenied::NobodyHere)
        );
        let shout = WhisperTo::Channel {
            channel_id: 99,
            children: false,
            links: false,
            group: None,
        };
        assert_eq!(
            voice_target(&s, &shout, 1),
            Err(WhisperDenied::ChannelNotFound)
        );
    }

    #[test]
    fn whisper_never_targets_oneself() {
        assert_eq!(
            saved_target(&state(), &people(&["Zoe"]), 1),
            Err(WhisperDenied::NobodyHere)
        );
    }

    #[test]
    fn shout_to_channel_carries_children_links_and_group() {
        let target = saved_target(&state(), &raid(true, false, Some("admin")), 7).expect("ok");
        let t = &target.targets[0];
        assert_eq!(t.channel_id, Some(5));
        assert_eq!((t.children, t.links), (Some(true), Some(false)));
        assert_eq!(t.group.as_deref(), Some("admin"));
        assert!(t.session.is_empty());
        let blank = saved_target(&state(), &raid(false, true, Some("  ")), 7).expect("ok");
        assert_eq!(blank.targets[0].group, None);
    }

    #[test]
    fn voice_target_encodes_like_mumble_proto() {
        let target = saved_target(&state(), &raid(true, true, None), 30).expect("ok");
        let bytes = target.encode_to_vec();
        // Field 1 (id) varint 30, then field 2 (targets): channel_id 5, links, children.
        assert_eq!(&bytes[..2], &[0x08, 30]);
        assert_eq!(bytes[2], 0x12);
        let decoded = proto::VoiceTarget::decode(bytes.as_slice()).expect("decode");
        assert_eq!(decoded, target);
    }

    #[test]
    fn missing_channel_and_permissions_are_refused() {
        let mut s = state();
        let gone = WhisperReceivers::Channel {
            path: vec!["Nope".into()],
            children: false,
            links: false,
            group: None,
        };
        assert_eq!(
            saved_target(&s, &gone, 1),
            Err(WhisperDenied::ChannelNotFound)
        );
        s.permissions.insert(5, permissions::SPEAK);
        assert_eq!(
            saved_target(&s, &raid(false, false, None), 1),
            Err(WhisperDenied::NoPermission {
                channel: "Raid Night".into()
            })
        );
        // Priya sits in Raid Night: only Maya stays; alone, Priya is refused with the reason.
        let both = saved_target(&s, &people(&["Maya", "Priya"]), 1).expect("ok");
        assert_eq!(both.targets[0].session, vec![2]);
        assert!(matches!(
            saved_target(&s, &people(&["Priya"]), 1),
            Err(WhisperDenied::NoPermission { .. })
        ));
        s.permissions.insert(5, permissions::WHISPER);
        assert!(saved_target(&s, &raid(false, false, None), 1).is_ok());
    }

    #[test]
    fn muted_users_cannot_whisper() {
        let mut s = state();
        s.users.get_mut(&OWN).expect("own").self_mute = true;
        assert_eq!(
            saved_target(&s, &people(&["Maya"]), 1),
            Err(WhisperDenied::Muted)
        );
        let own = s.users.get_mut(&OWN).expect("own");
        own.self_mute = false;
        own.suppress = true;
        assert_eq!(
            saved_target(&s, &people(&["Maya"]), 1),
            Err(WhisperDenied::ServerMuted)
        );
    }

    #[test]
    fn target_ids_cycle_through_the_client_range() {
        let ids: Vec<u8> = (0..64).map(|_| next_target_id()).collect();
        assert!(ids.iter().all(|id| (1..=VOICE_TARGET_MAX).contains(id)));
        assert!(ids.windows(2).all(|w| w[0] != w[1]));
    }

    #[test]
    fn incoming_marks_follow_the_packets() {
        let mut marks = IncomingWhispers::default();
        let t0 = Instant::now();
        let whisper = Some(WhisperKind::Whisper);
        assert_eq!(marks.on_packet(2, whisper, false, t0), Some(whisper));
        assert_eq!(marks.on_packet(2, whisper, false, t0), None);
        assert_eq!(
            marks.on_packet(2, Some(WhisperKind::Shout), false, t0),
            Some(Some(WhisperKind::Shout))
        );
        // Normal talking or the terminator end the mark.
        assert_eq!(marks.on_packet(2, None, false, t0), Some(None));
        assert_eq!(marks.on_packet(2, None, false, t0), None);
        assert_eq!(marks.on_packet(3, whisper, false, t0), Some(whisper));
        assert_eq!(marks.on_packet(3, whisper, true, t0), Some(None));
        // A lost terminator: the mark times out.
        marks.on_packet(4, whisper, false, t0);
        assert!(marks.expire(t0 + MARK_TIMEOUT / 2).is_empty());
        assert_eq!(marks.expire(t0 + MARK_TIMEOUT), vec![4]);
    }
}
