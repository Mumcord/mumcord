//! Activity-log events (since 1.1): one line in the Activity tab of the server the user is on.
//! Needs the `activity` permission.
//!
//! The host removes control characters from `text` and caps it at 128 characters; the action label
//! is capped at 24. Clicking the action on the event arrives as the existing
//! [`crate::events::Event::UiAction`] with the same `id`, so there is no separate callback. At
//! most one push per 10 seconds and 50 per hour per plugin.
//!
//! ```no_run
//! use mumcord_plugin_sdk::{activity, types::Error};
//!
//! # fn demo() -> Result<(), Error> {
//! activity::push(
//!     "Raid starts in five minutes",
//!     Some(&activity::Action {
//!         label: "Notify me".into(),
//!         id: "raid".into(),
//!     }),
//! )?;
//! # Ok(())
//! # }
//! ```

pub use crate::bindings::mumcord::plugin::activity::*;
