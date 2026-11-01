//! `ui` (FR-6.40 … FR-6.42): plugin UI trees, the dialog and the user-menu entry. Each slot needs
//! its own permission. Trees are checked against the FR-6.42 limits here, before the
//! [`PluginIo`](super::io::PluginIo) sees them; a rejected tree leaves a line in the plugin's
//! Diagnostics log.

use std::collections::HashMap;
use std::sync::PoisonError;
use std::time::{Duration, Instant};

use wasmtime::component::{HasSelf, Linker};

use crate::api::theme::TOKENS;
use crate::runtime::bindings::mumcord::plugin::ui::{
    Component, Host, Node, Slot, UserMenuFilter, add_to_linker,
};
use crate::runtime::wit::types::Error;
use crate::runtime::{HostApi, Level, PluginState};

pub const PANEL_PERMISSION: &str = "ui.panel";
pub const DIALOG_PERMISSION: &str = "ui.dialog";
pub const USER_MENU_PERMISSION: &str = "ui.user-menu";

/// Characters of a user-menu label.
pub const MAX_MENU_LABEL_CHARS: usize = 40;
/// Sessions in a user-menu filter.
pub const MAX_MENU_SESSIONS: usize = 256;

/// Nodes per tree (FR-6.42).
pub const MAX_NODES: usize = 500;
/// Characters in a dialog title or subtitle.
pub const MAX_DIALOG_TITLE_CHARS: usize = 64;
/// Encoded tree size per slot (FR-6.42), see [`encoded_size`].
pub const MAX_TREE_BYTES: usize = 16 * 1024;
/// `set-tree` calls per slot and second (FR-6.42).
pub const MAX_UPDATES_PER_SECOND: u32 = 20;
/// Characters per string (FR-6.42).
pub const MAX_TEXT_CHARS: usize = 200;
/// Rows of a list (FR-6.41).
pub const MAX_LIST_ROWS: usize = 50;
/// Sprite frame rate (FR-6.41).
pub const MAX_SPRITE_FPS: u8 = 12;
/// Bytes of a text input (FR-6.41).
pub const MAX_INPUT_BYTES: u8 = 64;
/// Bytes of a `copy-button` text (UTF-8). Exempt from the per-string limit, but still counted in
/// the tree size.
pub const MAX_COPY_BYTES: usize = 8 * 1024;
/// Spacing of a stack in px.
pub const MAX_GAP: u8 = 32;

/// Fixed share of every node in [`encoded_size`]: its tag, numbers and flags.
const NODE_OVERHEAD: usize = 16;
/// Slots remembered by the rate limiter before old windows are dropped (chat-card ids are
/// unbounded).
const MAX_TRACKED_SLOTS: usize = 64;
const WINDOW: Duration = Duration::from_secs(1);

pub(crate) struct UiApi;

impl HostApi for UiApi {
    fn add_to_linker(linker: &mut Linker<PluginState>) -> wasmtime::Result<()> {
        add_to_linker::<PluginState, HasSelf<PluginState>>(linker, |s| s)
    }
}

/// The permission a slot needs; the settings form needs none.
pub fn slot_permission(slot: &Slot) -> Option<&'static str> {
    match slot {
        Slot::Panel => Some(PANEL_PERMISSION),
        Slot::MemberBadge(_) => Some("ui.member-badge"),
        Slot::ProfileCard(_) => Some("ui.profile-card"),
        Slot::ChatCard(_) | Slot::ChatDecoration(_) => Some("ui.chat-card"),
        Slot::Settings => None,
        Slot::Dialog => Some(DIALOG_PERMISSION),
    }
}

/// A short, content-free name of a slot for the rate limiter and Diagnostics lines.
/// Dialog title and subtitle: at most [`MAX_DIALOG_TITLE_CHARS`] each.
fn check_dialog_text(title: &str, subtitle: Option<&str>) -> Result<(), Error> {
    let long = |s: &str| s.chars().count() > MAX_DIALOG_TITLE_CHARS;
    if long(title) || subtitle.is_some_and(long) {
        Err(Error::TooLarge)
    } else {
        Ok(())
    }
}

