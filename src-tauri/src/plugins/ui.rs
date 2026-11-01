//! Plugin UI trees for the UI (`plugin://ui`, the contract of the tree renderer) and the way
//! back: which component a `ui-action` belongs to.
//!
//! The types mirror the WIT `ui` interface one to one. Everything in a tree is untrusted text
//! from the plugin; the renderer validates and sanitises it.

use std::collections::HashMap;

use mumcord_plugins::api::webview::WebviewPlace;
use mumcord_plugins::service::UiUpdate;
use mumcord_plugins::wit::events::PanelVisibility as WitPanelVisibility;
use mumcord_plugins::wit::ui as wit;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;

/// Where a plugin tree is shown.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PluginSlot {
    /// 260×160 in the Plugins strip, 420×560 expanded.
    Panel,
    /// Next to a user's name.
    MemberBadge { session: u32 },
    /// A section on a user's profile card.
    ProfileCard { session: u32 },
    /// A local card in the chat of the current channel.
    ChatCard { id: String },
    /// A decoration next to a user's name in chat.
    ChatDecoration { session: u32 },
    /// The plugin's form in Settings → Plugins.
    Settings,
    /// The plugin's own dialog while it is open.
    Dialog,
}

impl From<wit::Slot> for PluginSlot {
    fn from(slot: wit::Slot) -> Self {
        match slot {
            wit::Slot::Panel => Self::Panel,
            wit::Slot::MemberBadge(session) => Self::MemberBadge { session },
            wit::Slot::ProfileCard(session) => Self::ProfileCard { session },
            wit::Slot::ChatCard(id) => Self::ChatCard { id },
            wit::Slot::ChatDecoration(session) => Self::ChatDecoration { session },
            wit::Slot::Settings => Self::Settings,
            wit::Slot::Dialog => Self::Dialog,
        }
    }
}

