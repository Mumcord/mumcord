//! `stage` (FR-9.44 … FR-9.52): plugin activities in the media area. Needs `ui.stage`.
//!
//! These functions check what only the plugin's own state can tell (permissions, the title, the
//! tree and its update rate) and hand the call to [`crate::api::io::PluginIo`], where the
//! service checks the gesture, the channel and the limits ([`crate::stage::Stages`]).

use std::sync::PoisonError;
use std::time::Instant;

use crate::api::io::StageContent;
use crate::api::ui::validate;
use crate::runtime::wit::types::Error;
use crate::runtime::wit::ui::Node;
use crate::runtime::{Level, PluginState};
use crate::stage::announce::is_title;

/// The permission every `stage` call needs.
pub const PERMISSION: &str = "ui.stage";
/// The key the stage tree counts its updates under (20 per second, as `ui.set-tree`).
const RATE_KEY: &str = "stage";

/// `stage.start`.
pub(crate) fn start(
    state: &mut PluginState,
    title: String,
    content: StageContent,
) -> Result<String, Error> {
    let grants = &state.ctx.grants;
    grants.check(&state.id, PERMISSION)?;
    if !is_title(&title) {
        return Err(Error::Invalid);
    }
    match &content {
        StageContent::Tree(nodes) => check_tree(state, nodes)?,
        StageContent::Web(_) => grants.check(&state.id, crate::api::webview::PERMISSION)?,
    }
    state.io()?.stage_start(&state.id, title, content)
}

/// `stage.join`.
pub(crate) fn join(state: &PluginState, activity: &str) -> Result<(), Error> {
    state.ctx.grants.check(&state.id, PERMISSION)?;
    state.io()?.stage_join(&state.id, activity)
}

/// `stage.leave`.
pub(crate) fn leave(state: &PluginState) {
    if state.ctx.grants.allows(&state.id, PERMISSION)
        && let Ok(io) = state.io()
    {
        io.stage_leave(&state.id);
    }
}

/// `stage.end`.
pub(crate) fn end(state: &PluginState) {
    if state.ctx.grants.allows(&state.id, PERMISSION)
        && let Ok(io) = state.io()
    {
        io.stage_end(&state.id);
    }
}

/// `stage.set-tree`.
pub(crate) fn set_tree(state: &mut PluginState, nodes: Vec<Node>) -> Result<(), Error> {
    state.ctx.grants.check(&state.id, PERMISSION)?;
    check_tree(state, &nodes)?;
    state.io()?.stage_set_tree(&state.id, nodes)
}

/// The same tree rules and update rate as `ui.set-tree`.
fn check_tree(state: &mut PluginState, nodes: &[Node]) -> Result<(), Error> {
    if !state.ui.allow_key(RATE_KEY.to_owned(), Instant::now()) {
        reject(state, "more than 20 updates per second");
        return Err(Error::RateLimited);
    }
    let grants = &state.ctx.grants;
    if let Err(e) = validate(nodes, |permission| grants.allows(&state.id, permission)) {
        reject(state, &e.reason);
        return Err(e.error);
    }
    Ok(())
}

/// Leaves a content-free line in the plugin's Diagnostics log.
fn reject(state: &PluginState, reason: &str) {
    state
        .log
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(Level::Warn, &format!("Stage tree not shown: {reason}"));
}
