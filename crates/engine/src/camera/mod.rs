//! Camera sharing (spec 08): the engine side of the camera room.

mod admit;
pub mod avail;
pub mod backgrounds;
mod local;
#[cfg(target_os = "linux")]
mod lock_linux;
#[cfg(target_os = "macos")]
mod lock_macos;
#[cfg(windows)]
mod lock_windows;
pub mod preview;
pub mod recv;
mod send;
pub mod session;
pub mod signal;
pub(crate) mod stop;

pub use mumcord_camera::{CameraDevice, CameraError, DeviceId};
pub use mumcord_effects::{
    Background, EffectStats, EffectsError, Level as EffectLevel, Load as EffectLoad,
};
pub use preview::{PreviewPicture, PreviewSink};
pub use recv::{CameraRoomView, CameraTile, TileQuality, TileState};
pub use send::SendEnded;
pub use session::{CameraFailure, CameraState, CameraView};
pub use stop::StopReason;
