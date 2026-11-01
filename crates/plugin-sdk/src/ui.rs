//! Plugin UI: typed builders for the component tree, plus the raw `ui` interface.
//!
//! The host takes a flat list of nodes where node 0 is the root and children are referenced by
//! index. [`El`] lets plugins write the tree nested instead; [`show`] flattens and sends it.
//!
//! ```no_run
//! use mumcord_plugin_sdk::ui::{self, Animation, ButtonKind, Slot};
//!
//! # fn demo() -> Result<(), mumcord_plugin_sdk::types::Error> {
//! let tree = ui::column()
//!     .child(ui::heading("Dice"))
//!     .child(ui::text("Roll a d6").animate(Animation::Bob))
//!     .child(
//!         ui::row()
//!             .child(ui::button("roll", "Roll"))
//!             .child(ui::button("reset", "Reset").kind(ButtonKind::Secondary).disabled(true)),
//!     );
//! ui::show(Slot::Panel, tree)
//! # }
//! ```
//!
//! Strings such as `action` are the plugin's own identifiers; they come back in the `ui-action`
//! event. The host validates every tree (only stacks and lists may have children, labels are
//! required, colours must be theme tokens) and refuses invalid ones with `invalid`.
//!
//! A user-menu entry can be limited to bots that say so in their Mumcord hello, plus users the
//! plugin lists itself (e.g. saved bots found by `peer.key`); [`crate::peers::bot`] tells a
//! user's bot info:
//!
//! ```no_run
//! use mumcord_plugin_sdk::ui::{self, UserMenuFilter};
//!
//! # fn demo(saved: Vec<u32>) -> Result<(), mumcord_plugin_sdk::types::Error> {
//! let filter = UserMenuFilter {
//!     bot_name: Some("mumbot".into()),
//!     bot_module: Some("music".into()),
//!     sessions: saved,
//! };
//! ui::set_user_menu_entry_for("Configure bot", &filter)
//! # }
//! ```

pub use crate::bindings::mumcord::plugin::ui::*;

/// One element of a UI tree with its children, before flattening.
#[derive(Debug, Clone)]
pub struct El {
    pub component: Component,
    pub animation: Option<Animation>,
    pub children: Vec<El>,
}

impl El {
    pub fn new(component: Component) -> Self {
        Self {
            component,
            animation: None,
            children: Vec::new(),
        }
    }

    /// Appends a child. Only `stack` and `list` may have children.
    #[must_use]
    pub fn child(mut self, child: El) -> Self {
        self.children.push(child);
        self
    }

    /// Appends several children. Only `stack` and `list` may have children.
    #[must_use]
    pub fn children(mut self, children: impl IntoIterator<Item = El>) -> Self {
        self.children.extend(children);
        self
    }

    #[must_use]
    pub fn animate(mut self, animation: Animation) -> Self {
        self.animation = Some(animation);
        self
    }

    /// Disables an interactive component; no effect on others.
    #[must_use]
    pub fn disabled(mut self, disabled: bool) -> Self {
        match &mut self.component {
            Component::Button(c) => c.disabled = disabled,
            Component::IconButton(c) => c.disabled = disabled,
            Component::Toggle(c) => c.disabled = disabled,
            Component::Select(c) => c.disabled = disabled,
            Component::Slider(c) => c.disabled = disabled,
            Component::TextInput(c) => c.disabled = disabled,
            Component::Password(c) => c.disabled = disabled,
            Component::FilePicker(c) => c.disabled = disabled,
            _ => {}
        }
        self
    }

    /// Sets the colour token of a text, meter or chip; no effect on others.
    #[must_use]
    pub fn color(mut self, token: &str) -> Self {
        match &mut self.component {
            Component::Text(c) => c.color = Some(token.to_owned()),
            Component::Meter(c) => c.color = Some(token.to_owned()),
            Component::Chip(c) => c.color = Some(token.to_owned()),
            _ => {}
        }
        self
    }

    /// Sets the kind of a button; no effect on others.
    #[must_use]
    pub fn kind(mut self, kind: ButtonKind) -> Self {
        if let Component::Button(c) = &mut self.component {
            c.kind = kind;
        }
        self
    }

