//! Plugin data `mumcord.camera` (spec 08, FR-8.8, FR-8.91, section 6.6): the camera state of a
//! user and the viewers' joins. Each message is a Bovnar document of at most 512 bytes; this
//! module is the only place that knows the encoding. It parses, filters and rate-limits what
//! arrives and sends what the camera session asks for; it keeps no camera session state.
//!
//! Everything here comes from other clients and is untrusted. Logs carry a reason code and never
//! payload content.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use bovnar::DomDoc;
use mumcord_protocol::PluginData;

use crate::files::wire::{parse_document, str_field, version_field, write_document};
use crate::files::{is_lower_hex, wire::Rejected as WireRejected};
use crate::mumcord::ServerPeers;
use crate::session::{SessionCtx, lock};

/// Plugin data id of these messages.
pub const DATA_ID: &str = "mumcord.camera";

/// Hello feature of clients that understand [`DATA_ID`] (counts toward the hello limit of 16).
pub const FEATURE: &str = "camera";

/// Largest payload, in bytes; checked before any decoding.
pub const MAX_PAYLOAD_BYTES: usize = 512;

/// A viewer may send `cam-join` to one sender at most once in this time.
pub const JOIN_INTERVAL: Duration = Duration::from_secs(10);

const VERSION: u8 = 1;
/// A broadcast id is 16 lowercase hex characters.
const ID_LEN: usize = 16;
/// Beyond this many remembered viewers, entries older than [`JOIN_INTERVAL`] are dropped.
const MAX_REMEMBERED_VIEWERS: usize = 64;

/// A video codec a viewer can play.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    Av1,
    Vp9,
}

impl Codec {
    const fn name(self) -> &'static str {
        match self {
            Self::Av1 => "av1",
            Self::Vp9 => "vp9",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        match name {
            "av1" => Some(Self::Av1),
            "vp9" => Some(Self::Vp9),
            _ => None,
        }
    }
}

/// One `mumcord.camera` message. The sender is the session the server stamped on the data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CameraMessage {
    /// Camera owner to the channel (and to newcomers): the camera is on, with this broadcast id.
    On { id: String },
    /// Camera owner to the channel: the camera is off.
    Off,
    /// Viewer to camera owner: please admit me to broadcast `id`. `codecs` lists what the viewer
    /// plays, never empty.
    CamJoin { id: String, codecs: Vec<Codec> },
    /// Viewer to camera owner: I stopped watching broadcast `id`.
    CamLeave { id: String },
    /// Camera owner to one viewer: broadcast `id` has no room for you (FR-8.52); ask again on
    /// the next `on`.
    Full { id: String },
    /// Camera owner to one viewer: camera `id` uses a media format this client cannot play and
    /// needs Mumcord 0.9.0 (FR-8.95); stop asking until the next `on`.
    Needs { id: String },
}

impl CameraMessage {
    /// Whether the message goes to the whole channel (`on`, `off`) or to one person (`cam-join`,
    /// `cam-leave` to the camera owner, `full` to the viewer).
    pub const fn to_channel(&self) -> bool {
        matches!(self, Self::On { .. } | Self::Off)
    }
}

/// Why a received document or message was refused. Logs carry this reason only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Refused {
    #[error("too-large")]
    TooLarge,
    #[error("malformed")]
    Malformed,
    #[error("wrong-type")]
    WrongType,
    #[error("invalid")]
    Invalid,
    #[error("not-same-channel")]
    NotSameChannel,
    #[error("rate-limited")]
    RateLimited,
}

impl From<WireRejected> for Refused {
    fn from(r: WireRejected) -> Self {
        match r {
            WireRejected::TooLarge => Self::TooLarge,
            WireRejected::Malformed => Self::Malformed,
            WireRejected::WrongType => Self::WrongType,
            WireRejected::Invalid => Self::Invalid,
        }
    }
}

/// A message that passed every check, with the session that sent it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CameraEvent {
    pub sender: u32,
    pub message: CameraMessage,
}

fn is_broadcast_id(id: &str) -> bool {
    id.len() == ID_LEN && is_lower_hex(id)
}