impl From<PluginSlot> for wit::Slot {
    fn from(slot: PluginSlot) -> Self {
        match slot {
            PluginSlot::Panel => Self::Panel,
            PluginSlot::MemberBadge { session } => Self::MemberBadge(session),
            PluginSlot::ProfileCard { session } => Self::ProfileCard(session),
            PluginSlot::ChatCard { id } => Self::ChatCard(id),
            PluginSlot::ChatDecoration { session } => Self::ChatDecoration(session),
            PluginSlot::Settings => Self::Settings,
            PluginSlot::Dialog => Self::Dialog,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum PluginAnimation {
    Bob,
    Hop,
    Wave,
    Nap,
    Sparkle,
    Shake,
    Fade,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum PluginDirection {
    Row,
    Column,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum PluginAlign {
    Start,
    Center,
    End,
    Stretch,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum PluginTextStyle {
    Body,
    Caption,
    Heading,
    Monospace,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum PluginButtonKind {
    Primary,
    Secondary,
    Danger,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum PluginInputPurpose {
    Plain,
    /// Hosts typed here become the only ones the plugin may reach with `net.http`.
    ServerAddress,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginSelectOption {
    pub value: String,
    pub label: String,
}

/// One UI component. `color` values are theme token names (unknown ones make a tree invalid).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PluginComponent {
    Stack {
        direction: PluginDirection,
        gap: u8,
        align: PluginAlign,
    },
    Text {
        text: String,
        style: PluginTextStyle,
        color: Option<String>,
    },
    /// `asset` is a path inside the package's `assets/`.
    Image {
        asset: String,
        alt: String,
        width: u16,
        height: u16,
    },
    Sprite {
        asset: String,
        alt: String,
        frame_width: u16,
        frame_height: u16,
        frames: u8,
        fps: u8,
        still_frame: u8,
    },
    Meter {
        label: String,
        value: f32,
        color: Option<String>,
    },
    Button {
        label: String,
        action: String,
        kind: PluginButtonKind,
        disabled: bool,
    },
    IconButton {
        icon: String,
        label: String,
        action: String,
        disabled: bool,
    },
    Chip {
        text: String,
        color: Option<String>,
    },
    Divider,
    /// `value` none: indeterminate.
    Progress {
        label: String,
        value: Option<f32>,
    },
    List {
        label: String,
        reorder_action: Option<String>,
    },
    Toggle {
        label: String,
        value: bool,
        action: String,
        disabled: bool,
    },
    Select {
        label: String,
        options: Vec<PluginSelectOption>,
        selected: Option<String>,
        action: String,
        refresh_action: Option<String>,
        disabled: bool,
    },
    Slider {
        label: String,
        min: f32,
        max: f32,
        step: f32,
        value: f32,
        action: String,
        disabled: bool,
    },
    TextInput {
        label: String,
        value: String,
        placeholder: String,
        max_bytes: u8,
        purpose: PluginInputPurpose,
        action: String,
        disabled: bool,
    },
    /// What the user types goes into the named secret; the plugin never sees it.
    Password {
        label: String,
        secret: String,
        action: String,
        disabled: bool,
    },
    FilePicker {
        label: String,
        accept: Vec<String>,
        action: String,
        disabled: bool,
    },
    /// `until` in Unix seconds.
    Countdown {
        label: String,
        until: f64,
    },
}

/// One node of a flat tree: node 0 is the root, `children` are indices into the same list.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginNode {
    pub component: PluginComponent,
    pub animation: Option<PluginAnimation>,
    pub children: Vec<u32>,
}

/// What changed in a plugin's UI.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PluginUiChange {
    /// Replaces the slot's tree (a new chat card id appends the card).
    SetTree {
        slot: PluginSlot,
        nodes: Vec<PluginNode>,
    },
    /// Removes the slot's tree.
    Clear { slot: PluginSlot },
    /// Opens the plugin's dialog, or only changes its title when it is open.
    OpenDialog { title: String },
    /// Closes the dialog without telling the plugin.
    CloseDialog,
    /// The plugin's entry in the user context menu; `None` removes it.
    UserMenuEntry { label: Option<String> },
    /// The plugin's sealed web view opens (or switches page) at `url`, a `mumcord-web` URL the
    /// app serves from the plugin's `web/` folder (FR-6.46).
    WebviewShow {
        place: PluginWebviewPlace,
        url: String,
    },
    /// The web view closes; its place shows the plugin's tree again.
    WebviewHide,
    /// A bridge message for the page (untrusted text). Not part of the snapshot.
    WebviewPost { message: String },
}

/// Where a plugin's web view is shown.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum PluginWebviewPlace {
    PanelPopover,
    Dialog,
}

impl From<WebviewPlace> for PluginWebviewPlace {
    fn from(place: WebviewPlace) -> Self {
        match place {
            WebviewPlace::PanelPopover => Self::PanelPopover,
            WebviewPlace::Dialog => Self::Dialog,
        }
    }
}

/// `plugin://ui`: a plugin changed its UI.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type, Event)]
#[serde(rename_all = "camelCase")]
#[tauri_specta(event_name = "plugin://ui")]
pub struct PluginUi {
    pub plugin: String,
    pub change: PluginUiChange,
}

/// How the user saw the plugin's panel.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum PluginPanelVisibility {
    Hidden,
    Inline,
    Expanded,
}

impl From<PluginPanelVisibility> for WitPanelVisibility {
    fn from(v: PluginPanelVisibility) -> Self {
        match v {
            PluginPanelVisibility::Hidden => Self::Hidden,
            PluginPanelVisibility::Inline => Self::Inline,
            PluginPanelVisibility::Expanded => Self::Expanded,
        }
    }
}

fn animation(a: wit::Animation) -> PluginAnimation {
    match a {
        wit::Animation::Bob => PluginAnimation::Bob,
        wit::Animation::Hop => PluginAnimation::Hop,
        wit::Animation::Wave => PluginAnimation::Wave,
        wit::Animation::Nap => PluginAnimation::Nap,
        wit::Animation::Sparkle => PluginAnimation::Sparkle,
        wit::Animation::Shake => PluginAnimation::Shake,
        wit::Animation::Fade => PluginAnimation::Fade,
    }
}

fn component(c: wit::Component) -> PluginComponent {
    use PluginComponent as P;
    match c {
        wit::Component::Stack(s) => P::Stack {
            direction: match s.direction {
                wit::Direction::Row => PluginDirection::Row,
                wit::Direction::Column => PluginDirection::Column,
            },
            gap: s.gap,
            align: match s.align {
                wit::Align::Start => PluginAlign::Start,
                wit::Align::Center => PluginAlign::Center,
                wit::Align::End => PluginAlign::End,
                wit::Align::Stretch => PluginAlign::Stretch,
            },
        },
        wit::Component::Text(t) => P::Text {
            text: t.text,
            style: match t.style {
                wit::TextStyle::Body => PluginTextStyle::Body,
                wit::TextStyle::Caption => PluginTextStyle::Caption,
                wit::TextStyle::Heading => PluginTextStyle::Heading,
                wit::TextStyle::Monospace => PluginTextStyle::Monospace,
            },
            color: t.color,
        },
        wit::Component::Image(i) => P::Image {
            asset: i.asset,
            alt: i.alt,
            width: i.width,
            height: i.height,
        },
        wit::Component::Sprite(s) => P::Sprite {
            asset: s.asset,
            alt: s.alt,
            frame_width: s.frame_width,
            frame_height: s.frame_height,
            frames: s.frames,
            fps: s.fps,
            still_frame: s.still_frame,
        },
        wit::Component::Meter(m) => P::Meter {
            label: m.label,
            value: m.value,
            color: m.color,
        },
        wit::Component::Button(b) => P::Button {
            label: b.label,
            action: b.action,
            kind: match b.kind {
                wit::ButtonKind::Primary => PluginButtonKind::Primary,
                wit::ButtonKind::Secondary => PluginButtonKind::Secondary,
                wit::ButtonKind::Danger => PluginButtonKind::Danger,
            },
            disabled: b.disabled,
        },
        wit::Component::IconButton(b) => P::IconButton {
            icon: b.icon,
            label: b.label,
            action: b.action,
            disabled: b.disabled,
        },
        wit::Component::Chip(c) => P::Chip {
            text: c.text,
            color: c.color,
        },
        wit::Component::Divider => P::Divider,
        wit::Component::Progress(p) => P::Progress {
            label: p.label,
            value: p.value,
        },
        wit::Component::List(l) => P::List {
            label: l.label,
            reorder_action: l.reorder_action,
        },
        wit::Component::Toggle(t) => P::Toggle {
            label: t.label,
            value: t.value,
            action: t.action,
            disabled: t.disabled,
        },
        wit::Component::Select(s) => P::Select {
            label: s.label,
            options: s
                .options
                .into_iter()
                .map(|o| PluginSelectOption {
                    value: o.value,
                    label: o.label,
                })
                .collect(),
            selected: s.selected,
            action: s.action,
            refresh_action: s.refresh_action,
            disabled: s.disabled,
        },
        wit::Component::Slider(s) => P::Slider {
            label: s.label,
            min: s.min,
            max: s.max,
            step: s.step,
            value: s.value,
            action: s.action,
            disabled: s.disabled,
        },
        wit::Component::TextInput(t) => P::TextInput {
            label: t.label,
            value: t.value,
            placeholder: t.placeholder,
            max_bytes: t.max_bytes,
            purpose: match t.purpose {
                wit::InputPurpose::Plain => PluginInputPurpose::Plain,
                wit::InputPurpose::ServerAddress => PluginInputPurpose::ServerAddress,
            },
            action: t.action,
            disabled: t.disabled,
        },
        wit::Component::Password(p) => P::Password {
            label: p.label,
            secret: p.secret,
            action: p.action,
            disabled: p.disabled,
        },
        wit::Component::FilePicker(f) => P::FilePicker {
            label: f.label,
            accept: f.accept,
            action: f.action,
            disabled: f.disabled,
        },
        // Unix seconds stay exact in a JavaScript number for millions of years.
        wit::Component::Countdown(c) => P::Countdown {
            label: c.label,
            until: c.until as f64,
        },
    }
}

impl From<wit::Node> for PluginNode {
    fn from(node: wit::Node) -> Self {
        Self {
            component: component(node.component),
            animation: node.animation.map(animation),
            children: node.children,
        }
    }
}

impl PluginUiChange {
    /// The change for `plugin`'s UI update (web view pages get their full URL).
    pub fn new(plugin: &str, update: UiUpdate) -> Self {
        match update {
            UiUpdate::WebviewShow { place, entry } => Self::WebviewShow {
                place: place.into(),
                url: super::webview::url(plugin, &entry),
            },
            other => other.into(),
        }
    }
}

impl From<UiUpdate> for PluginUiChange {
    fn from(update: UiUpdate) -> Self {
        match update {
            UiUpdate::SetTree { slot, nodes } => Self::SetTree {
                slot: slot.into(),
                nodes: nodes.into_iter().map(Into::into).collect(),
            },
            UiUpdate::Clear { slot } => Self::Clear { slot: slot.into() },
            UiUpdate::OpenDialog { title } => Self::OpenDialog { title },
            UiUpdate::CloseDialog => Self::CloseDialog,
            UiUpdate::UserMenuEntry { label } => Self::UserMenuEntry { label },
            UiUpdate::WebviewShow { place, entry } => Self::WebviewShow {
                place: place.into(),
                url: entry,
            },
            UiUpdate::WebviewHide => Self::WebviewHide,
            UiUpdate::WebviewPost { message } => Self::WebviewPost { message },
        }
    }
}

/// What an action needs from the app before the plugin gets it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionKind {
    /// Passed on as it is.
    Plain,
    /// A `server-address` text input: the typed host becomes reachable for `net.http`.
    ServerAddress,
    /// A password field: the typed text goes into this secret, the plugin gets no value.
    Password { secret: String },
    /// A file picker: the picked path becomes a file handle.
    File,
}

/// The current UI of every plugin, so a window that mounts late gets it and every
/// `ui-action` can be checked against what is on screen.
#[derive(Debug, Default)]
pub struct Trees {
    trees: HashMap<String, HashMap<PluginSlot, Vec<PluginNode>>>,
    dialogs: HashMap<String, String>,
    menu_entries: HashMap<String, String>,
    webviews: HashMap<String, (PluginWebviewPlace, String)>,
}

impl Trees {
    pub fn apply(&mut self, plugin: &str, change: &PluginUiChange) {
        match change {
            PluginUiChange::SetTree { slot, nodes } => {
                self.trees
                    .entry(plugin.to_owned())
                    .or_default()
                    .insert(slot.clone(), nodes.clone());
            }
            PluginUiChange::Clear { slot } => {
                if let Some(slots) = self.trees.get_mut(plugin) {
                    slots.remove(slot);
                }
            }
            PluginUiChange::OpenDialog { title } => {
                self.dialogs.insert(plugin.to_owned(), title.clone());
            }
            PluginUiChange::CloseDialog => {
                self.dialogs.remove(plugin);
                if self
                    .webviews
                    .get(plugin)
                    .is_some_and(|(place, _)| *place == PluginWebviewPlace::Dialog)
                {
                    self.webviews.remove(plugin);
                }
                if let Some(slots) = self.trees.get_mut(plugin) {
                    slots.remove(&PluginSlot::Dialog);
                }
            }
            PluginUiChange::UserMenuEntry { label } => match label {
                Some(label) => {
                    self.menu_entries.insert(plugin.to_owned(), label.clone());
                }
                None => {
                    self.menu_entries.remove(plugin);
                }
            },
            PluginUiChange::WebviewShow { place, url } => {
                self.webviews
                    .insert(plugin.to_owned(), (*place, url.clone()));
            }
            PluginUiChange::WebviewHide => {
                self.webviews.remove(plugin);
            }
            PluginUiChange::WebviewPost { .. } => {}
        }
    }

    /// Plugins that are no longer running lose their UI.
    pub fn retain(&mut self, running: impl Fn(&str) -> bool) {
        self.trees.retain(|id, _| running(id));
        self.dialogs.retain(|id, _| running(id));
        self.menu_entries.retain(|id, _| running(id));
        self.webviews.retain(|id, _| running(id));
    }

    /// Everything on screen, as the changes that rebuild it.
    pub fn snapshot(&self) -> Vec<PluginUi> {
        let mut out = Vec::new();
        for (plugin, slots) in &self.trees {
            for (slot, nodes) in slots {
                out.push(PluginUi {
                    plugin: plugin.clone(),
                    change: PluginUiChange::SetTree {
                        slot: slot.clone(),
                        nodes: nodes.clone(),
                    },
                });
            }
        }
        for (plugin, title) in &self.dialogs {
            out.push(PluginUi {
                plugin: plugin.clone(),
                change: PluginUiChange::OpenDialog {
                    title: title.clone(),
                },
            });
        }
        for (plugin, label) in &self.menu_entries {
            out.push(PluginUi {
                plugin: plugin.clone(),
                change: PluginUiChange::UserMenuEntry {
                    label: Some(label.clone()),
                },
            });
        }
        // After the dialogs, so a web view in a dialog finds it open.
        for (plugin, (place, url)) in &self.webviews {
            out.push(PluginUi {
                plugin: plugin.clone(),
                change: PluginUiChange::WebviewShow {
                    place: *place,
                    url: url.clone(),
                },
            });
        }
        out
    }

    /// The kind of the enabled component that sends `action` in the slot's current tree;
    /// `None` when no such component is on screen.
    pub fn action_kind(&self, plugin: &str, slot: &PluginSlot, action: &str) -> Option<ActionKind> {
        let nodes = self.trees.get(plugin)?.get(slot)?;
        nodes
            .iter()
            .find_map(|node| kind_of(&node.component, action))
    }
}

fn kind_of(component: &PluginComponent, wanted: &str) -> Option<ActionKind> {
    use PluginComponent as P;
    let (action, disabled, kind) = match component {
        P::Button {
            action, disabled, ..
        }
        | P::IconButton {
            action, disabled, ..
        }
        | P::Toggle {
            action, disabled, ..
        }
        | P::Slider {
            action, disabled, ..
        } => (action.as_str(), *disabled, ActionKind::Plain),
        P::Select {
            action,
            refresh_action,
            disabled,
            ..
        } => {
            let hit = action == wanted || refresh_action.as_deref() == Some(wanted);
            return (hit && !disabled).then_some(ActionKind::Plain);
        }
        P::List {
            reorder_action: Some(action),
            ..
        } => (action.as_str(), false, ActionKind::Plain),
        P::TextInput {
            action,
            disabled,
            purpose,
            ..
        } => {
            let kind = match purpose {
                PluginInputPurpose::Plain => ActionKind::Plain,
                PluginInputPurpose::ServerAddress => ActionKind::ServerAddress,
            };
            (action.as_str(), *disabled, kind)
        }
        P::Password {
            action,
            secret,
            disabled,
            ..
        } => (
            action.as_str(),
            *disabled,
            ActionKind::Password {
                secret: secret.clone(),
            },
        ),
        P::FilePicker {
            action, disabled, ..
        } => (action.as_str(), *disabled, ActionKind::File),
        _ => return None,
    };
    (action == wanted && !disabled).then_some(kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(component: PluginComponent) -> PluginNode {
        PluginNode {
            component,
            animation: None,
            children: vec![],
        }
    }

    fn set(trees: &mut Trees, slot: PluginSlot, nodes: Vec<PluginNode>) {
        trees.apply("poll", &PluginUiChange::SetTree { slot, nodes });
    }

    #[test]
    fn wit_trees_become_ui_trees() {
        let wit_node = wit::Node {
            component: wit::Component::Button(wit::Button {
                label: "Vote".into(),
                action: "vote:1".into(),
                kind: wit::ButtonKind::Primary,
                disabled: false,
            }),
            animation: Some(wit::Animation::Hop),
            children: vec![],
        };
        let change: PluginUiChange = UiUpdate::SetTree {
            slot: wit::Slot::ChatCard("poll-1".into()),
            nodes: vec![wit_node],
        }
        .into();
        assert_eq!(
            change,
            PluginUiChange::SetTree {
                slot: PluginSlot::ChatCard {
                    id: "poll-1".into()
                },
                nodes: vec![PluginNode {
                    component: PluginComponent::Button {
                        label: "Vote".into(),
                        action: "vote:1".into(),
                        kind: PluginButtonKind::Primary,
                        disabled: false,
                    },
                    animation: Some(PluginAnimation::Hop),
                    children: vec![],
                }],
            }
        );
        let json = serde_json::to_value(&change).expect("serialises");
        assert_eq!(json["type"], "setTree");
        assert_eq!(json["slot"]["kind"], "chatCard");
        assert_eq!(json["nodes"][0]["component"]["type"], "button");
    }

    #[test]
    fn actions_must_be_on_screen_and_enabled() {
        let mut trees = Trees::default();
        set(
            &mut trees,
            PluginSlot::Settings,
            vec![
                node(PluginComponent::TextInput {
                    label: "Bot".into(),
                    value: String::new(),
                    placeholder: String::new(),
                    max_bytes: 64,
                    purpose: PluginInputPurpose::ServerAddress,
                    action: "address".into(),
                    disabled: false,
                }),
                node(PluginComponent::Password {
                    label: "Key".into(),
                    secret: "api-key".into(),
                    action: "save-key".into(),
                    disabled: false,
                }),
                node(PluginComponent::Button {
                    label: "Off".into(),
                    action: "off".into(),
                    kind: PluginButtonKind::Secondary,
                    disabled: true,
                }),
            ],
        );
        let kind = |action| trees.action_kind("poll", &PluginSlot::Settings, action);
        assert_eq!(kind("address"), Some(ActionKind::ServerAddress));
        assert_eq!(
            kind("save-key"),
            Some(ActionKind::Password {
                secret: "api-key".into()
            })
        );
        assert_eq!(kind("off"), None, "disabled");
        assert_eq!(kind("other"), None, "not on screen");
        assert_eq!(
            trees.action_kind("poll", &PluginSlot::Panel, "address"),
            None,
            "another slot"
        );
    }

    #[test]
    fn closing_the_dialog_and_forgetting_remove_the_ui() {
        let mut trees = Trees::default();
        let button = node(PluginComponent::Button {
            label: "Go".into(),
            action: "go".into(),
            kind: PluginButtonKind::Primary,
            disabled: false,
        });
        set(&mut trees, PluginSlot::Dialog, vec![button.clone()]);
        set(&mut trees, PluginSlot::Panel, vec![button]);
        trees.apply(
            "poll",
            &PluginUiChange::OpenDialog {
                title: "Poll".into(),
            },
        );
        assert_eq!(trees.snapshot().len(), 3);
        trees.apply("poll", &PluginUiChange::CloseDialog);
        assert_eq!(trees.snapshot().len(), 1);
        assert!(
            trees
                .action_kind("poll", &PluginSlot::Dialog, "go")
                .is_none()
        );
        trees.retain(|id| id != "poll");
        assert!(trees.snapshot().is_empty());
    }

    #[test]
    fn web_views_are_in_the_snapshot_and_close_with_their_dialog() {
        let mut trees = Trees::default();
        let show = |place| PluginUiChange::WebviewShow {
            place,
            url: "mumcord-web://localhost/snacks/index.html".into(),
        };
        trees.apply(
            "snacks",
            &PluginUiChange::OpenDialog {
                title: "Snacks".into(),
            },
        );
        trees.apply("snacks", &show(PluginWebviewPlace::Dialog));
        trees.apply(
            "snacks",
            &PluginUiChange::WebviewPost {
                message: "hi".into(),
            },
        );
        let snapshot = trees.snapshot();
        assert_eq!(snapshot.len(), 2);
        assert_eq!(snapshot[1].change, show(PluginWebviewPlace::Dialog));
        trees.apply("snacks", &PluginUiChange::CloseDialog);
        assert!(trees.snapshot().is_empty());
        trees.apply("snacks", &show(PluginWebviewPlace::PanelPopover));
        trees.apply("snacks", &PluginUiChange::WebviewHide);
        assert!(trees.snapshot().is_empty());
        trees.apply("snacks", &show(PluginWebviewPlace::PanelPopover));
        trees.retain(|_| false);
        assert!(trees.snapshot().is_empty());
    }
}