/// A user-menu label: not blank, at most [`MAX_MENU_LABEL_CHARS`].
fn check_menu_label(label: &str) -> Result<(), Error> {
    if label.trim().is_empty() {
        Err(Error::Invalid)
    } else if label.chars().count() > MAX_MENU_LABEL_CHARS {
        Err(Error::TooLarge)
    } else {
        Ok(())
    }
}

fn slot_key(slot: &Slot) -> String {
    match slot {
        Slot::Panel => "panel".to_owned(),
        Slot::MemberBadge(s) => format!("member-badge:{s}"),
        Slot::ProfileCard(s) => format!("profile-card:{s}"),
        Slot::ChatCard(id) => format!("chat-card:{id}"),
        Slot::ChatDecoration(s) => format!("chat-decoration:{s}"),
        Slot::Settings => "settings".to_owned(),
        Slot::Dialog => "dialog".to_owned(),
    }
}

fn slot_name(slot: &Slot) -> &'static str {
    match slot {
        Slot::Panel => "panel",
        Slot::MemberBadge(_) => "member-badge",
        Slot::ProfileCard(_) => "profile-card",
        Slot::ChatCard(_) => "chat-card",
        Slot::ChatDecoration(_) => "chat-decoration",
        Slot::Settings => "settings",
        Slot::Dialog => "dialog",
    }
}

/// Counts `set-tree` calls per slot in fixed one-second windows.
#[derive(Debug, Default)]
pub(crate) struct UiLimiter {
    windows: HashMap<String, (Instant, u32)>,
}

impl UiLimiter {
    fn allow(&mut self, slot: &Slot, now: Instant) -> bool {
        if self.windows.len() >= MAX_TRACKED_SLOTS {
            self.windows
                .retain(|_, (start, _)| now.duration_since(*start) < WINDOW);
        }
        let key = slot_key(slot);
        if self.windows.len() >= MAX_TRACKED_SLOTS && !self.windows.contains_key(&key) {
            return false;
        }
        let (start, count) = self.windows.entry(key).or_insert((now, 0));
        if now.duration_since(*start) >= WINDOW {
            *start = now;
            *count = 0;
        }
        if *count >= MAX_UPDATES_PER_SECOND {
            return false;
        }
        *count += 1;
        true
    }
}

/// Why a tree was refused. The message names the rule and node index, never content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeError {
    pub error: Error,
    pub reason: String,
}

fn invalid(node: usize, rule: &str) -> TreeError {
    TreeError {
        error: Error::Invalid,
        reason: format!("node {node}: {rule}"),
    }
}

/// The permissions individual components need beyond the slot's own.
fn component_permission(component: &Component) -> Option<&'static str> {
    match component {
        Component::Password(_) => Some("secrets"),
        Component::FilePicker(_) => Some("net.http"),
        _ => None,
    }
}

