//! Packet inspector (issue #60): a hidden debugging view of the control-channel messages,
//! mainly Mumcord's plugin data.
//!
//! Capture runs only while the user has unlocked the developer tools; otherwise the
//! connection's tap returns after one atomic load. Captured packets live in a ring buffer of
//! [`CAPACITY`] entries in memory only: they are never logged or sent anywhere, and turning
//! capture off drops them. Plugin data keeps its payload (for the decoded view and raw hex);
//! every other control message keeps only its type and size, because those carry chat text
//! and names the inspector does not need. Voice is counted, not captured.

use std::collections::{BTreeMap, VecDeque};
use std::fmt::Write as _;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use mumcord_p2p::PEER_DATA_ID;
pub use mumcord_protocol::TapDirection;
use mumcord_protocol::framing::MessageType;
use mumcord_protocol::{PacketTap, TappedFrame};
use mumcord_store::ServerId;
use serde_json::{Value, json};

use crate::session::lock;

/// Field of a `mumcord.peer` token message holding the token.
const TOKEN_FIELD: &str = "tok";

/// Packets kept; the oldest go first.
pub const CAPACITY: usize = 2000;

/// One captured packet, decoded for display.
#[derive(Debug, Clone, PartialEq)]
pub struct PacketView {
    /// Increases by one per captured packet; the UI asks for packets after the last one it has.
    pub id: u64,
    pub server_id: ServerId,
    /// Capture time in milliseconds since the Unix epoch.
    pub at_ms: u64,
    pub direction: TapDirection,
    /// The plugin data id (e.g. `mumcord.watch`), or the Mumble message name (e.g. `UserState`).
    pub kind: String,
    pub plugin_data: bool,
    /// Bytes of the plugin data itself (what the server's size limit applies to), or of the
    /// whole message for other types.
    pub size: u32,
    /// Sender session of received plugin data.
    pub sender: Option<u32>,
    /// Receiver sessions of sent plugin data.
    pub receivers: Vec<u32>,
    /// Time the client pacer held sent plugin data back.
    pub delay_ms: Option<u32>,
    /// Plugin-data messages the pacer still lets through at once after this one.
    pub tokens_left: Option<u32>,
    /// The payload as pretty-printed JSON; `None` if it is neither JSON nor a known format.
    pub decoded: Option<String>,
    /// The plugin data as lowercase hex; empty for other types.
    pub raw_hex: String,
}

/// Packets captured per server, kind and direction since capture started or was cleared,
/// including those the ring buffer already dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PacketCount {
    pub server_id: ServerId,
    pub kind: String,
    pub direction: TapDirection,
    pub count: u64,
}

/// Voice packets of a connected server since its voice link started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VoiceCount {
    pub server_id: ServerId,
    pub sent: u64,
    pub received: u64,
    /// Audio rating and its cause on the server with the voice pipeline (issue #152).
    pub rating: Option<(
        crate::audio_rating::AudioRating,
        Option<crate::audio_rating::AudioIssue>,
    )>,
}

/// What the UI gets on each poll.
#[derive(Debug, Clone, PartialEq)]
pub struct InspectorPoll {
    pub capturing: bool,
    /// Packets newer than the requested id, oldest first.
    pub packets: Vec<PacketView>,
    pub counts: Vec<PacketCount>,
    pub voice: Vec<VoiceCount>,
}

struct Captured {
    id: u64,
    server_id: ServerId,
    at_ms: u64,
    frame: TappedFrame,
}

#[derive(Default)]
struct Buffer {
    packets: VecDeque<Captured>,
    next_id: u64,
    counts: BTreeMap<(ServerId, String, Direction), u64>,
}

/// [`TapDirection`] with an order, for the counter map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Direction {
    Sent,
    Received,
}

impl From<TapDirection> for Direction {
    fn from(d: TapDirection) -> Self {
        match d {
            TapDirection::Sent => Self::Sent,
            TapDirection::Received => Self::Received,
        }
    }
}

