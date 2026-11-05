//! The Apps menu (since 1.1). Items are declared in the manifest (`app_menu`); the plugin gets
//! [`crate::events::Event::AppMenuActivated`] with the item id when the user picks one, and can
//! grey an item out with [`set_state`], giving the user a reason.

pub use crate::bindings::mumcord::plugin::app_menu::*;

/// Enables an item again after [`disable`].
pub fn enable(item: &str) -> Result<(), Error> {
    set_state(item, true, None)
}

/// Greys an item out; `reason` is shown to the user as the item's explanation.
pub fn disable(item: &str, reason: &str) -> Result<(), Error> {
    set_state(item, false, Some(reason))
}