/// The strings of a component, for the length and size checks.
fn strings(component: &Component) -> Vec<&str> {
    match component {
        Component::Stack(_) | Component::Divider => vec![],
        Component::Text(t) => [Some(t.text.as_str()), t.color.as_deref()]
            .into_iter()
            .flatten()
            .collect(),
        Component::Image(i) => vec![&i.asset, &i.alt],
        Component::Sprite(s) => vec![&s.asset, &s.alt],
        Component::Meter(m) => [Some(m.label.as_str()), m.color.as_deref()]
            .into_iter()
            .flatten()
            .collect(),
        Component::Button(b) => vec![&b.label, &b.action],
        Component::IconButton(b) => vec![&b.icon, &b.label, &b.action],
        Component::Chip(c) => [Some(c.text.as_str()), c.color.as_deref()]
            .into_iter()
            .flatten()
            .collect(),
        Component::Progress(p) => vec![&p.label],
        Component::List(l) => [Some(l.label.as_str()), l.reorder_action.as_deref()]
            .into_iter()
            .flatten()
            .collect(),
        Component::Toggle(t) => vec![&t.label, &t.action],
        Component::Select(s) => {
            let mut all: Vec<&str> = vec![&s.label, &s.action];
            all.extend(s.selected.as_deref());
            all.extend(s.refresh_action.as_deref());
            for option in &s.options {
                all.push(&option.value);
                all.push(&option.label);
            }
            all
        }
        Component::Slider(s) => vec![&s.label, &s.action],
        Component::TextInput(t) => {
            let mut all: Vec<&str> = vec![&t.label, &t.value, &t.placeholder, &t.action];
            all.extend(t.help.as_deref());
            all.extend(t.error.as_deref());
            all
        }
        Component::Password(p) => {
            let mut all: Vec<&str> = vec![&p.label, &p.secret, &p.action];
            all.extend(p.help.as_deref());
            all.extend(p.error.as_deref());
            all
        }
        Component::FilePicker(f) => {
            let mut all: Vec<&str> = vec![&f.label, &f.action];
            all.extend(f.accept.iter().map(String::as_str));
            all
        }
        Component::Countdown(c) => vec![&c.label],
        Component::CopyButton(c) => vec![&c.label],
    }
}

/// The text a `copy-button` puts on the clipboard; kept out of [`strings`] because it has its
/// own, larger limit.
fn copy_text(component: &Component) -> Option<&str> {
    match component {
        Component::CopyButton(c) => Some(&c.text),
        _ => None,
    }
}

/// The accessible label of an interactive component; `None` for display-only ones.
fn required_label(component: &Component) -> Option<&str> {
    match component {
        Component::Button(b) => Some(&b.label),
        Component::IconButton(b) => Some(&b.label),
        Component::CopyButton(b) => Some(&b.label),
        Component::Toggle(t) => Some(&t.label),
        Component::Select(s) => Some(&s.label),
        Component::Slider(s) => Some(&s.label),
        Component::TextInput(t) => Some(&t.label),
        Component::Password(p) => Some(&p.label),
        Component::FilePicker(f) => Some(&f.label),
        Component::List(l) if l.reorder_action.is_some() => Some(&l.label),
        _ => None,
    }
}

fn actions(component: &Component) -> Vec<&str> {
    match component {
        Component::Button(b) => vec![&b.action],
        Component::IconButton(b) => vec![&b.action],
        Component::Toggle(t) => vec![&t.action],
        Component::Select(s) => {
            let mut all: Vec<&str> = vec![&s.action];
            all.extend(s.refresh_action.as_deref());
            all
        }
        Component::Slider(s) => vec![&s.action],
        Component::TextInput(t) => vec![&t.action],
        Component::Password(p) => vec![&p.action],
        Component::FilePicker(f) => vec![&f.action],
        Component::List(l) => l.reorder_action.as_deref().into_iter().collect(),
        _ => vec![],
    }
}

fn color(component: &Component) -> Option<&str> {
    match component {
        Component::Text(t) => t.color.as_deref(),
        Component::Meter(m) => m.color.as_deref(),
        Component::Chip(c) => c.color.as_deref(),
        _ => None,
    }
}

fn assets(component: &Component) -> Vec<&str> {
    match component {
        Component::Image(i) => vec![&i.asset],
        Component::Sprite(s) => vec![&s.asset],
        Component::IconButton(b) => vec![&b.icon],
        _ => vec![],
    }
}

/// A path inside the package's `assets/` folder, without tricks.
pub fn is_asset_path(path: &str) -> bool {
    path.strip_prefix("assets/").is_some_and(|rest| {
        !rest.is_empty()
            && rest
                .split('/')
                .all(|part| !part.is_empty() && part != "." && part != "..")
            && !path.contains('\\')
            && !path.contains(':')
    })
}

fn unit(value: f32) -> bool {
    (0.0..=1.0).contains(&value)
}