    /// Flattens the tree into the host's node list: node 0 is the root, then depth-first order.
    pub fn into_nodes(self) -> Vec<Node> {
        let mut nodes = Vec::new();
        flatten(self, &mut nodes);
        nodes
    }
}

fn flatten(el: El, nodes: &mut Vec<Node>) -> NodeIndex {
    let index = nodes.len();
    nodes.push(Node {
        component: el.component,
        animation: el.animation,
        children: Vec::new(),
    });
    let children = el.children.into_iter().map(|c| flatten(c, nodes)).collect();
    nodes[index].children = children;
    // The host refuses trees far smaller than u32::MAX nodes, so this never saturates in practice.
    NodeIndex::try_from(index).unwrap_or(NodeIndex::MAX)
}

/// Replaces the tree shown in `slot`.
pub fn show(slot: Slot, tree: El) -> Result<(), Error> {
    set_tree(&slot, &tree.into_nodes())
}

pub fn stack(direction: Direction, gap: u8, align: Align) -> El {
    El::new(Component::Stack(Stack {
        direction,
        gap,
        align,
    }))
}

/// A vertical stack with a small gap.
pub fn column() -> El {
    stack(Direction::Column, 8, Align::Stretch)
}

/// A horizontal stack with a small gap.
pub fn row() -> El {
    stack(Direction::Row, 8, Align::Center)
}

pub fn styled_text(text: &str, style: TextStyle) -> El {
    El::new(Component::Text(Text {
        text: text.to_owned(),
        style,
        color: None,
    }))
}

pub fn text(text: &str) -> El {
    styled_text(text, TextStyle::Body)
}

pub fn heading(text: &str) -> El {
    styled_text(text, TextStyle::Heading)
}

pub fn caption(text: &str) -> El {
    styled_text(text, TextStyle::Caption)
}

pub fn image(asset: &str, alt: &str, width: u16, height: u16) -> El {
    El::new(Component::Image(Image {
        asset: asset.to_owned(),
        alt: alt.to_owned(),
        width,
        height,
    }))
}

pub fn sprite(sprite: Sprite) -> El {
    El::new(Component::Sprite(sprite))
}

/// `value` is between 0 and 1.
pub fn meter(label: &str, value: f32) -> El {
    El::new(Component::Meter(Meter {
        label: label.to_owned(),
        value,
        color: None,
    }))
}

pub fn button(action: &str, label: &str) -> El {
    El::new(Component::Button(Button {
        label: label.to_owned(),
        action: action.to_owned(),
        kind: ButtonKind::Primary,
        disabled: false,
    }))
}

pub fn icon_button(action: &str, icon: &str, label: &str) -> El {
    El::new(Component::IconButton(IconButton {
        icon: icon.to_owned(),
        label: label.to_owned(),
        action: action.to_owned(),
        disabled: false,
    }))
}

pub fn chip(text: &str) -> El {
    El::new(Component::Chip(Chip {
        text: text.to_owned(),
        color: None,
    }))
}

pub fn divider() -> El {
    El::new(Component::Divider)
}

/// `value` is between 0 and 1; `None` shows an indeterminate bar.
pub fn progress(label: &str, value: Option<f32>) -> El {
    El::new(Component::Progress(Progress {
        label: label.to_owned(),
        value,
    }))
}

pub fn list(label: &str) -> El {
    El::new(Component::List(ListProps {
        label: label.to_owned(),
        reorder_action: None,
    }))
}

pub fn toggle(action: &str, label: &str, value: bool) -> El {
    El::new(Component::Toggle(Toggle {
        label: label.to_owned(),
        value,
        action: action.to_owned(),
        disabled: false,
    }))
}

/// `options` are `(value, label)` pairs.
pub fn select(action: &str, label: &str, options: &[(&str, &str)], selected: Option<&str>) -> El {
    El::new(Component::Select(Select {
        label: label.to_owned(),
        options: options
            .iter()
            .map(|(value, label)| SelectOption {
                value: (*value).to_owned(),
                label: (*label).to_owned(),
            })
            .collect(),
        selected: selected.map(str::to_owned),
        action: action.to_owned(),
        refresh_action: None,
        disabled: false,
    }))
}