/// Encodes a message as a Bovnar document; `None` if an id is not a broadcast id or `codecs` is
/// empty.
pub fn encode(message: &CameraMessage) -> Option<Vec<u8>> {
    let (t, id, codecs) = match message {
        CameraMessage::On { id } => ("on", Some(id), None),
        CameraMessage::Off => ("off", None, None),
        CameraMessage::CamJoin { id, codecs } => {
            if codecs.is_empty() {
                return None;
            }
            let names: Vec<_> = codecs.iter().map(|c| c.name()).collect();
            ("cam-join", Some(id), Some(names.join(",")))
        }
        CameraMessage::CamLeave { id } => ("cam-leave", Some(id), None),
        CameraMessage::Full { id } => ("full", Some(id), None),
        CameraMessage::Needs { id } => ("needs", Some(id), None),
    };
    if id.is_some_and(|id| !is_broadcast_id(id)) {
        return None;
    }
    write_document(MAX_PAYLOAD_BYTES, |w| {
        w.write_uint("v", VERSION.into(), 8, bovnar::Base::DECIMAL)?;
        w.write_string("t", t)?;
        if let Some(id) = id {
            w.write_string("id", id)?;
        }
        if let Some(codecs) = &codecs {
            w.write_string("codecs", codecs)?;
        }
        Ok(())
    })
}

/// Decodes received plugin data. The size is checked first, so oversized input never reaches
/// the parser.
pub fn decode(data: &[u8]) -> Result<CameraMessage, Refused> {
    let doc = parse_document(data, MAX_PAYLOAD_BYTES)?;
    version_field(&doc, VERSION)?;
    let t = str_field(&doc, "t")?;
    Ok(match t {
        "off" => CameraMessage::Off,
        "on" => CameraMessage::On { id: id_of(&doc)? },
        "cam-leave" => CameraMessage::CamLeave { id: id_of(&doc)? },
        "full" => CameraMessage::Full { id: id_of(&doc)? },
        "needs" => CameraMessage::Needs { id: id_of(&doc)? },
        "cam-join" => {
            let id = id_of(&doc)?;
            // Names this client cannot play are skipped, so a newer client may list more; a
            // list without any known codec leaves nothing to send.
            let codecs: Vec<Codec> = str_field(&doc, "codecs")?
                .split(',')
                .filter_map(Codec::from_name)
                .collect();
            if codecs.is_empty() {
                return Err(Refused::Invalid);
            }
            CameraMessage::CamJoin { id, codecs }
        }
        _ => return Err(Refused::Invalid),
    })
}

fn id_of(doc: &DomDoc) -> Result<String, Refused> {
    let id = str_field(doc, "id")?;
    if !is_broadcast_id(id) {
        return Err(Refused::Invalid);
    }
    Ok(id.to_owned())
}

/// When each viewer last sent `cam-join`, for the 1-per-10-s limit.
#[derive(Debug, Default)]
pub struct JoinLimiter {
    last: HashMap<u32, Instant>,
}

impl JoinLimiter {
    /// True if `viewer` may send a `cam-join` at `now`; a refused one does not extend the wait.
    pub fn allow(&mut self, viewer: u32, now: Instant) -> bool {
        if self.last.len() >= MAX_REMEMBERED_VIEWERS {
            self.last
                .retain(|_, at| now.saturating_duration_since(*at) < JOIN_INTERVAL);
        }
        match self.last.get(&viewer) {
            Some(at) if now.saturating_duration_since(*at) < JOIN_INTERVAL => false,
            _ => {
                self.last.insert(viewer, now);
                true
            }
        }
    }

    /// Forgets a viewer who left the server.
    pub fn forget(&mut self, viewer: u32) {
        self.last.remove(&viewer);
    }
}

/// Checks and decodes one payload from `sender`. `same_channel` says whether the sender is a
/// non-blocked session in the own channel; the caller works that out from the session state.
pub fn receive(
    sender: u32,
    same_channel: bool,
    data: &[u8],
    limiter: &mut JoinLimiter,
    now: Instant,
) -> Result<CameraEvent, Refused> {
    if !same_channel {
        return Err(Refused::NotSameChannel);
    }
    let message = decode(data)?;
    if matches!(message, CameraMessage::CamJoin { .. }) && !limiter.allow(sender, now) {
        return Err(Refused::RateLimited);
    }
    Ok(CameraEvent { sender, message })
}

/// State of the camera channel for all servers: the join limits.
#[derive(Default)]
pub(crate) struct CameraSignals {
    limits: Mutex<HashMap<crate::ServerId, JoinLimiter>>,
}

