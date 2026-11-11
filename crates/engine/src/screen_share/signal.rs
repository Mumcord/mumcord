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
/// The `mv` value of a client that reads the Bovnar media profile (FR-7.96).
pub const MEDIA_VERSION: u8 = 2;

/// What the sharer picked, when the platform can tell (issue #163).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShareKind {
    Window,
    Screen,
}
/// Viewers per sharer (FR-2.43); the ninth gets [`Signal::Full`].
pub const MAX_VIEWERS: usize = 8;
/// Longest viewer list read: a relay broadcast has more viewers than direct copies (spec 07,
/// FR-7.46; the relay's limit is a u16, 25 by default).
pub const MAX_LISTED_VIEWERS: usize = 64;

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
    /// The sender wants to watch the receiver's share. 0.6.x sends neither field: it plays
    /// VP9 only and reads frame version 1 (FR-7.58).
    Watch {
        /// Codecs the viewer can play; unknown names are dropped when read.
        #[serde(
            default,
            deserialize_with = "known_codecs",
            skip_serializing_if = "Vec::is_empty"
        )]
        codecs: Vec<CodecName>,
        /// 1 when the viewer can join relay broadcasts (FR-7.42).
        #[serde(default, skip_serializing_if = "is_zero")]
        bc: u8,
        /// [`MEDIA_VERSION`] when the viewer reads the Bovnar media profile (FR-7.96).
        #[serde(default, skip_serializing_if = "is_zero")]
        mv: u8,
    },
    /// The receiver may watch the sender's share now.
    Accept,
    /// The sender's share already has [`MAX_VIEWERS`] viewers.
    Full,
    /// The sender's share uses a format this viewer's Mumcord cannot read.
    NeedsUpdate,
    /// The sender stopped watching the receiver's share.
    Unwatch,
    /// Everyone watching the sender's share, sorted.
    Viewers { list: Vec<u32> },
    // BC1 (spec 07, FR-7.52), added before the wire v2 work (N5b): keep this variant when
    // merging.
    /// A viewer watching through the relay broadcast tells the sharer what it receives, every
    /// few seconds: video frames in the last second, and whether layers went missing in the
    /// last 10 s that it did not turn off itself ("slow connection").
    Report { fps: u32, slow: bool },
}

/// A codec name in [`Signal::Watch`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CodecName {
    Av1,
    Vp9,
}

/// What this client can play as a viewer, sent in every [`Signal::Watch`]: AV1 and VP9 always,
/// since the engine decodes either (dav1d, libvpx) where the webview's WebCodecs cannot (FR-7.58).
pub const PLAYABLE_CODECS: &[CodecName] = &[CodecName::Av1, CodecName::Vp9];

/// The watch request this client sends; `broadcast` when it can join the sharer's relay
/// broadcast now (FR-7.42).
pub fn watch_request(broadcast: bool) -> Signal {
    Signal::Watch {
        codecs: PLAYABLE_CODECS.to_vec(),
        bc: u8::from(broadcast),
        mv: MEDIA_VERSION,
    }
}

/// Drops codec names this client does not know, so a newer viewer listing one still gets in.
fn known_codecs<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<CodecName>, D::Error> {
    let names: Vec<serde_json::Value> = Vec::deserialize(d)?;
    let mut codecs: Vec<CodecName> = names
        .into_iter()
        .filter_map(|name| serde_json::from_value(name).ok())
        .collect();
    codecs.dedup();
    Ok(codecs)
}

fn is_zero(n: &u8) -> bool {
    *n == 0
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
/// list over [`MAX_LISTED_VIEWERS`].
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
            if list.len() > MAX_LISTED_VIEWERS {
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
            Signal::Watch {
                codecs: vec![],
                bc: 0,
                mv: 0,
            },
            Signal::Watch {
                codecs: vec![CodecName::Av1, CodecName::Vp9],
                bc: 1,
                mv: MEDIA_VERSION,
            },
            Signal::Accept,
            Signal::Full,
            Signal::Unwatch,
            Signal::Viewers {
                list: vec![1, 2, 3],
            },
            Signal::Report {
                fps: 30,
                slow: true,
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
    fn broadcast_viewer_lists_are_read() {
        let list: Vec<u32> = (1..=25).collect();
        assert_eq!(
            decode(&encode(&Signal::Viewers { list: list.clone() })),
            Some(Signal::Viewers { list })
        );
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
        let list: Vec<String> = (0..=MAX_LISTED_VIEWERS).map(|i| i.to_string()).collect();
        let long = format!(r#"{{"v":1,"t":"viewers","list":[{}]}}"#, list.join(","));
        assert_eq!(decode(long.as_bytes()), None);
    }

    #[test]
    fn reads_newer_versions_and_normalises_viewers() {
        assert_eq!(
            decode(br#"{"v":2,"t":"watch","extra":1}"#),
            Some(Signal::Watch {
                codecs: vec![],
                bc: 0,
                mv: 0
            })
        );
        assert_eq!(
            decode(br#"{"v":1,"t":"viewers","list":[3,1,3]}"#),
            Some(Signal::Viewers { list: vec![1, 3] })
        );
    }

    #[test]
    fn watch_codecs_are_optional_and_unknown_ones_dropped() {
        assert_eq!(
            decode(br#"{"v":2,"t":"watch","codecs":["h266","av1",7,"vp9"],"bc":1}"#),
            Some(Signal::Watch {
                codecs: vec![CodecName::Av1, CodecName::Vp9],
                bc: 1,
                mv: 0
            })
        );
        assert_eq!(decode(br#"{"v":2,"t":"watch","codecs":"av1"}"#), None);
        // 0.6.x sends no list; a Watch without codecs stays as short as before.
        assert_eq!(
            encode(&Signal::Watch {
                codecs: vec![],
                bc: 0,
                mv: 0
            }),
            br#"{"v":2,"t":"watch"}"#
        );
    }

    #[test]
    fn this_client_asks_with_its_playable_codecs() {
        let sent = encode(&watch_request(false));
        assert_eq!(
            sent,
            br#"{"v":2,"t":"watch","codecs":["av1","vp9"],"mv":2}"#
        );
        // 0.6.x reads `{"t":"watch"}` with extra fields as a plain watch (see the test above).
        assert!(decode(&sent).is_some());
        assert_eq!(
            encode(&watch_request(true)),
            br#"{"v":2,"t":"watch","codecs":["av1","vp9"],"bc":1,"mv":2}"#
        );
    }
}
