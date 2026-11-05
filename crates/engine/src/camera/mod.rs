//! Camera sharing (spec 08): the engine side of the camera room.

mod admit;
pub mod avail;
pub mod recv;
mod send;
pub mod session;
pub mod signal;

pub use recv::{CameraRoomView, CameraTile, TileQuality, TileState};
pub use send::{FrameEffect, SendEnded};
pub use session::{CameraFailure, CameraState, CameraView};
