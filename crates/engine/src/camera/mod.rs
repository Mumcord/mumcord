//! Camera sharing (spec 08): the engine side of the camera room.

mod admit;
pub mod avail;
#[cfg(target_os = "linux")]
mod lock_linux;
#[cfg(target_os = "macos")]
mod lock_macos;
#[cfg(windows)]
mod lock_windows;
pub mod recv;
mod send;
pub mod session;
pub mod signal;
pub(crate) mod stop;

pub use recv::{CameraRoomView, CameraTile, TileQuality, TileState};
pub use send::{FrameEffect, SendEnded};
pub use session::{CameraFailure, CameraState, CameraView};
pub use stop::StopReason;