impl CameraSignals {
    /// `mumcord.camera` data arrived (session task). Returns the event for the camera session;
    /// anything refused is logged with its reason code and dropped. Data from blocked people
    /// never gets here (the session drops it first).
    pub(crate) fn on_plugin_data(
        &self,
        ctx: &SessionCtx,
        data: &PluginData,
    ) -> Option<CameraEvent> {
        if data.data_id != DATA_ID {
            return None;
        }
        let sender = data.sender?;
        let same_channel = ctx.handle.with_state(|s| {
            let own = s.users.get(&s.session?)?.channel_id;
            Some(s.session != Some(sender) && s.users.get(&sender)?.channel_id == own)
        });
        let mut limits = lock(&self.limits);
        let limiter = limits.entry(ctx.server_id).or_default();
        match receive(
            sender,
            same_channel.unwrap_or(false),
            &data.data,
            limiter,
            Instant::now(),
        ) {
            Ok(event) => Some(event),
            Err(reason) => {
                tracing::debug!(%reason, size = data.data.len(), "ignoring a camera message");
                None
            }
        }
    }

    /// A person left `server_id`.
    pub(crate) fn on_user_removed(&self, server_id: crate::ServerId, session: u32) {
        if let Some(limiter) = lock(&self.limits).get_mut(&server_id) {
            limiter.forget(session);
        }
    }

    /// The connection to `server_id` ended or started afresh.
    pub(crate) fn on_connection_reset(&self, server_id: crate::ServerId) {
        lock(&self.limits).remove(&server_id);
    }
}

/// The sessions in the own channel that announced [`FEATURE`], without the own session. Callers
/// drop blocked people (FR-8.59) before sending `cam-join`; `on` and `off` go to everyone here.
pub(crate) fn channel_peers(ctx: &SessionCtx, peers: &ServerPeers) -> Vec<u32> {
    let in_channel: Vec<u32> = ctx.handle.with_state(|s| {
        let Some(own) = s.session else {
            return Vec::new();
        };
        let Some(channel_id) = s.users.get(&own).map(|u| u.channel_id) else {
            return Vec::new();
        };
        s.users
            .values()
            .filter(|u| u.session != own && u.channel_id == channel_id)
            .map(|u| u.session)
            .collect()
    });
    let servers = lock(peers);
    let Some(known) = servers.get(&ctx.server_id) else {
        return Vec::new();
    };
    in_channel
        .into_iter()
        .filter(|s| {
            known
                .features(*s)
                .is_some_and(|f| f.iter().any(|x| x == FEATURE))
        })
        .collect()
}

/// Sends `message` to `receivers`. Returns false if nothing was sent: no receivers, an invalid
/// message or a failed send (logged by reason only).
pub(crate) fn send(ctx: &SessionCtx, receivers: &[u32], message: &CameraMessage) -> bool {
    if receivers.is_empty() {
        return false;
    }
    let Some(bytes) = encode(message) else {
        tracing::debug!("camera message not sent: invalid");
        return false;
    };
    match ctx.handle.send_plugin_data(receivers, DATA_ID, bytes) {
        Ok(()) => true,
        Err(e) => {
            tracing::debug!(error = %e, "camera message not sent");
            false
        }
    }
}

