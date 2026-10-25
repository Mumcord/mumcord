//! Mumcord's in-game overlay for Linux (FR-1.46b), drawn inside games:
//! - a Vulkan layer (`VK_LAYER_MUMCORD_overlay`) that Mumcord registers as an implicit layer
//!   while the in-game overlay is on, so every Vulkan game (also DXVK/VKD3D under Proton) loads it;
//! - an OpenGL fallback for `LD_PRELOAD` (`glXSwapBuffers` / `eglSwapBuffers`).
//!
//! Both draw what Mumcord sends over the local IPC channel (`mumcord-overlay-ipc`) and draw
//! nothing while Mumcord is not running, the overlay is off for the game, or anything fails.
//! See `docs/engine-api.md` (In-game overlay).

#[cfg(target_os = "linux")]
mod gl;
#[cfg(target_os = "linux")]
mod log;
#[cfg(target_os = "linux")]
mod shared;
#[cfg(target_os = "linux")]
pub mod vulkan;
