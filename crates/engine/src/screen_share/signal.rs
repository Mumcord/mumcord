//! Screen share signalling over plugin data ([`DATA_ID`]), sent only to Mumcord users in the
//! same channel that announced [`FEATURE`].
//!
//! A sharer announces its share with [`Signal::Share`] (again on every change, and `live:
//! false` when it stops); someone entering the channel gets it twice, a few seconds apart, in
//! case the server dropped the first. A viewer asks with [`Signal::Watch`]; the sharer answers
//! [`Signal::Accept`] (the viewer then opens a [`crate::p2p::STREAM_SCREEN`] stream to the
//! sharer) or [`Signal::Full`] once [`MAX_VIEWERS`] watch. [`Signal::Unwatch`] ends watching
//! and [`Signal::Viewers`] tells the viewers who else watches.
//!
//! The sender of a signal is the session the server stamped on the data, never a payload
//! field. Payloads are untrusted: they are size-capped and versioned, and viewer lists are
//! capped (and must still be checked against the people in the channel by the caller).

use mumcord_protocol::PLUGIN_DATA_MAX;
use serde::{Deserialize, Serialize};

/// Plugin data id of every screen share signal; other ids are ignored.
pub const DATA_ID: &str = "mumcord.screen";
/// Hello feature of clients that can share and watch screens.
pub const FEATURE: &str = "screen-share";
/// The signal version this client sends; higher versions are still read.
pub const VERSION: u32 = 2;

/// What the sharer picked, when the platform can tell (issue #163).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShareKind {
    Window,
    Screen,
}
/// Viewers per sharer (FR-2.43); the ninth gets [`Signal::Full`].
pub const MAX_VIEWERS: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "lowercase")]
pub enum Signal {
    /// The sender's share: whether it is live, paused, and whether it carries sound.
    Share {
        live: bool,
        paused: bool,
        sound: bool,
        /// What the sharer picked; `None` where the platform cannot tell (issue #163).
        #[serde(default)]
        kind: Option<ShareKind>,
        /// How long the share had been running when this signal was sent (issue #163).
        #[serde(default)]
        elapsed_ms: u32,
    },
    /// The sender wants to watch the receiver's share.
    Watch,
    /// The receiver may watch the sender's share now.
    Accept,
    /// The sender's share already has [`MAX_VIEWERS`] viewers.
    Full,
    /// The sender stopped watching the receiver's share.
    Unwatch,
    /// Everyone watching the sender's share, sorted.
    Viewers { list: Vec<u32> },
}

#[derive(Serialize, Deserialize)]
struct Wire {
    v: u32,
    #[serde(flatten)]
    signal: Signal,
}

pub fn encode(signal: &Signal) -> Vec<u8> {
    serde_json::to_vec(&Wire {
        v: VERSION,
        signal: signal.clone(),
    })
    .unwrap_or_default()
}

/// The signal in `data`; `None` for anything too large, malformed, unknown or with a viewer
/// list over [`MAX_VIEWERS`].
pub fn decode(data: &[u8]) -> Option<Signal> {
    if data.len() > PLUGIN_DATA_MAX {
        return None;
    }
    let wire: Wire = serde_json::from_slice(data).ok()?;
    if wire.v == 0 {
        return None;
    }
    match wire.signal {
        Signal::Viewers { mut list } => {
            if list.len() > MAX_VIEWERS {
                return None;
            }
            list.sort_unstable();
            list.dedup();
            Some(Signal::Viewers { list })
        }
        other => Some(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signals_round_trip() {
        let signals = [
            Signal::Share {
                live: true,
                paused: false,
                sound: true,
                kind: Some(ShareKind::Window),
                elapsed_ms: 4200,
            },
            Signal::Watch,
            Signal::Accept,
            Signal::Full,
            Signal::Unwatch,
            Signal::Viewers {
                list: vec![1, 2, 3],
            },
        ];
        for signal in signals {
            assert_eq!(decode(&encode(&signal)), Some(signal));
        }
    }

    #[test]
    fn version_1_shares_still_read() {
        let signal = decode(br#"{"v":1,"t":"share","live":true,"paused":false,"sound":false}"#);
        let Some(Signal::Share {
            kind, elapsed_ms, ..
        }) = signal
        else {
            panic!("not a share: {signal:?}");
        };
        assert_eq!((kind, elapsed_ms), (None, 0));
    }

    #[test]
    fn full_viewer_list_fits_plugin_data() {
        let list = (0..MAX_VIEWERS as u32).map(|i| u32::MAX - i).collect();
        assert!(encode(&Signal::Viewers { list }).len() <= PLUGIN_DATA_MAX);
    }

    #[test]
    fn rejects_bad_payloads() {
        assert_eq!(decode(&vec![b' '; PLUGIN_DATA_MAX + 1]), None);
        assert_eq!(decode(b"not json"), None);
        assert_eq!(decode(br#"{"v":0,"t":"watch"}"#), None);
        assert_eq!(decode(br#"{"t":"watch"}"#), None);
        assert_eq!(decode(br#"{"v":1,"t":"dance"}"#), None);
        assert_eq!(decode(br#"{"v":1,"t":"share","live":true}"#), None);
        assert_eq!(
            decode(br#"{"v":1,"t":"viewers","list":[1,2,3,4,5,6,7,8,9]}"#),
            None
        );
    }

    #[test]
    fn reads_newer_versions_and_normalises_viewers() {
        assert_eq!(
            decode(br#"{"v":2,"t":"watch","extra":1}"#),
            Some(Signal::Watch)
        );
        assert_eq!(
            decode(br#"{"v":1,"t":"viewers","list":[3,1,3]}"#),
            Some(Signal::Viewers { list: vec![1, 3] })
        );
    }
}
