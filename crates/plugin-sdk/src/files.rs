//! Host-owned file dialogs (since 1.1): the plugin never sees a path, only the bytes the user
//! chose to save or import. Both calls open a system dialog and need a recent user gesture.

pub use crate::bindings::mumcord::plugin::files::*;
