//! Stage activities (since 1.1): a shared activity for the channel, drawn either from a node
//! tree or from a web page. Needs the `stage` permission.
//!
//! ```no_run
//! use mumcord_plugin_sdk::{stage, ui};
//!
//! # fn demo(board: ui::El) -> Result<(), mumcord_plugin_sdk::types::Error> {
//! let activity = stage::start_tree("Tic-tac-toe", board)?;
//! # let _ = activity;
//! # Ok(())
//! # }
//! ```

pub use crate::bindings::mumcord::plugin::stage::*;
use crate::ui::El;

/// Starts an activity whose content is drawn from `tree`; update it with [`show_tree`].
pub fn start_tree(title: &str, tree: El) -> Result<ActivityId, Error> {
    start(title, &StageContent::Tree(tree.into_nodes()))
}

/// Replaces the tree of the running activity.
pub fn show_tree(tree: El) -> Result<(), Error> {
    set_tree(&tree.into_nodes())
}

/// Starts an activity whose content is the web page `entry`, a path inside the plugin package.
pub fn start_web(title: &str, entry: &str) -> Result<ActivityId, Error> {
    start(title, &StageContent::Web(entry.to_owned()))
}
