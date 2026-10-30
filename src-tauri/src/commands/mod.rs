//! Typed commands and events shared with the UI.
//!
//! Each area lives in its own file so later tasks can own exactly one file. The
//! human-readable contract is `docs/engine-api.md`; the TypeScript bindings are generated
//! into `ui/src/bindings/commands.ts`.

pub mod app;
pub mod audio;
pub mod cache;
pub mod changelog;
pub mod channels;
pub mod chat;
pub mod diagnostics;
pub mod dropped;
pub mod follow;
pub mod friends;
pub mod full_image;
pub mod history;
pub mod identity;
pub mod ingame_overlay;
pub mod inspector;
pub mod licences;
pub mod links;
pub mod notifications;
pub mod now_gaming;
pub mod now_playing;
pub mod p2p;
pub mod people;
pub mod positional;
pub mod profile;
pub mod ptt;
pub mod public_servers;
pub mod screen_share;
pub mod servers;
pub mod session;
pub mod settings;
pub mod types;
pub mod updates;
pub mod user_stats;
pub mod watch_party;