impl From<Direction> for TapDirection {
    fn from(d: Direction) -> Self {
        match d {
            Direction::Sent => Self::Sent,
            Direction::Received => Self::Received,
        }
    }
}

/// The capture switch and ring buffer, shared by every connection's tap.
#[derive(Default)]
pub struct Inspector {
    enabled: AtomicBool,
    buffer: Mutex<Buffer>,
}

impl Inspector {
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// Starts or stops capturing; stopping drops everything captured.
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
        if !enabled {
            self.clear();
        }
    }

    /// Drops the captured packets and resets the counters.
    pub fn clear(&self) {
        let mut buffer = lock(&self.buffer);
        buffer.packets.clear();
        buffer.counts.clear();
    }

    /// The tap for one server's connection.
    pub(crate) fn tap(self: &Arc<Self>, server_id: ServerId) -> Arc<dyn PacketTap> {
        Arc::new(ServerTap {
            server_id,
            inspector: self.clone(),
        })
    }

    fn capture(&self, server_id: ServerId, frame: TappedFrame) {
        // Voice is counted by the voice link; capturing every 10 ms frame would flood the list.
        if frame.kind == MessageType::UdpTunnel as u16 {
            return;
        }
        let at_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
        let mut buffer = lock(&self.buffer);
        *buffer
            .counts
            .entry((server_id, kind_of(&frame), frame.direction.into()))
            .or_default() += 1;
        if buffer.packets.len() == CAPACITY {
            buffer.packets.pop_front();
        }
        let id = buffer.next_id;
        buffer.next_id += 1;
        buffer.packets.push_back(Captured {
            id,
            server_id,
            at_ms,
            frame,
        });
    }

    /// Counts a received plugin-data message the engine dropped, e.g. a relay announcement of
    /// an unregistered user (FR-4.8), under `"{data_id} (ignored: {reason})"`.
    pub(crate) fn count_ignored(&self, server_id: ServerId, data_id: &str, reason: &str) {
        if !self.is_enabled() {
            return;
        }
        let kind = format!("{data_id} (ignored: {reason})");
        *lock(&self.buffer)
            .counts
            .entry((server_id, kind, Direction::Received))
            .or_default() += 1;
    }

    /// Packets with an id above `after` (all with `None`) plus the counters.
    pub(crate) fn poll(&self, after: Option<u64>, voice: Vec<VoiceCount>) -> InspectorPoll {
        let buffer = lock(&self.buffer);
        let packets = buffer
            .packets
            .iter()
            .filter(|p| after.is_none_or(|after| p.id > after))
            .map(view)
            .collect();
        let counts = buffer
            .counts
            .iter()
            .map(|((server_id, kind, direction), count)| PacketCount {
                server_id: *server_id,
                kind: kind.clone(),
                direction: (*direction).into(),
                count: *count,
            })
            .collect();
        InspectorPoll {
            capturing: self.is_enabled(),
            packets,
            counts,
            voice,
        }
    }
}

struct ServerTap {
    server_id: ServerId,
    inspector: Arc<Inspector>,
}

impl PacketTap for ServerTap {
    fn enabled(&self) -> bool {
        self.inspector.is_enabled()
    }

    fn on_frame(&self, frame: TappedFrame) {
        self.inspector.capture(self.server_id, frame);
    }
}

fn kind_of(frame: &TappedFrame) -> String {
    match (&frame.plugin_data, MessageType::from_u16(frame.kind)) {
        (Some(data), _) => data.data_id.clone(),
        (None, Some(kind)) => format!("{kind:?}"),
        (None, None) => format!("Unknown({})", frame.kind),
    }
}

