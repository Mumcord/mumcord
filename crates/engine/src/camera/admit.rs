//! Who may watch the own camera (spec 08, FR-8.47 – FR-8.49, FR-8.52).
//!
//! Admission is automatic: a `cam-join` from a Mumcord user in the same channel who is not
//! blocked (checked before it gets here, `signal.rs` and the session's block filter) is admitted
//! through the bot while the broadcast has room, or served over its own direct stream when the
//! camera has no new-format relay broadcast (FR-8.94). A `cam-leave`, a viewer leaving the
//! channel or the server ends the admission: the bot is told (`bcast-kick`) and the key rotates
//! (FR-7.44) for a relay viewer, while a direct viewer's stream simply closes.

use std::collections::BTreeMap;

use super::signal::Codec;

/// Viewers per camera broadcast (FR-8.52, relay `broadcast_full`); the direct path serves
/// [`super::direct::MAX_DIRECT_VIEWERS`] instead (FR-8.93).
pub const MAX_VIEWERS: u16 = 25;

/// How one viewer gets the own camera; the camera room's viewer list shows it (FR-8.62,
/// FR-8.94).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraPath {
    /// Through the relay broadcast.
    Relay,
    /// Through its own `STREAM_CAMERA` stream.
    Direct,
}

/// One admitted viewer of the own camera.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CameraViewer {
    pub session: u32,
    pub path: CameraPath,
}

/// What to do about a `cam-join`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Join {
    /// Ask the bot to admit the viewer, and let its certificate get the keys.
    Admit,
    /// The camera has no room; the viewer asks again later (FR-8.52).
    Full,
}

#[derive(Debug, Clone)]
struct Viewer {
    hash: String,
    av1: bool,
    path: CameraPath,
}

/// The admitted viewers of the own camera.
#[derive(Debug)]
pub struct CameraViewers {
    limit: usize,
    viewers: BTreeMap<u32, Viewer>,
}

impl CameraViewers {
    /// Room for `limit` viewers, at most [`MAX_VIEWERS`].
    pub fn new(limit: u16) -> Self {
        Self {
            limit: usize::from(limit.min(MAX_VIEWERS)),
            viewers: BTreeMap::new(),
        }
    }

    /// `viewer` with certificate `hash` asked to join, playing `codecs` and served over `path`.
    /// A viewer who is already in is admitted again (it binds anew, for example after a
    /// reconnect of its link).
    pub fn join(&mut self, viewer: u32, hash: &str, codecs: &[Codec], path: CameraPath) -> Join {
        if !self.viewers.contains_key(&viewer) && self.is_full() {
            return Join::Full;
        }
        self.viewers.insert(
            viewer,
            Viewer {
                hash: hash.to_ascii_lowercase(),
                av1: codecs.contains(&Codec::Av1),
                path,
            },
        );
        Join::Admit
    }

    /// `viewer` left or was refused by the bot; returns its certificate hash and path so the
    /// caller drops it from the publisher and rekeys, or `None` when it was not admitted.
    pub fn leave(&mut self, viewer: u32) -> Option<(String, CameraPath)> {
        self.viewers.remove(&viewer).map(|v| (v.hash, v.path))
    }

    /// No room for another viewer.
    pub fn is_full(&self) -> bool {
        self.viewers.len() >= self.limit
    }

    pub fn contains(&self, viewer: u32) -> bool {
        self.viewers.contains_key(&viewer)
    }

    /// Some admitted viewer cannot play AV1, so the camera sends VP9 (FR-8.43, FR-7.58).
    pub fn needs_vp9(&self) -> bool {
        self.viewers.values().any(|v| !v.av1)
    }

    /// The admitted viewers with their paths, in session order.
    pub fn entries(&self) -> Vec<CameraViewer> {
        self.viewers
            .iter()
            .map(|(&session, viewer)| CameraViewer {
                session,
                path: viewer.path,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOTH: &[Codec] = &[Codec::Av1, Codec::Vp9];

    #[test]
    fn viewers_are_admitted_until_the_broadcast_is_full() {
        let mut viewers = CameraViewers::new(2);
        assert_eq!(viewers.join(1, "AA", BOTH, CameraPath::Relay), Join::Admit);
        assert_eq!(viewers.join(2, "bb", BOTH, CameraPath::Direct), Join::Admit);
        assert_eq!(viewers.join(3, "cc", BOTH, CameraPath::Relay), Join::Full);
        assert!(viewers.is_full());
        // Joining again is fine for someone who is in.
        assert_eq!(viewers.join(1, "aa", BOTH, CameraPath::Relay), Join::Admit);
        assert_eq!(
            viewers.entries(),
            [
                CameraViewer {
                    session: 1,
                    path: CameraPath::Relay
                },
                CameraViewer {
                    session: 2,
                    path: CameraPath::Direct
                }
            ]
        );
        // A leave frees the seat and hands back the hash and path for the rekey.
        assert_eq!(viewers.leave(1), Some(("aa".to_owned(), CameraPath::Relay)));
        assert_eq!(viewers.leave(1), None);
        assert!(!viewers.is_full());
        assert_eq!(viewers.join(3, "cc", BOTH, CameraPath::Relay), Join::Admit);
        assert!(viewers.contains(3));
    }

    #[test]
    fn the_limit_is_at_most_twenty_five() {
        let mut viewers = CameraViewers::new(100);
        for v in 0..u32::from(MAX_VIEWERS) {
            assert_eq!(viewers.join(v, "h", BOTH, CameraPath::Relay), Join::Admit);
        }
        assert_eq!(viewers.join(99, "h", BOTH, CameraPath::Relay), Join::Full);
    }

    #[test]
    fn a_vp9_only_viewer_switches_the_camera_to_vp9_while_it_watches() {
        let mut viewers = CameraViewers::new(MAX_VIEWERS);
        viewers.join(1, "a", BOTH, CameraPath::Direct);
        assert!(!viewers.needs_vp9());
        viewers.join(2, "b", &[Codec::Vp9], CameraPath::Direct);
        assert!(viewers.needs_vp9());
        viewers.leave(2);
        assert!(!viewers.needs_vp9());
    }
}
