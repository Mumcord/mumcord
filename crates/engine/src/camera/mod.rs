//! Camera sharing (spec 08): the engine side of the camera room.

mod admit;
mod send;
pub mod session;
pub mod signal;

pub use send::{FrameEffect, SendEnded};
pub use session::{CameraFailure, CameraState, CameraView};
