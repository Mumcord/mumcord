//! Who may watch the own camera (spec 08, FR-8.47 – FR-8.49, FR-8.52).
//!
//! Admission is automatic: a `cam-join` from a Mumcord user in the same channel who is not
//! blocked (checked before it gets here, `signal.rs` and the session's block filter) is admitted
//! through the bot while the broadcast has room. A `cam-leave`, a viewer leaving the channel or
//! the server ends the admission: the bot is told (`bcast-kick`) and the key rotates (FR-7.44).

use std::collections::BTreeMap;

use super::signal::Codec;

/// Viewers per camera (FR-8.52, relay `broadcast_full`).
pub const MAX_VIEWERS: u16 = 25;

/// What to do about a `cam-join`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Join {
    /// Ask the bot to admit the viewer, and let its certificate get the keys.
    Admit,
    /// The broadcast has no room; the viewer asks again later (FR-8.52).
    Full,
}

#[derive(Debug, Clone)]
struct Viewer {
    hash: String,
    av1: bool,
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

    /// `viewer` with certificate `hash` asked to join, playing `codecs`. A viewer who is already
    /// in is admitted again (it binds anew, for example after a reconnect of its link).
    pub fn join(&mut self, viewer: u32, hash: &str, codecs: &[Codec]) -> Join {
        if !self.viewers.contains_key(&viewer) && self.viewers.len() >= self.limit {
            return Join::Full;
        }
        self.viewers.insert(
            viewer,
            Viewer {
                hash: hash.to_ascii_lowercase(),
                av1: codecs.contains(&Codec::Av1),
            },
        );
        Join::Admit
    }

    /// `viewer` left or was refused by the bot; returns its certificate hash so the publisher
    /// drops it and rekeys, or `None` when it was not admitted.
    pub fn leave(&mut self, viewer: u32) -> Option<String> {
        self.viewers.remove(&viewer).map(|v| v.hash)
    }

    pub fn contains(&self, viewer: u32) -> bool {
        self.viewers.contains_key(&viewer)
    }

    /// Some admitted viewer cannot play AV1, so the camera sends VP9 (FR-8.43, FR-7.58).
    pub fn needs_vp9(&self) -> bool {
        self.viewers.values().any(|v| !v.av1)
    }

    pub fn sessions(&self) -> Vec<u32> {
        self.viewers.keys().copied().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOTH: &[Codec] = &[Codec::Av1, Codec::Vp9];

    #[test]
    fn viewers_are_admitted_until_the_broadcast_is_full() {
        let mut viewers = CameraViewers::new(2);
        assert_eq!(viewers.join(1, "AA", BOTH), Join::Admit);
        assert_eq!(viewers.join(2, "bb", BOTH), Join::Admit);
        assert_eq!(viewers.join(3, "cc", BOTH), Join::Full);
        // Joining again is fine for someone who is in.
        assert_eq!(viewers.join(1, "aa", BOTH), Join::Admit);
        assert_eq!(viewers.sessions(), [1, 2]);
        // A leave frees the seat and hands back the hash for the rekey.
        assert_eq!(viewers.leave(1).as_deref(), Some("aa"));
        assert_eq!(viewers.leave(1), None);
        assert_eq!(viewers.join(3, "cc", BOTH), Join::Admit);
        assert!(viewers.contains(3));
    }

    #[test]
    fn the_limit_is_at_most_twenty_five() {
        let mut viewers = CameraViewers::new(100);
        for v in 0..u32::from(MAX_VIEWERS) {
            assert_eq!(viewers.join(v, "h", BOTH), Join::Admit);
        }
        assert_eq!(viewers.join(99, "h", BOTH), Join::Full);
    }

    #[test]
    fn a_vp9_only_viewer_switches_the_camera_to_vp9_while_it_watches() {
        let mut viewers = CameraViewers::new(MAX_VIEWERS);
        viewers.join(1, "a", BOTH);
        assert!(!viewers.needs_vp9());
        viewers.join(2, "b", &[Codec::Vp9]);
        assert!(viewers.needs_vp9());
        viewers.leave(2);
        assert!(!viewers.needs_vp9());
    }
}
