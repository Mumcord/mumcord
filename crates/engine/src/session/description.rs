//! Channel descriptions for the pinned info box in the chat (FR-1.48b). Servers send long
//! descriptions only as a hash; the full text is requested on demand with `RequestBlob` when the
//! channel's chat is opened. The protocol state keeps the text until the hash changes, so each
//! hash is requested at most once per connection.

use std::collections::HashSet;

use mumcord_protocol::Channel;

/// Blob requests already sent on this connection, keyed by channel and description hash.
#[derive(Debug, Default)]
pub struct DescriptionRequests {
    sent: HashSet<(u32, Vec<u8>)>,
}

impl DescriptionRequests {
    /// Whether the channel's full description must be requested now. Records the request, so a
    /// second call for the same hash (chat reopened before the answer arrived) returns false.
    pub fn should_request(&mut self, channel: &Channel) -> bool {
        match (&channel.description, &channel.description_hash) {
            (None, Some(hash)) => self.sent.insert((channel.id, hash.clone())),
            _ => false,
        }
    }
}

/// True when the channel has a description, even if only its hash is known yet.
pub fn has_description(channel: &Channel) -> bool {
    match &channel.description {
        Some(text) => !text.trim().is_empty(),
        None => channel.description_hash.is_some(),
    }
}

#[cfg(test)]
mod tests {
    use mumcord_protocol::{ServerState, proto};

    use super::*;
    use crate::session::convert::channel_view;

    fn state_with(m: proto::ChannelState) -> ServerState {
        let mut state = ServerState::default();
        state.apply_channel_state(proto::ChannelState {
            channel_id: Some(0),
            name: Some("Root".into()),
            ..Default::default()
        });
        state.apply_channel_state(m);
        state
    }

    #[test]
    fn hash_only_description_is_requested_once_and_shown_when_it_arrives() {
        let mut state = state_with(proto::ChannelState {
            channel_id: Some(2),
            parent: Some(0),
            name: Some("Raid".into()),
            description_hash: Some(vec![1, 2, 3]),
            ..Default::default()
        });
        let mut requests = DescriptionRequests::default();
        let view = channel_view(&state, &state.channels[&2]);
        assert!(view.has_description);
        assert_eq!(view.description, None);
        assert!(requests.should_request(&state.channels[&2]));
        assert!(
            !requests.should_request(&state.channels[&2]),
            "already requested"
        );

        // The server's answer to RequestBlob: the text without the hash.
        state.apply_channel_state(proto::ChannelState {
            channel_id: Some(2),
            description: Some("<b>Raid</b> rules<script>x()</script>".into()),
            ..Default::default()
        });
        let view = channel_view(&state, &state.channels[&2]);
        assert_eq!(view.description.as_deref(), Some("<b>Raid</b> rules"));
        assert!(
            !requests.should_request(&state.channels[&2]),
            "text is known"
        );

        // A changed description arrives as a new hash and is requested again.
        state.apply_channel_state(proto::ChannelState {
            channel_id: Some(2),
            description_hash: Some(vec![4, 5, 6]),
            ..Default::default()
        });
        let view = channel_view(&state, &state.channels[&2]);
        assert_eq!(view.description, None);
        assert!(requests.should_request(&state.channels[&2]));
    }

    #[test]
    fn inline_and_empty_descriptions_need_no_request() {
        let state = state_with(proto::ChannelState {
            channel_id: Some(1),
            parent: Some(0),
            name: Some("Lobby".into()),
            description: Some("Welcome".into()),
            ..Default::default()
        });
        let mut requests = DescriptionRequests::default();
        let lobby = &state.channels[&1];
        assert!(!requests.should_request(lobby));
        assert_eq!(
            channel_view(&state, lobby).description.as_deref(),
            Some("Welcome")
        );

        let root = &state.channels[&0];
        assert!(!has_description(root));
        assert!(!requests.should_request(root));

        let cleared = Channel {
            description: Some("  ".into()),
            description_hash: Some(vec![9]),
            ..root.clone()
        };
        assert!(!has_description(&cleared));
        assert!(!requests.should_request(&cleared));
    }
}