pub fn slider(action: &str, label: &str, min: f32, max: f32, step: f32, value: f32) -> El {
    El::new(Component::Slider(Slider {
        label: label.to_owned(),
        min,
        max,
        step,
        value,
        action: action.to_owned(),
        disabled: false,
    }))
}

pub fn text_input(action: &str, label: &str, value: &str, max_bytes: u8) -> El {
    El::new(Component::TextInput(TextInput {
        label: label.to_owned(),
        value: value.to_owned(),
        placeholder: String::new(),
        max_bytes,
        purpose: InputPurpose::Plain,
        action: action.to_owned(),
        disabled: false,
    }))
}

/// A secret field bound to the `secrets` entry named `secret`.
pub fn password(action: &str, label: &str, secret: &str) -> El {
    El::new(Component::Password(Password {
        label: label.to_owned(),
        secret: secret.to_owned(),
        action: action.to_owned(),
        disabled: false,
    }))
}

pub fn file_picker(action: &str, label: &str, accept: &[&str]) -> El {
    El::new(Component::FilePicker(FilePicker {
        label: label.to_owned(),
        accept: accept.iter().map(|a| (*a).to_owned()).collect(),
        action: action.to_owned(),
        disabled: false,
    }))
}

/// `until` is in Unix seconds.
pub fn countdown(label: &str, until: u64) -> El {
    El::new(Component::Countdown(Countdown {
        label: label.to_owned(),
        until,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn label(node: &Node) -> &str {
        match &node.component {
            Component::Text(t) => &t.text,
            Component::Button(b) => &b.label,
            Component::Stack(_) => "stack",
            Component::Divider => "divider",
            _ => "other",
        }
    }

    #[test]
    fn single_element_is_root_without_children() {
        let nodes = text("hi").into_nodes();
        assert_eq!(nodes.len(), 1);
        assert!(nodes[0].children.is_empty());
    }

    #[test]
    fn nested_tree_flattens_depth_first_with_root_at_zero() {
        let tree = column()
            .child(heading("a"))
            .child(row().child(button("x", "b")).child(divider()))
            .child(text("c").animate(Animation::Bob));
        let nodes = tree.into_nodes();
        let labels: Vec<&str> = nodes.iter().map(label).collect();
        assert_eq!(labels, ["stack", "a", "stack", "b", "divider", "c"]);
        assert_eq!(nodes[0].children, [1, 2, 5]);
        assert_eq!(nodes[2].children, [3, 4]);
        assert!(nodes[1].children.is_empty());
        assert_eq!(nodes[5].animation, Some(Animation::Bob));
        assert_eq!(nodes[1].animation, None);
    }

    #[test]
    fn every_child_index_points_forward_and_is_used_once() {
        let tree = list("l").children((0..20).map(|i| row().child(text(&i.to_string()))));
        let nodes = tree.into_nodes();
        let mut seen = vec![false; nodes.len()];
        for (i, node) in nodes.iter().enumerate() {
            for &c in &node.children {
                let c = c as usize;
                assert!(c > i && c < nodes.len());
                assert!(!seen[c]);
                seen[c] = true;
            }
        }
        assert!(seen[1..].iter().all(|s| *s));
    }

    #[test]
    fn modifiers_change_only_matching_components() {
        let b = button("a", "A").kind(ButtonKind::Danger).disabled(true);
        let Component::Button(b) = b.component else {
            panic!("not a button")
        };
        assert_eq!(b.kind, ButtonKind::Danger);
        assert!(b.disabled);

        let t = text("t")
            .color("accent")
            .disabled(true)
            .kind(ButtonKind::Danger);
        let Component::Text(t) = t.component else {
            panic!("not a text")
        };
        assert_eq!(t.color.as_deref(), Some("accent"));
    }

    #[test]
    fn select_maps_option_pairs() {
        let s = select(
            "pick",
            "Pick",
            &[("a", "Apple"), ("b", "Banana")],
            Some("b"),
        );
        let Component::Select(s) = s.component else {
            panic!("not a select")
        };
        assert_eq!(s.options.len(), 2);
        assert_eq!(s.options[1].value, "b");
        assert_eq!(s.options[1].label, "Banana");
        assert_eq!(s.selected.as_deref(), Some("b"));
    }
}
