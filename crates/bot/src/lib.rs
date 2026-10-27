//! Scripted bot clients for integration/E2E tests and manual checks against the local test
//! servers in `testenv/`. Bots are ordinary Mumble clients built on `mumcord-protocol`.
//!
//! Voice (playing/receiving audio) is in [`voice`]; it builds on [`Bot::handle`] and
//! [`Bot::subscribe`].

mod acl;
mod bot;
pub mod cert;
pub mod checks;
pub mod env;
pub mod samples;
pub mod voice;
pub mod webhook;

pub use bot::{Bot, BotConfig, DEFAULT_TIMEOUT, Moderation};
pub use checks::{MessageCheck, UserCheck};
pub use mumcord_protocol as protocol;