fn view(p: &Captured) -> PacketView {
    let frame = &p.frame;
    let data = frame.plugin_data.as_ref();
    PacketView {
        id: p.id,
        server_id: p.server_id,
        at_ms: p.at_ms,
        direction: frame.direction,
        kind: kind_of(frame),
        plugin_data: data.is_some(),
        size: u32::try_from(data.map_or(frame.size, |d| d.data.len())).unwrap_or(u32::MAX),
        sender: data.and_then(|d| d.sender),
        receivers: data.map(|d| d.receivers.clone()).unwrap_or_default(),
        delay_ms: frame
            .pacing
            .map(|p| u32::try_from(p.delay.as_millis()).unwrap_or(u32::MAX)),
        tokens_left: frame.pacing.map(|p| p.tokens_left),
        decoded: data
            .and_then(|d| decode(&d.data_id, &d.data))
            .and_then(|v| serde_json::to_string_pretty(&v).ok()),
        raw_hex: data
            .filter(|d| !carries_token(&d.data_id, &d.data))
            .map(|d| hex(&d.data))
            .unwrap_or_default(),
    }
}

/// Relay tokens are credentials: their messages show neither the token nor the raw bytes
/// (FR-4.21).
fn carries_token(data_id: &str, data: &[u8]) -> bool {
    data_id == PEER_DATA_ID
        && serde_json::from_slice::<Value>(data).map_or(true, |v| v.get(TOKEN_FIELD).is_some())
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        // Writing to a String cannot fail.
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// A readable form of a plugin-data payload. Mumcord's messages are JSON, history sync a JSON
/// header plus a binary body, and full-image transfers binary; anything else from another
/// client or plugin is shown as text if it is UTF-8 and as raw hex only otherwise.
fn decode(data_id: &str, data: &[u8]) -> Option<Value> {
    if data_id == crate::full_image::wire::DATA_ID {
        return crate::full_image::wire::Msg::decode(data).map(|m| image_json(&m));
    }
    if data_id == PEER_DATA_ID {
        return serde_json::from_slice::<Value>(data).ok().map(mask_token);
    }
    if let Ok(value) = serde_json::from_slice::<Value>(data) {
        return Some(value);
    }
    if let Some(at) = data.iter().position(|&b| b == b'\n')
        && let Ok(header) = serde_json::from_slice::<Value>(&data[..at])
    {
        return Some(json!({ "header": header, "bodyBytes": data.len() - at - 1 }));
    }
    std::str::from_utf8(data)
        .ok()
        .map(|text| Value::String(text.to_owned()))
}

fn mask_token(mut value: Value) -> Value {
    if let Some(token) = value.get_mut(TOKEN_FIELD) {
        *token = Value::String("•••".to_owned());
    }
    value
}

fn image_json(m: &crate::full_image::wire::Msg) -> Value {
    use crate::full_image::wire::Msg;
    match m {
        Msg::Get { id, hash } => json!({ "kind": "get", "id": id, "hash": hex(hash) }),
        Msg::Queued { id } => json!({ "kind": "queued", "id": id }),
        Msg::Start { id, size } => json!({ "kind": "start", "id": id, "size": size }),
        Msg::Chunk { id, index, data } => {
            json!({ "kind": "chunk", "id": id, "index": index, "bytes": data.len() })
        }
        Msg::Unavailable { id } => json!({ "kind": "unavailable", "id": id }),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bytes::Bytes;
    use mumcord_protocol::{TapPacing, TapPluginData};

    use super::*;

    fn plugin(direction: TapDirection, data_id: &str, data: &[u8]) -> TappedFrame {
        TappedFrame {
            direction,
            kind: MessageType::PluginDataTransmission as u16,
            size: data.len() + 20,
            plugin_data: Some(TapPluginData {
                sender: Some(7),
                receivers: vec![3, 4],
                data_id: data_id.to_owned(),
                data: Bytes::copy_from_slice(data),
            }),
            pacing: (direction == TapDirection::Sent).then_some(TapPacing {
                delay: Duration::from_millis(340),
                tokens_left: 2,
            }),
        }
    }

    fn control(kind: MessageType) -> TappedFrame {
        TappedFrame {
            direction: TapDirection::Received,
            kind: kind as u16,
            size: 42,
            plugin_data: None,
            pacing: None,
        }
    }

    fn enabled() -> Arc<Inspector> {
        let inspector = Arc::new(Inspector::default());
        inspector.set_enabled(true);
        inspector
    }

    #[test]
    fn the_tap_follows_the_capture_switch() {
        let inspector = Arc::new(Inspector::default());
        let tap = inspector.tap(1);
        assert!(!tap.enabled());
        inspector.set_enabled(true);
        assert!(tap.enabled());
        tap.on_frame(control(MessageType::Ping));
        assert_eq!(inspector.poll(None, Vec::new()).packets.len(), 1);

        inspector.set_enabled(false);
        assert!(!tap.enabled());
        let poll = inspector.poll(None, Vec::new());
        assert!(!poll.capturing);
        assert!(
            poll.packets.is_empty(),
            "turning capture off drops the packets"
        );
        assert!(poll.counts.is_empty());
    }

    #[test]
    fn nothing_is_captured_until_capture_is_switched_on() {
        let inspector = Arc::new(Inspector::default());
        let tap = inspector.tap(1);
        assert!(!inspector.is_enabled());
        assert!(!tap.enabled(), "the connection's tap is told not to run");
        let poll = inspector.poll(None, Vec::new());
        assert!(!poll.capturing);
        assert!(poll.packets.is_empty());
        assert!(poll.counts.is_empty());
    }

    #[test]
    fn the_ring_buffer_keeps_the_newest_packets_and_counts_all() {
        let inspector = enabled();
        for _ in 0..CAPACITY + 5 {
            inspector.capture(1, control(MessageType::Ping));
        }
        let poll = inspector.poll(None, Vec::new());
        assert_eq!(poll.packets.len(), CAPACITY);
        assert_eq!(poll.packets[0].id, 5);
        assert_eq!(poll.counts[0].count, (CAPACITY + 5) as u64);

        let newer = inspector.poll(Some(CAPACITY as u64 + 2), Vec::new());
        let ids: Vec<u64> = newer.packets.iter().map(|p| p.id).collect();
        assert_eq!(ids, [CAPACITY as u64 + 3, CAPACITY as u64 + 4]);

        inspector.clear();
        let poll = inspector.poll(None, Vec::new());
        assert!(poll.packets.is_empty() && poll.counts.is_empty());
        inspector.capture(1, control(MessageType::Ping));
        let ids: Vec<u64> = inspector
            .poll(Some(CAPACITY as u64 + 4), Vec::new())
            .packets
            .iter()
            .map(|p| p.id)
            .collect();
        assert_eq!(
            ids,
            [CAPACITY as u64 + 5],
            "ids keep increasing after a clear"
        );
    }

    #[test]
    fn voice_frames_are_not_captured() {
        let inspector = enabled();
        inspector.capture(1, control(MessageType::UdpTunnel));
        let poll = inspector.poll(None, Vec::new());
        assert!(poll.packets.is_empty() && poll.counts.is_empty());
    }

    #[test]
    fn plugin_data_is_decoded_with_pacing_and_hex() {
        let inspector = enabled();
        inspector.capture(
            2,
            plugin(
                TapDirection::Sent,
                "mumcord.watch",
                br#"{"v":1,"a":"play"}"#,
            ),
        );
        inspector.capture(2, control(MessageType::TextMessage));
        let poll = inspector.poll(None, Vec::new());
        let sent = &poll.packets[0];
        assert_eq!(sent.kind, "mumcord.watch");
        assert!(sent.plugin_data);
        assert_eq!(sent.size, 18);
        assert_eq!(sent.receivers, [3, 4]);
        assert_eq!(sent.delay_ms, Some(340));
        assert_eq!(sent.tokens_left, Some(2));
        assert_eq!(
            sent.decoded.as_deref(),
            Some("{\n  \"a\": \"play\",\n  \"v\": 1\n}")
        );
        assert_eq!(sent.raw_hex, hex(br#"{"v":1,"a":"play"}"#));

        let text = &poll.packets[1];
        assert_eq!(text.kind, "TextMessage");
        assert!(!text.plugin_data);
        assert_eq!(text.size, 42);
        assert_eq!((text.decoded.as_ref(), text.raw_hex.as_str()), (None, ""));

        let mut counts: Vec<(String, TapDirection)> = poll
            .counts
            .into_iter()
            .map(|c| (c.kind, c.direction))
            .collect();
        counts.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(
            counts,
            [
                ("TextMessage".to_owned(), TapDirection::Received),
                ("mumcord.watch".to_owned(), TapDirection::Sent),
            ]
        );
    }

    #[test]
    fn relay_tokens_are_masked_in_both_directions() {
        let inspector = enabled();
        let token = "A".repeat(43);
        let message = format!(
            r#"{{"v":1,"t":"token","rid":"00000000000000aa","a":1,"b":2,"tok":"{token}","ttl":30}}"#
        );
        inspector.capture(
            2,
            plugin(TapDirection::Received, PEER_DATA_ID, message.as_bytes()),
        );
        inspector.capture(
            2,
            plugin(TapDirection::Sent, PEER_DATA_ID, message.as_bytes()),
        );
        let request = br#"{"v":1,"t":"token-req","rid":"00000000000000aa","peer":2}"#;
        inspector.capture(2, plugin(TapDirection::Sent, PEER_DATA_ID, request));
        let poll = inspector.poll(None, Vec::new());
        for packet in &poll.packets[..2] {
            let decoded = packet.decoded.as_deref().expect("decoded");
            assert!(!decoded.contains(&token));
            assert!(decoded.contains("\"tok\": \"•••\""));
            assert!(decoded.contains("\"rid\": \"00000000000000aa\""));
            assert_eq!(packet.raw_hex, "");
        }
        let request_view = &poll.packets[2];
        assert!(
            request_view
                .decoded
                .as_deref()
                .is_some_and(|d| d.contains("token-req"))
        );
        assert_eq!(request_view.raw_hex, hex(request));
    }

    #[test]
    fn ignored_messages_are_counted_while_capturing() {
        let inspector = Arc::new(Inspector::default());
        inspector.count_ignored(2, PEER_DATA_ID, "sender not registered");
        assert!(inspector.poll(None, Vec::new()).counts.is_empty());
        inspector.set_enabled(true);
        inspector.count_ignored(2, PEER_DATA_ID, "sender not registered");
        let counts = inspector.poll(None, Vec::new()).counts;
        assert_eq!(counts.len(), 1);
        assert_eq!(
            counts[0].kind,
            "mumcord.peer (ignored: sender not registered)"
        );
        assert_eq!(counts[0].direction, TapDirection::Received);
        assert_eq!(counts[0].count, 1);
    }

    #[test]
    fn payload_formats_are_decoded() {
        assert_eq!(
            decode("mumcord.hello", br#"{"v":1}"#),
            Some(json!({ "v": 1 }))
        );
        assert_eq!(
            decode("mumcord.history", b"{\"v\":1}\n\x00\x01\x02"),
            Some(json!({ "header": { "v": 1 }, "bodyBytes": 3 }))
        );
        let start = crate::full_image::wire::Msg::Start { id: 9, size: 1234 };
        assert_eq!(
            decode(crate::full_image::wire::DATA_ID, &start.encode()),
            Some(json!({ "kind": "start", "id": 9, "size": 1234 }))
        );
        assert_eq!(
            decode("other.plugin", b"plain text"),
            Some(json!("plain text"))
        );
        assert_eq!(decode("other.plugin", &[0xff, 0x00]), None);
    }
}