/// Answers a newcomer in the channel with the own `on` (FR-2.75: the sender answers). Does
/// nothing unless the newcomer is in the own channel and announced [`FEATURE`].
pub(crate) fn announce_to_newcomer(
    ctx: &SessionCtx,
    peers: &ServerPeers,
    newcomer: u32,
    on: &CameraMessage,
) -> bool {
    channel_peers(ctx, peers).contains(&newcomer) && send(ctx, &[newcomer], on)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const ID: &str = "0123456789abcdef";

    fn all() -> Vec<CameraMessage> {
        vec![
            CameraMessage::On { id: ID.into() },
            CameraMessage::Off,
            CameraMessage::CamJoin {
                id: ID.into(),
                codecs: vec![Codec::Av1, Codec::Vp9],
            },
            CameraMessage::CamLeave { id: ID.into() },
            CameraMessage::Full { id: ID.into() },
            CameraMessage::Needs { id: ID.into() },
        ]
    }

    fn doc(fields: &str) -> Vec<u8> {
        fields.replace("ID", ID).into_bytes()
    }

    #[test]
    fn every_message_round_trips_as_bovnar_within_the_limit() {
        for m in all() {
            let bytes = encode(&m).expect("valid");
            assert!(bytes.len() <= MAX_PAYLOAD_BYTES);
            assert_eq!(bovnar::json::detect(&bytes), bovnar::json::Format::Bovnar);
            assert_eq!(decode(&bytes), Ok(m));
        }
    }

    #[test]
    fn fields_match_the_spec_table() {
        use bovnar::DomNode;
        let join = encode(&all()[2]).expect("valid");
        let d = DomDoc::parse(&join).expect("bovnar");
        assert_eq!(d.get("v").and_then(DomNode::as_u8), Some(1));
        assert_eq!(d.get("t").and_then(DomNode::as_str), Some("cam-join"));
        assert_eq!(d.get("id").and_then(DomNode::as_str), Some(ID));
        assert_eq!(d.get("codecs").and_then(DomNode::as_str), Some("av1,vp9"));
        let off = DomDoc::parse(&encode(&CameraMessage::Off).expect("valid")).expect("bovnar");
        assert!(off.get("id").is_none());
    }

    #[test]
    fn direction_follows_the_table() {
        let to_channel: Vec<_> = all().iter().map(CameraMessage::to_channel).collect();
        assert_eq!(to_channel, [true, true, false, false, false, false]);
    }

    #[test]
    fn invalid_messages_are_not_encoded() {
        assert_eq!(encode(&CameraMessage::On { id: "short".into() }), None);
        assert_eq!(
            encode(&CameraMessage::On {
                id: "ABCDEF0123456789".into()
            }),
            None
        );
        assert_eq!(
            encode(&CameraMessage::CamJoin {
                id: ID.into(),
                codecs: vec![]
            }),
            None
        );
        assert_eq!(encode(&CameraMessage::Needs { id: "short".into() }), None);
    }

    #[test]
    fn needs_is_validated_and_carries_only_the_camera_id() {
        let good = encode(&CameraMessage::Needs { id: ID.into() }).expect("valid");
        assert_eq!(decode(&good), Ok(CameraMessage::Needs { id: ID.into() }));
        let cases = [
            (r#".v = 1; .t = "needs";"#, Refused::Malformed),
            (r#".v = 0; .t = "needs"; .id = "ID";"#, Refused::Invalid),
            (r#".v = 1; .t = "needs"; .id = 5;"#, Refused::WrongType),
            (
                r#".v = 1; .t = "needs"; .id = "0123456789ABCDEF";"#,
                Refused::Invalid,
            ),
            (
                r#".v = 1; .t = "needs"; .id = "0123456789abcde";"#,
                Refused::Invalid,
            ),
            (
                r#".v = 1; .t = "needs"; .id = "0123456789abcdef0";"#,
                Refused::Invalid,
            ),
            (
                r#".v = 1; .t = "needs"; .id = "0123456789abcdeg";"#,
                Refused::Invalid,
            ),
        ];
        for (text, reason) in cases {
            assert_eq!(decode(&doc(text)), Err(reason), "{text}");
        }
        let mut big = good;
        big.resize(MAX_PAYLOAD_BYTES + 1, b' ');
        assert_eq!(decode(&big), Err(Refused::TooLarge));
    }

    #[test]
    fn bad_documents_are_refused_with_a_reason() {
        let cases = [
            (r#".v = 1; .t = "on";"#, Refused::Malformed),
            (r#".v = 1; .id = "ID";"#, Refused::Malformed),
            (r#".v = "1"; .t = "on"; .id = "ID";"#, Refused::WrongType),
            (r#".v = 1; .t = 5; .id = "ID";"#, Refused::WrongType),
            (r#".v = 1; .t = "on"; .id = 7;"#, Refused::WrongType),
            (r#".v = 0; .t = "off";"#, Refused::Invalid),
            (r#".v = 1; .t = "dance";"#, Refused::Invalid),
            (r#".v = 1; .t = "on"; .id = "xyz";"#, Refused::Invalid),
            (
                r#".v = 1; .t = "cam-join"; .id = "ID";"#,
                Refused::Malformed,
            ),
            (
                r#".v = 1; .t = "cam-join"; .id = "ID"; .codecs = 3;"#,
                Refused::WrongType,
            ),
            (
                r#".v = 1; .t = "cam-join"; .id = "ID"; .codecs = "h264";"#,
                Refused::Invalid,
            ),
            (
                r#".v = 1; .t = "cam-join"; .id = "ID"; .codecs = "";"#,
                Refused::Invalid,
            ),
        ];
        for (text, reason) in cases {
            assert_eq!(decode(&doc(text)), Err(reason), "{text}");
        }
    }

    #[test]
    fn truncated_json_and_empty_payloads_are_refused() {
        let good = encode(&all()[0]).expect("valid");
        for cut in 0..good.len() {
            assert!(decode(&good[..cut]).is_err() || cut == good.len());
        }
        assert!(decode(br#"{"v":1,"t":"off"}"#).is_err());
        assert!(decode(b"").is_err());
    }

    #[test]
    fn oversized_payloads_are_refused_before_decoding() {
        let mut big = encode(&all()[0]).expect("valid");
        big.resize(MAX_PAYLOAD_BYTES + 1, b' ');
        assert_eq!(decode(&big), Err(Refused::TooLarge));
        big.truncate(MAX_PAYLOAD_BYTES);
        assert_ne!(decode(&big), Err(Refused::TooLarge));
    }

    #[test]
    fn newer_versions_unknown_fields_and_unknown_codecs_are_read() {
        let text =
            doc(r#".v = 4; .t = "cam-join"; .id = "ID"; .codecs = "h266,vp9"; .x = [1, 2];"#);
        assert_eq!(
            decode(&text),
            Ok(CameraMessage::CamJoin {
                id: ID.into(),
                codecs: vec![Codec::Vp9]
            })
        );
    }

    #[test]
    fn cam_join_is_limited_to_one_per_ten_seconds_per_viewer() {
        let mut limiter = JoinLimiter::default();
        let join = encode(&all()[2]).expect("valid");
        let t0 = Instant::now();
        assert!(receive(7, true, &join, &mut limiter, t0).is_ok());
        let soon = t0 + Duration::from_secs(9);
        assert_eq!(
            receive(7, true, &join, &mut limiter, soon),
            Err(Refused::RateLimited)
        );
        // Another viewer is unaffected, and the refused attempt did not extend the wait.
        assert!(receive(8, true, &join, &mut limiter, soon).is_ok());
        assert!(receive(7, true, &join, &mut limiter, t0 + JOIN_INTERVAL).is_ok());
    }

    #[test]
    fn other_messages_are_not_join_limited() {
        let mut limiter = JoinLimiter::default();
        let now = Instant::now();
        for m in [&all()[0], &all()[1], &all()[3], &all()[5]] {
            let bytes = encode(m).expect("valid");
            for _ in 0..3 {
                assert!(receive(7, true, &bytes, &mut limiter, now).is_ok());
            }
        }
    }

    #[test]
    fn people_outside_the_channel_are_refused_before_anything_else() {
        let mut limiter = JoinLimiter::default();
        let join = encode(&all()[2]).expect("valid");
        let now = Instant::now();
        assert_eq!(
            receive(7, false, &join, &mut limiter, now),
            Err(Refused::NotSameChannel)
        );
        // The refused join did not use up the viewer's allowance.
        assert!(receive(7, true, &join, &mut limiter, now).is_ok());
    }

    #[test]
    fn the_limiter_stays_small_and_forgets_leavers() {
        let mut limiter = JoinLimiter::default();
        let t0 = Instant::now();
        for viewer in 0..200 {
            limiter.allow(viewer, t0);
        }
        assert!(limiter.allow(1000, t0 + JOIN_INTERVAL));
        assert!(limiter.last.len() <= MAX_REMEMBERED_VIEWERS);
        limiter.forget(1000);
        assert!(limiter.allow(1000, t0 + JOIN_INTERVAL));
    }

    proptest! {
        #[test]
        fn random_bytes_never_panic_and_never_pass_oversized(data in proptest::collection::vec(any::<u8>(), 0..2048)) {
            let result = decode(&data);
            if data.len() > MAX_PAYLOAD_BYTES {
                prop_assert_eq!(result, Err(Refused::TooLarge));
            }
        }

        #[test]
        fn mutated_valid_documents_never_panic(
            which in 0usize..6,
            flips in proptest::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..6),
        ) {
            let mut bytes = encode(&all()[which]).expect("valid");
            for (at, value) in flips {
                let i = at.index(bytes.len());
                bytes[i] = value;
            }
            let _ = decode(&bytes);
        }
    }
}
