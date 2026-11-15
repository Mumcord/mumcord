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
    /// reconnect of its link). The relay broadcast and the direct path fill separately
    /// (FR-8.93).
    pub fn join(&mut self, viewer: u32, hash: &str, codecs: &[Codec], path: CameraPath) -> Join {
        if !self.viewers.contains_key(&viewer) && self.path_full(path) {
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

    /// No room for another viewer on `path`. The relay cap is this list's limit (at most 25);
    /// the direct cap is eight, and one path being full leaves the other open (FR-8.93).
    pub fn path_full(&self, path: CameraPath) -> bool {
        let taken = self.viewers.values().filter(|v| v.path == path).count();
        taken >= self.cap(path)
    }

    /// The path `viewer` was admitted on, if they were.
    pub fn path_of(&self, viewer: u32) -> Option<CameraPath> {
        self.viewers.get(&viewer).map(|v| v.path)
    }

    fn cap(&self, path: CameraPath) -> usize {
        match path {
            CameraPath::Relay => self.limit,
            CameraPath::Direct => super::direct::MAX_DIRECT_VIEWERS,
        }
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
    fn the_relay_and_direct_paths_fill_on_their_own() {
        let mut viewers = CameraViewers::new(2);
        assert_eq!(viewers.join(1, "AA", BOTH, CameraPath::Relay), Join::Admit);
        assert_eq!(viewers.join(2, "bb", BOTH, CameraPath::Direct), Join::Admit);
        // A direct viewer does not take a relay seat (FR-8.93).
        assert_eq!(viewers.join(3, "cc", BOTH, CameraPath::Relay), Join::Admit);
        assert!(viewers.path_full(CameraPath::Relay));
        assert!(!viewers.path_full(CameraPath::Direct));
        assert_eq!(viewers.path_of(2), Some(CameraPath::Direct));
        assert_eq!(viewers.join(4, "dd", BOTH, CameraPath::Relay), Join::Full);
        assert_eq!(viewers.join(5, "ee", BOTH, CameraPath::Direct), Join::Admit);
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
                },
                CameraViewer {
                    session: 3,
                    path: CameraPath::Relay
                },
                CameraViewer {
                    session: 5,
                    path: CameraPath::Direct
                }
            ]
        );
        // A leave frees the seat and hands back the hash and path for the rekey.
        assert_eq!(viewers.leave(1), Some(("aa".to_owned(), CameraPath::Relay)));
        assert_eq!(viewers.leave(1), None);
        assert!(!viewers.path_full(CameraPath::Relay));
        assert_eq!(viewers.join(4, "dd", BOTH, CameraPath::Relay), Join::Admit);
        assert!(viewers.contains(4));
    }

    #[test]
    fn the_direct_path_fills_at_eight_while_the_relay_still_has_room() {
        let mut viewers = CameraViewers::new(MAX_VIEWERS);
        for viewer in 0..u32::try_from(crate::camera::direct::MAX_DIRECT_VIEWERS).expect("fits") {
            assert_eq!(
                viewers.join(viewer, "h", BOTH, CameraPath::Direct),
                Join::Admit
            );
        }
        assert!(viewers.path_full(CameraPath::Direct));
        assert!(!viewers.path_full(CameraPath::Relay));
        assert_eq!(viewers.join(8, "h", BOTH, CameraPath::Direct), Join::Full);
        assert_eq!(viewers.join(100, "h", BOTH, CameraPath::Relay), Join::Admit);
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
