//! A read-only view of the control channel for debugging tools such as Mumcord's packet
//! inspector. The connection hands every frame it writes or reads to a [`PacketTap`], but only
//! while [`PacketTap::enabled`] says so; otherwise the cost is one atomic load per frame.

use std::time::Duration;

use bytes::Bytes;
use prost::Message;

use crate::framing::MessageType;
use crate::proto;

/// Whether a frame went to the server or came from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TapDirection {
    Sent,
    Received,
}

/// The client pacer's view of a sent plugin-data frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TapPacing {
    /// How long the frame waited in the pacer queue before it went out.
    pub delay: Duration,
    /// Messages the pacer still lets through at once after this one.
    pub tokens_left: u32,
}

/// The fields of a `PluginDataTransmission`; `data` is untrusted when received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TapPluginData {
    pub sender: Option<u32>,
    pub receivers: Vec<u32>,
    pub data_id: String,
    pub data: Bytes,
}

/// One control-channel frame as the tap sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TappedFrame {
    pub direction: TapDirection,
    /// Wire type number; [`MessageType::from_u16`] names the known ones.
    pub kind: u16,
    /// Payload size in bytes, without the 6-byte frame header.
    pub size: usize,
    /// Decoded for plugin data only: other messages carry chat text and names the inspector
    /// does not need.
    pub plugin_data: Option<TapPluginData>,
    /// Set on sent plugin data, which the client paces.
    pub pacing: Option<TapPacing>,
}

impl TappedFrame {
    pub(crate) fn new(
        direction: TapDirection,
        kind: u16,
        payload: &Bytes,
        pacing: Option<TapPacing>,
    ) -> Self {
        let plugin_data = (kind == MessageType::PluginDataTransmission as u16)
            .then(|| proto::PluginDataTransmission::decode(payload.clone()).ok())
            .flatten()
            .map(|m| TapPluginData {
                sender: m.sender_session,
                receivers: m.receiver_sessions,
                data_id: m.data_id.unwrap_or_default(),
                data: m.data.map(Bytes::from).unwrap_or_default(),
            });
        Self {
            direction,
            kind,
            size: payload.len(),
            plugin_data,
            pacing,
        }
    }
}

/// Receives copies of control-channel frames. Called on the connection's reader and writer
/// tasks, so implementations must return quickly and never block.
pub trait PacketTap: Send + Sync {
    /// Checked before every frame; frames are only built and handed over while this is true.
    fn enabled(&self) -> bool;
    fn on_frame(&self, frame: TappedFrame);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framing::Frame;

    #[test]
    fn plugin_data_is_decoded_and_other_messages_are_not() {
        let m = proto::PluginDataTransmission {
            sender_session: Some(3),
            receiver_sessions: vec![4, 5],
            data: Some(b"{}".to_vec()),
            data_id: Some("mumcord.test".into()),
        };
        let frame = Frame::from_message(MessageType::PluginDataTransmission, &m);
        let tapped = TappedFrame::new(TapDirection::Sent, frame.kind, &frame.payload, None);
        assert_eq!(tapped.size, frame.payload.len());
        let data = tapped.plugin_data.expect("decoded");
        assert_eq!(data.sender, Some(3));
        assert_eq!(data.receivers, vec![4, 5]);
        assert_eq!(data.data_id, "mumcord.test");
        assert_eq!(&data.data[..], b"{}");

        let text = proto::TextMessage {
            message: "secret".into(),
            ..Default::default()
        };
        let frame = Frame::from_message(MessageType::TextMessage, &text);
        let tapped = TappedFrame::new(TapDirection::Received, frame.kind, &frame.payload, None);
        assert_eq!(tapped.plugin_data, None);
        assert_eq!(tapped.kind, MessageType::TextMessage as u16);
    }
}
