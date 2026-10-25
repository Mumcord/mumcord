//! Position sources for positional audio (FR-1.41–1.43).
//!
//! A [`PositionSource`] reports where the player's avatar and camera are while a game delivers
//! positions. Today's source is the Mumble Link shared memory ([`link`]); the official game
//! plugins (B31) plug in as further sources. Coordinates follow Mumble's convention:
//! left-handed, X to the right, Y up, Z forward, in metres.

pub mod link;
mod shm;

use std::time::Instant;

pub use link::LinkSource;

/// Position and orientation (unit vectors) of the avatar or the camera.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Pose {
    pub position: [f32; 3],
    pub front: [f32; 3],
    pub top: [f32; 3],
}

/// What an active game delivers.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Positional {
    /// Sent with every voice packet: where the others hear us.
    pub avatar: Pose,
    /// Where we listen from.
    pub camera: Pose,
    /// Game context as sent to the server (`UserState.plugin_context`, already prefixed with
    /// the plugin name like the official client does). Positions are only exchanged between
    /// users with the same context.
    pub context: Vec<u8>,
    /// Player identity as sent to the server (`UserState.plugin_identity`).
    pub identity: String,
}

/// A game currently delivering positions.
#[derive(Debug, Clone, PartialEq)]
pub struct Fetch {
    /// Display name of the source, e.g. "Mumble Link" or a plugin name.
    pub source: String,
    /// Game name as reported by the game, if any.
    pub game: Option<String>,
    pub data: Positional,
}

/// One way of getting positions from a game. Polled about every 20 ms off the audio threads;
/// `fetch` may do (cheap) I/O.
pub trait PositionSource: Send {
    /// The current positions, or `None` while no game delivers any.
    fn fetch(&mut self, now: Instant) -> Option<Fetch>;
}
