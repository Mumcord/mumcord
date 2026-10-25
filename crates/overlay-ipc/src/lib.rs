//! In-game overlay (FR-1.46b): the local channel between Mumcord and the code drawn inside
//! games (the Linux Vulkan layer / OpenGL preload, the Windows graphics hook).
//!
//! Mumcord listens on a per-user endpoint ([`endpoint`]). A game that loaded the layer connects,
//! says who it is ([`Hello`]) and Mumcord answers with the per-game decision and options
//! ([`Config`]) and, while allowed, who is talking ([`Scene`]). Mumcord sends a [`Message::Heartbeat`]
//! every second; the game side treats a silent connection as stale and draws nothing.
//!
//! Everything here is std-only (plus `libc` on Linux for the peer check) so the game-side
//! libraries stay small. The renderer that turns a scene into pixels is behind the `render`
//! feature.

pub mod client;
pub mod endpoint;
#[cfg(feature = "render")]
pub mod render;
pub mod wire;

pub use wire::{Api, Config, Corner, DecodeError, Hello, Message, Person, Scene};