/// Component-specific value rules.
fn check_values(index: usize, component: &Component) -> Result<(), TreeError> {
    match component {
        Component::Stack(s) if s.gap > MAX_GAP => Err(invalid(index, "gap above 32")),
        Component::Sprite(s) => {
            if s.frames == 0 || s.fps == 0 || s.fps > MAX_SPRITE_FPS {
                Err(invalid(
                    index,
                    "sprite needs 1 to 12 fps and at least one frame",
                ))
            } else if s.still_frame >= s.frames {
                Err(invalid(index, "sprite still frame out of range"))
            } else if s.frame_width == 0 || s.frame_height == 0 {
                Err(invalid(index, "sprite frame size is zero"))
            } else {
                Ok(())
            }
        }
        Component::Meter(m) if !unit(m.value) => Err(invalid(index, "meter value outside 0..1")),
        Component::Progress(p) if p.value.is_some_and(|v| !unit(v)) => {
            Err(invalid(index, "progress value outside 0..1"))
        }
        Component::Slider(s)
            if !(s.min.is_finite()
                && s.max.is_finite()
                && s.step.is_finite()
                && s.value.is_finite()
                && s.min < s.max
                && s.step > 0.0
                && (s.min..=s.max).contains(&s.value)) =>
        {
            Err(invalid(index, "slider range"))
        }
        Component::TextInput(t)
            if t.max_bytes == 0
                || t.max_bytes > MAX_INPUT_BYTES
                || t.value.len() > usize::from(t.max_bytes) =>
        {
            Err(invalid(index, "text input size"))
        }
        Component::Select(s)
            if s.selected
                .as_ref()
                .is_some_and(|sel| !s.options.iter().any(|o| &o.value == sel)) =>
        {
            Err(invalid(index, "selected value is not an option"))
        }
        _ => Ok(()),
    }
}

/// The size of a tree as the host counts it: every string's bytes, four bytes per child index
/// and a fixed share per node. Deterministic, so plugin authors can reproduce it.
pub fn encoded_size(nodes: &[Node]) -> usize {
    nodes
        .iter()
        .map(|node| {
            NODE_OVERHEAD
                + 4 * node.children.len()
                + strings(&node.component)
                    .iter()
                    .map(|s| s.len())
                    .sum::<usize>()
                + copy_text(&node.component).map_or(0, str::len)
        })
        .sum()
}

/// Checks a tree against FR-6.40 … FR-6.42. `allows` answers whether the plugin holds a
/// permission (password and file-picker need their own).
pub fn validate(nodes: &[Node], allows: impl Fn(&str) -> bool) -> Result<(), TreeError> {
    if nodes.is_empty() {
        return Err(invalid(0, "empty tree"));
    }
    if nodes.len() > MAX_NODES {
        return Err(TreeError {
            error: Error::TooLarge,
            reason: format!("{} nodes, at most {MAX_NODES}", nodes.len()),
        });
    }
    let size = encoded_size(nodes);
    if size > MAX_TREE_BYTES {
        return Err(TreeError {
            error: Error::TooLarge,
            reason: format!("{size} bytes, at most {MAX_TREE_BYTES}"),
        });
    }
    let mut has_parent = vec![false; nodes.len()];
    for (index, node) in nodes.iter().enumerate() {
        let component = &node.component;
        if let Some(permission) = component_permission(component)
            && !allows(permission)
        {
            return Err(TreeError {
                error: Error::NotPermitted,
                reason: format!("node {index}: needs {permission}"),
            });
        }
        if strings(component)
            .iter()
            .any(|s| s.chars().count() > MAX_TEXT_CHARS)
        {
            return Err(TreeError {
                error: Error::TooLarge,
                reason: format!("node {index}: text above {MAX_TEXT_CHARS} characters"),
            });
        }
        if let Some(text) = copy_text(component) {
            if text.len() > MAX_COPY_BYTES {
                return Err(TreeError {
                    error: Error::TooLarge,
                    reason: format!("node {index}: copy text above {MAX_COPY_BYTES} bytes"),
                });
            }
            if text.is_empty() {
                return Err(invalid(index, "empty copy text"));
            }
        }
        if required_label(component).is_some_and(|label| label.trim().is_empty()) {
            return Err(invalid(index, "missing accessible label"));
        }
        if actions(component).iter().any(|a| a.is_empty()) {
            return Err(invalid(index, "empty action"));
        }
        if color(component).is_some_and(|c| !TOKENS.contains(&c)) {
            return Err(invalid(index, "unknown colour token"));
        }
        if assets(component).iter().any(|a| !is_asset_path(a)) {
            return Err(invalid(index, "asset path outside assets/"));
        }
        check_values(index, component)?;
        let container = matches!(component, Component::Stack(_) | Component::List(_));
        if !container && !node.children.is_empty() {
            return Err(invalid(index, "only stack and list may have children"));
        }
        if matches!(component, Component::List(_)) && node.children.len() > MAX_LIST_ROWS {
            return Err(TreeError {
                error: Error::TooLarge,
                reason: format!("node {index}: more than {MAX_LIST_ROWS} list rows"),
            });
        }
        for &child in &node.children {
            let child = child as usize;
            // Children come after their parent, which also rules out cycles.
            if child <= index || child >= nodes.len() {
                return Err(invalid(index, "child index out of range"));
            }
            if std::mem::replace(&mut has_parent[child], true) {
                return Err(invalid(child, "more than one parent"));
            }
        }
    }
    if let Some(orphan) = has_parent.iter().skip(1).position(|p| !p) {
        return Err(invalid(orphan + 1, "not reachable from the root"));
    }
    Ok(())
}

impl PluginState {
    fn check_slot(&self, slot: &Slot) -> Result<(), Error> {
        match slot_permission(slot) {
            Some(permission) => self.ctx.grants.check(&self.id, permission),
            None => Ok(()),
        }
    }

    /// Leaves a content-free line in the plugin's Diagnostics log.
    fn reject_tree(&self, slot: &Slot, reason: &str) {
        self.log
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(
                Level::Warn,
                &format!("UI tree for {} not shown: {reason}", slot_name(slot)),
            );
    }
}

impl Host for PluginState {
    fn set_tree(&mut self, slot: Slot, nodes: Vec<Node>) -> Result<(), Error> {
        self.check_slot(&slot)?;
        if !self.ui.allow(&slot, Instant::now()) {
            self.reject_tree(&slot, "more than 20 updates per second");
            return Err(Error::RateLimited);
        }
        let grants = &self.ctx.grants;
        if let Err(e) = validate(&nodes, |permission| grants.allows(&self.id, permission)) {
            self.reject_tree(&slot, &e.reason);
            return Err(e.error);
        }
        self.io()?.set_tree(&self.id, slot, nodes, self.chat.server)
    }

    fn clear(&mut self, slot: Slot) {
        if self.check_slot(&slot).is_ok()
            && let Ok(io) = self.io()
        {
            io.clear(&self.id, slot);
        }
    }

    fn open_dialog(&mut self, title: String) -> Result<(), Error> {
        self.open_dialog_with(title, None)
    }

    fn open_dialog_with(&mut self, title: String, subtitle: Option<String>) -> Result<(), Error> {
        self.ctx.grants.check(&self.id, DIALOG_PERMISSION)?;
        check_dialog_text(&title, subtitle.as_deref())?;
        self.io()?.open_dialog(&self.id, title, subtitle)
    }

    fn close_dialog(&mut self) {
        if self.ctx.grants.allows(&self.id, DIALOG_PERMISSION)
            && let Ok(io) = self.io()
        {
            io.close_dialog(&self.id);
        }
    }

    fn set_user_menu_entry(&mut self, label: Option<String>) -> Result<(), Error> {
        self.ctx.grants.check(&self.id, USER_MENU_PERMISSION)?;
        if let Some(label) = &label {
            check_menu_label(label)?;
        }
        self.io()?.set_user_menu_entry(&self.id, label)
    }

    fn set_user_menu_entry_for(
        &mut self,
        label: String,
        filter: UserMenuFilter,
    ) -> Result<(), Error> {
        self.ctx.grants.check(&self.id, USER_MENU_PERMISSION)?;
        check_menu_label(&label)?;
        if filter.sessions.len() > MAX_MENU_SESSIONS {
            return Err(Error::TooLarge);
        }
        self.io()?.set_user_menu_entry_for(&self.id, label, filter)
    }

    fn open_panel_popover(&mut self) -> Result<(), Error> {
        self.ctx.grants.check(&self.id, PANEL_PERMISSION)?;
        self.io()?.open_panel_popover(&self.id)
    }

    fn close_panel_popover(&mut self) {
        if self.ctx.grants.allows(&self.id, PANEL_PERMISSION)
            && let Ok(io) = self.io()
        {
            io.close_panel_popover(&self.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::bindings::mumcord::plugin::ui::{
        Align, Button, ButtonKind, CopyButton, Direction, InputPurpose, ListProps, Sprite, Stack,
        Text, TextInput, TextStyle, Tone,
    };

    fn node(component: Component, children: Vec<u32>) -> Node {
        Node {
            component,
            animation: None,
            children,
        }
    }

    fn stack(children: Vec<u32>) -> Node {
        node(
            Component::Stack(Stack {
                direction: Direction::Column,
                gap: 4,
                align: Align::Start,
                justify: None,
            }),
            children,
        )
    }

    fn text(s: &str) -> Node {
        node(
            Component::Text(Text {
                text: s.to_owned(),
                style: TextStyle::Body,
                color: None,
            }),
            vec![],
        )
    }

    fn button(label: &str) -> Node {
        node(
            Component::Button(Button {
                label: label.to_owned(),
                action: "go".to_owned(),
                kind: ButtonKind::Primary,
                disabled: false,
                expanded: None,
            }),
            vec![],
        )
    }

    fn list(rows: usize) -> Vec<Node> {
        let mut nodes = vec![node(
            Component::List(ListProps {
                label: "Rows".to_owned(),
                reorder_action: Some("move".to_owned()),
            }),
            (1..=rows as u32).collect(),
        )];
        nodes.extend((0..rows).map(|_| text("row")));
        nodes
    }

    fn all(_: &str) -> bool {
        true
    }

    fn error(nodes: &[Node]) -> Error {
        validate(nodes, all).expect_err("refused").error
    }

    #[test]
    fn chat_decorations_share_the_chat_card_permission() {
        assert_eq!(
            slot_permission(&Slot::ChatDecoration(3)),
            Some("ui.chat-card")
        );
        assert_eq!(
            slot_permission(&Slot::ChatCard("c".into())),
            Some("ui.chat-card")
        );
        assert_eq!(slot_permission(&Slot::Settings), None);
        assert_eq!(slot_permission(&Slot::Dialog), Some(DIALOG_PERMISSION));
    }

    #[test]
    fn accepts_a_valid_tree() {
        assert_eq!(
            validate(&[stack(vec![1, 2]), text("hi"), button("Go")], all),
            Ok(())
        );
        assert_eq!(validate(&list(MAX_LIST_ROWS), all), Ok(()));
    }

    #[test]
    fn refuses_more_than_500_nodes() {
        let mut nodes = vec![stack((1..=MAX_NODES as u32).collect())];
        nodes.extend((0..MAX_NODES).map(|_| text("")));
        assert_eq!(error(&nodes), Error::TooLarge);
    }

    #[test]
    fn refuses_trees_above_16_kib() {
        let mut nodes = vec![stack((1..=100).collect())];
        nodes.extend((0..100).map(|_| text(&"x".repeat(MAX_TEXT_CHARS))));
        assert!(encoded_size(&nodes) > MAX_TREE_BYTES);
        assert_eq!(error(&nodes), Error::TooLarge);
    }

    #[test]
    fn refuses_long_text_counting_characters() {
        assert_eq!(validate(&[text(&"ä".repeat(MAX_TEXT_CHARS))], all), Ok(()));
        assert_eq!(
            error(&[text(&"x".repeat(MAX_TEXT_CHARS + 1))]),
            Error::TooLarge
        );
    }

    #[test]
    fn refuses_missing_labels_with_a_reason() {
        let e = validate(&[stack(vec![1]), button("  ")], all).expect_err("refused");
        assert_eq!(e.error, Error::Invalid);
        assert_eq!(e.reason, "node 1: missing accessible label");
    }

    #[test]
    fn refuses_bad_structure() {
        assert_eq!(error(&[]), Error::Invalid);
        assert_eq!(check_menu_label("Configure bot"), Ok(()));
        assert_eq!(check_menu_label(&"x".repeat(MAX_MENU_LABEL_CHARS)), Ok(()));
        assert_eq!(check_menu_label(&"x".repeat(41)), Err(Error::TooLarge));
        assert_eq!(check_menu_label(" "), Err(Error::Invalid));
        assert_eq!(error(&[stack(vec![5]), text("a")]), Error::Invalid);
        assert_eq!(error(&[stack(vec![0])]), Error::Invalid);
        assert_eq!(error(&[stack(vec![1, 1]), text("a")]), Error::Invalid);
        assert_eq!(error(&[stack(vec![]), text("orphan")]), Error::Invalid);
        let mut leaf_with_child = text("a");
        leaf_with_child.children = vec![1];
        assert_eq!(error(&[leaf_with_child, text("b")]), Error::Invalid);
        assert_eq!(error(&list(MAX_LIST_ROWS + 1)), Error::TooLarge);
    }

    #[test]
    fn colours_must_be_theme_tokens() {
        let mut t = text("a");
        if let Component::Text(inner) = &mut t.component {
            inner.color = Some("accent".to_owned());
        }
        assert_eq!(validate(&[t.clone()], all), Ok(()));
        if let Component::Text(inner) = &mut t.component {
            inner.color = Some("#ff0000".to_owned());
        }
        assert_eq!(error(&[t]), Error::Invalid);
    }

    #[test]
    fn checks_sprites_and_inputs() {
        let sprite = |fps, frames, still| {
            node(
                Component::Sprite(Sprite {
                    asset: "assets/pet.png".to_owned(),
                    alt: "Pet".to_owned(),
                    frame_width: 32,
                    frame_height: 32,
                    frames,
                    fps,
                    still_frame: still,
                }),
                vec![],
            )
        };
        assert_eq!(validate(&[sprite(12, 4, 3)], all), Ok(()));
        assert_eq!(error(&[sprite(13, 4, 0)]), Error::Invalid);
        assert_eq!(error(&[sprite(6, 4, 4)]), Error::Invalid);
        let input = |max_bytes| {
            node(
                Component::TextInput(TextInput {
                    label: "Name".to_owned(),
                    value: String::new(),
                    placeholder: String::new(),
                    max_bytes,
                    purpose: InputPurpose::Plain,
                    action: "name".to_owned(),
                    disabled: false,
                    help: None,
                    help_tone: None,
                    error: None,
                }),
                vec![],
            )
        };
        assert_eq!(validate(&[input(64)], all), Ok(()));
        assert_eq!(error(&[input(65)]), Error::Invalid);
    }

    #[test]
    fn field_notes_count_as_text() {
        let field = |help: &str, error: Option<&str>| {
            node(
                Component::TextInput(TextInput {
                    label: "Bot address".to_owned(),
                    value: String::new(),
                    placeholder: String::new(),
                    max_bytes: 64,
                    purpose: InputPurpose::ServerAddress,
                    action: "address".to_owned(),
                    disabled: false,
                    help: Some(help.to_owned()),
                    help_tone: Some(Tone::Success),
                    error: error.map(str::to_owned),
                }),
                vec![],
            )
        };
        let long = "x".repeat(MAX_TEXT_CHARS + 1);
        assert_eq!(
            validate(&[field("Reachable.", Some("Bot not reachable."))], all),
            Ok(())
        );
        assert_eq!(error(&[field(&long, None)]), Error::TooLarge);
        assert_eq!(error(&[field("Reachable.", Some(&long))]), Error::TooLarge);
    }

    #[test]
    fn copy_button_text_has_its_own_limit() {
        let copy = |label: &str, text: &str| {
            node(
                Component::CopyButton(CopyButton {
                    label: label.to_owned(),
                    text: text.to_owned(),
                    kind: ButtonKind::Secondary,
                }),
                vec![],
            )
        };
        // Far above the 200 character string limit, exactly at the byte limit.
        let max = "x".repeat(MAX_COPY_BYTES);
        assert_eq!(validate(&[copy("Copy fitting", &max)], all), Ok(()));
        assert_eq!(
            error(&[copy("Copy fitting", &format!("{max}x"))]),
            Error::TooLarge
        );
        // Bytes, not characters.
        assert_eq!(
            error(&[copy("Copy fitting", &"ä".repeat(MAX_COPY_BYTES / 2 + 1))]),
            Error::TooLarge
        );
        assert_eq!(error(&[copy("  ", "x")]), Error::Invalid);
        assert_eq!(error(&[copy("Copy", "")]), Error::Invalid);
        assert_eq!(
            error(&[copy(&"x".repeat(MAX_TEXT_CHARS + 1), "x")]),
            Error::TooLarge
        );
    }

    #[test]
    fn dialog_title_and_subtitle_are_limited() {
        let max = "ü".repeat(MAX_DIALOG_TITLE_CHARS);
        let over = "x".repeat(MAX_DIALOG_TITLE_CHARS + 1);
        assert_eq!(check_dialog_text(&max, Some(&max)), Ok(()));
        assert_eq!(check_dialog_text(&max, None), Ok(()));
        assert_eq!(check_dialog_text(&over, None), Err(Error::TooLarge));
        assert_eq!(
            check_dialog_text("Configure bot", Some(&over)),
            Err(Error::TooLarge)
        );
    }

    #[test]
    fn asset_paths_stay_inside_assets() {
        assert!(is_asset_path("assets/egg.svg"));
        assert!(is_asset_path("assets/pets/egg.png"));
        assert!(!is_asset_path("assets/../manifest.toml"));
        assert!(!is_asset_path("plugin.wasm"));
        assert!(!is_asset_path("assets/"));
        assert!(!is_asset_path("assets//x.png"));
    }

    #[test]
    fn password_and_file_picker_need_their_permission() {
        use crate::runtime::bindings::mumcord::plugin::ui::Password;
        let password = node(
            Component::Password(Password {
                label: "Token".to_owned(),
                secret: "token".to_owned(),
                action: "saved".to_owned(),
                disabled: false,
                help: None,
                help_tone: None,
                error: Some("Key rejected (401).".to_owned()),
            }),
            vec![],
        );
        assert_eq!(
            validate(std::slice::from_ref(&password), |p| p != "secrets")
                .expect_err("refused")
                .error,
            Error::NotPermitted
        );
        assert_eq!(validate(&[password], all), Ok(()));
    }

    #[test]
    fn limits_updates_per_slot_and_second() {
        let mut limiter = UiLimiter::default();
        let start = Instant::now();
        for _ in 0..MAX_UPDATES_PER_SECOND {
            assert!(limiter.allow(&Slot::Panel, start));
        }
        assert!(!limiter.allow(&Slot::Panel, start));
        assert!(limiter.allow(&Slot::Settings, start));
        assert!(limiter.allow(&Slot::Panel, start + WINDOW));
    }

    #[test]
    fn forgets_old_windows_of_many_chat_cards() {
        let mut limiter = UiLimiter::default();
        let start = Instant::now();
        for i in 0..MAX_TRACKED_SLOTS * 2 {
            limiter.allow(&Slot::ChatCard(i.to_string()), start);
        }
        assert_eq!(limiter.windows.len(), MAX_TRACKED_SLOTS);
        assert!(!limiter.allow(&Slot::Panel, start));
        limiter.allow(&Slot::Panel, start + WINDOW);
        assert!(limiter.windows.len() <= MAX_TRACKED_SLOTS);
    }
}
