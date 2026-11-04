//! The host side of `mumcord:plugin@1.1.0` (FR-9.5 … FR-9.9, FR-9.24).
//!
//! 1.1 only adds to 1.0, so the 1.1 interfaces delegate to the 1.0 host implementations in the
//! other `api/*` modules. Bindgen makes separate Rust types per package, so every value crosses
//! with [`Up`] (1.0 to 1.1, always possible) or [`Down`] (1.1 to 1.0, `None` for a case 1.1
//! added, such as the `stage` slot). The conversions are generated from the 1.0 WIT with the
//! `record!` and `variant!` macros below; bindgen's `with` cannot share plain types between
//! packages. The new 1.1 calls answer `not-permitted` until their feature tasks implement them.
//!
//! Events are queued as 1.1 events (the superset): a 1.1 plugin gets them as they are, a 1.0
//! plugin gets them converted back and never sees a 1.1-only event (FR-9.8).

use wasmtime::component::{HasSelf, Linker};

use crate::api::host_calls;
use crate::api::webview::WebviewPlace;
use crate::runtime::PluginState;
use crate::runtime::bindings::mumcord::plugin as v1_0;

pub(crate) mod bindings {
    wasmtime::component::bindgen!({ path: "wit/1.1.0", world: "plugin" });
}

/// The 1.1 WIT types as Rust types.
pub use bindings::mumcord::plugin as wit;
use wit::types::Error;

/// Puts every 1.1 interface into `linker`.
pub(crate) fn add_to_linker(linker: &mut Linker<PluginState>) -> wasmtime::Result<()> {
    bindings::Plugin::add_to_linker::<PluginState, HasSelf<PluginState>>(linker, |s| s)
}

/// A 1.0 value as its 1.1 type. Always possible: a minor only adds (FR-9.7).
pub trait Up {
    type New;
    fn up(self) -> Self::New;
}

/// A 1.1 value as its 1.0 type, or `None` when it uses a case that 1.1 added (FR-9.8).
pub trait Down {
    type Old;
    fn down(self) -> Option<Self::Old>;
}

/// WIT primitives are the same Rust type in both minors.
macro_rules! same {
    ($($ty:ty),*) => {$(
        impl Up for $ty {
            type New = Self;
            fn up(self) -> Self {
                self
            }
        }

        impl Down for $ty {
            type Old = Self;
            fn down(self) -> Option<Self> {
                Some(self)
            }
        }
    )*};
}

same!((), bool, u8, u16, u32, u64, i32, i64, f32, f64, String);

impl<T: Up> Up for Option<T> {
    type New = Option<T::New>;
    fn up(self) -> Self::New {
        self.map(Up::up)
    }
}

impl<T: Down> Down for Option<T> {
    type Old = Option<T::Old>;
    fn down(self) -> Option<Self::Old> {
        match self {
            Some(value) => value.down().map(Some),
            None => Some(None),
        }
    }
}

impl<T: Up> Up for Vec<T> {
    type New = Vec<T::New>;
    fn up(self) -> Self::New {
        self.into_iter().map(Up::up).collect()
    }
}

impl<T: Down> Down for Vec<T> {
    type Old = Vec<T::Old>;
    fn down(self) -> Option<Self::Old> {
        self.into_iter().map(Down::down).collect()
    }
}

impl<T: Up, E: Up> Up for Result<T, E> {
    type New = Result<T::New, E::New>;
    fn up(self) -> Self::New {
        self.map(Up::up).map_err(Up::up)
    }
}

impl<T: Down, E: Down> Down for Result<T, E> {
    type Old = Result<T::Old, E::Old>;
    fn down(self) -> Option<Self::Old> {
        match self {
            Ok(value) => value.down().map(Ok),
            Err(error) => error.down().map(Err),
        }
    }
}

impl<A: Up, B: Up> Up for (A, B) {
    type New = (A::New, B::New);
    fn up(self) -> Self::New {
        (self.0.up(), self.1.up())
    }
}

impl<A: Down, B: Down> Down for (A, B) {
    type Old = (A::Old, B::Old);
    fn down(self) -> Option<Self::Old> {
        Some((self.0.down()?, self.1.down()?))
    }
}

/// A WIT record: the same fields in both minors.
macro_rules! record {
    ($iface:ident :: $ty:ident { $($field:ident),* $(,)? }) => {
        impl Up for v1_0::$iface::$ty {
            type New = wit::$iface::$ty;
            fn up(self) -> Self::New {
                wit::$iface::$ty { $($field: self.$field.up()),* }
            }
        }

        impl Down for wit::$iface::$ty {
            type Old = v1_0::$iface::$ty;
            fn down(self) -> Option<Self::Old> {
                Some(v1_0::$iface::$ty { $($field: self.$field.down()?),* })
            }
        }
    };
}

/// A WIT enum or variant, listed with its 1.0 cases; cases that 1.1 added have no 1.0 value.
macro_rules! variant {
    ($iface:ident :: $ty:ident { $($case:ident $(($v:ident))?),* $(,)? }) => {
        impl Up for v1_0::$iface::$ty {
            type New = wit::$iface::$ty;
            fn up(self) -> Self::New {
                match self {
                    $(Self::$case $(($v))? => wit::$iface::$ty::$case $(($v.up()))?,)*
                }
            }
        }

        impl Down for wit::$iface::$ty {
            type Old = v1_0::$iface::$ty;
            #[allow(unreachable_patterns)]
            fn down(self) -> Option<Self::Old> {
                Some(match self {
                    $(Self::$case $(($v))? => v1_0::$iface::$ty::$case $(($v.down()?))?,)*
                    _ => return None,
                })
            }
        }
    };
}

// Generated from wit/1.0.0: every record, enum and variant of 1.0.
record!(clock::Datetime {
    unix_seconds,
    utc_offset_minutes
});
record!(events::Channel { id, name });
record!(events::SelfState {
    me,
    server,
    channel,
    muted,
    deafened,
    do_not_disturb
});
variant!(events::Visibility {
    Everyone,
    Friends,
    Nobody
});
record!(events::PluginSettings {
    granted,
    visibility
});
variant!(events::PanelVisibility {
    Hidden,
    Inline,
    Expanded
});
variant!(events::ActionValue { None, Boolean(v), Number(v), Text(v), Reorder(v), File(v) });
variant!(events::ActionSource { Slot(v), Command });
record!(events::UiAction {
    source,
    action,
    value
});
record!(events::PeerMessage {
    sender,
    kind,
    payload,
    direct
});
record!(events::ChatTrigger {
    sender,
    trigger,
    args
});
record!(events::ChatMatch {
    sender,
    pattern_index,
    text
});
record!(events::HttpResult { id, outcome });
variant!(events::Event { Talking(v), Joined(v), Left(v), ScreenShareStarted(v), ChatActivity(v), Mentioned(v), SelfChanged(v), ThemeChanged(v), LanguageChanged(v), SettingsChanged(v), UiAction(v), Timer(v), PeerMessage(v), ProfileCardOpened(v), PanelVisibilityChanged(v), UserMenuRequested(v), DialogClosed, HttpResponse(v), WebviewMessage(v), ChatTrigger(v), ChatMatch(v), PeerReady(v) });
variant!(http::Method {
    Get,
    Post,
    Put,
    Patch,
    Delete
});
variant!(http::HeaderValue { Text(v), Secret(v) });
record!(http::Header { name, value });
variant!(http::FormValue { Text(v), File(v) });
record!(http::FormField { name, value });
variant!(http::Body { Empty, Bytes(v), File(v), Form(v) });
record!(http::HttpRequest {
    method,
    url,
    headers,
    body
});
record!(http::HttpResponse {
    status,
    headers,
    body
});
variant!(http::HttpError { HostNotAllowed, InvalidRequest, Redirect, Timeout, Tls(v), Connect(v), ResponseTooLarge });
variant!(log::Level {
    Debug,
    Info,
    Warn,
    Error
});
variant!(peers::Target { Channel, Direct(v) });
variant!(storage::Scope { Identity, Global });
variant!(theme::Mode { Light, Dark });
record!(theme::ThemeInfo {
    mode,
    tokens,
    reduced_motion
});
record!(types::StartInfo {
    api_version,
    previous_version
});
record!(types::BotInfo { name, modules });
record!(types::Peer { session, name, key });
variant!(types::Error {
    NotPermitted,
    TooLarge,
    RateLimited,
    NoRecipients,
    Hidden,
    QuotaExceeded,
    Invalid
});
variant!(ui::Slot { Panel, MemberBadge(v), ProfileCard(v), ChatCard(v), ChatDecoration(v), Settings, Dialog });
variant!(ui::Animation {
    Bob,
    Hop,
    Wave,
    Nap,
    Sparkle,
    Shake,
    Fade
});
variant!(ui::Direction { Row, Column });
variant!(ui::Align {
    Start,
    Center,
    End,
    Stretch
});
variant!(ui::Justify {
    Start,
    End,
    SpaceBetween,
    FillFirst
});
record!(ui::Stack {
    direction,
    gap,
    align,
    justify
});
variant!(ui::TextStyle {
    Body,
    Caption,
    Heading,
    Monospace
});
record!(ui::Text { text, style, color });
record!(ui::Image {
    asset,
    alt,
    width,
    height
});
record!(ui::Sprite {
    asset,
    alt,
    frame_width,
    frame_height,
    frames,
    fps,
    still_frame
});
record!(ui::Meter {
    label,
    value,
    color
});
variant!(ui::Tone {
    Neutral,
    Success,
    Warning,
    Danger,
    Accent
});
variant!(ui::ButtonKind {
    Primary,
    Secondary,
    Danger
});
record!(ui::Button {
    label,
    action,
    kind,
    disabled,
    expanded,
    detail
});
record!(ui::IconButton {
    icon,
    label,
    action,
    disabled
});
record!(ui::CopyButton { label, text, kind });
record!(ui::Chip { text, color, tone });
record!(ui::Progress { label, value });
record!(ui::ListProps {
    label,
    reorder_action
});
record!(ui::Toggle {
    label,
    value,
    action,
    disabled
});
record!(ui::SelectOption { value, label });
record!(ui::Select {
    label,
    options,
    selected,
    action,
    refresh_action,
    disabled
});
record!(ui::Slider {
    label,
    min,
    max,
    step,
    value,
    action,
    disabled
});
variant!(ui::InputPurpose {
    Plain,
    ServerAddress
});
record!(ui::TextInput {
    label,
    value,
    placeholder,
    max_bytes,
    purpose,
    action,
    disabled,
    help,
    help_tone,
    error
});
record!(ui::Password {
    label,
    secret,
    action,
    disabled,
    help,
    help_tone,
    error
});
record!(ui::FilePicker {
    label,
    accept,
    action,
    disabled
});
record!(ui::Countdown { label, until });
variant!(ui::Component { Stack(v), Text(v), Image(v), Sprite(v), Meter(v), Button(v), IconButton(v), Chip(v), Divider, List(v), Progress(v), Toggle(v), Select(v), Slider(v), TextInput(v), Password(v), FilePicker(v), Countdown(v), CopyButton(v) });
record!(ui::Node {
    component,
    animation,
    children
});
record!(ui::UserMenuFilter {
    bot_name,
    bot_module,
    sessions
});
variant!(webview::Place {
    PanelPopover,
    Dialog
});

impl From<wit::webview::Place> for WebviewPlace {
    fn from(place: wit::webview::Place) -> Self {
        match place {
            wit::webview::Place::PanelPopover => Self::PanelPopover,
            wit::webview::Place::Dialog => Self::Dialog,
            wit::webview::Place::SidePanel => Self::SidePanel,
            wit::webview::Place::Stage => Self::Stage,
        }
    }
}

impl From<WebviewPlace> for wit::webview::Place {
    fn from(place: WebviewPlace) -> Self {
        match place {
            WebviewPlace::PanelPopover => Self::PanelPopover,
            WebviewPlace::Dialog => Self::Dialog,
            WebviewPlace::SidePanel => Self::SidePanel,
            WebviewPlace::Stage => Self::Stage,
        }
    }
}

/// Existing features keep producing 1.0 events; they are queued as 1.1 events.
impl From<v1_0::events::Event> for wit::events::Event {
    fn from(event: v1_0::events::Event) -> Self {
        event.up()
    }
}

/// An argument from a 1.1 plugin for a 1.0 host call; `invalid` when it uses a 1.1-only case.
fn down<T: Down>(value: T) -> Result<T::Old, Error> {
    value.down().ok_or(Error::Invalid)
}

fn up<T: Up>(result: Result<T, v1_0::types::Error>) -> Result<T::New, Error> {
    result.up()
}

impl wit::types::Host for PluginState {}
impl wit::events::Host for PluginState {}

impl wit::log::Host for PluginState {
    fn write(&mut self, level: wit::log::Level, message: String) {
        if let Some(level) = level.down() {
            v1_0::log::Host::write(self, level, message);
        }
    }
}

impl wit::storage::Host for PluginState {
    fn get(&mut self, scope: wit::storage::Scope, key: String) -> Result<Option<Vec<u8>>, Error> {
        up(v1_0::storage::Host::get(self, down(scope)?, key))
    }

    fn set(
        &mut self,
        scope: wit::storage::Scope,
        key: String,
        value: Vec<u8>,
    ) -> Result<(), Error> {
        up(v1_0::storage::Host::set(self, down(scope)?, key, value))
    }

    fn delete(&mut self, scope: wit::storage::Scope, key: String) -> Result<(), Error> {
        up(v1_0::storage::Host::delete(self, down(scope)?, key))
    }

    fn list_keys(
        &mut self,
        scope: wit::storage::Scope,
        prefix: String,
    ) -> Result<Vec<String>, Error> {
        up(v1_0::storage::Host::list_keys(self, down(scope)?, prefix))
    }
}

impl wit::timers::Host for PluginState {
    fn start(&mut self, interval_ms: u32, repeat: bool) -> Result<u32, Error> {
        up(v1_0::timers::Host::start(self, interval_ms, repeat))
    }

    fn cancel(&mut self, id: u32) {
        v1_0::timers::Host::cancel(self, id);
    }
}

impl wit::clock::Host for PluginState {
    fn now(&mut self) -> wit::clock::Datetime {
        v1_0::clock::Host::now(self).up()
    }
}

impl wit::notify::Host for PluginState {
    fn send(&mut self, title: String, body: String) -> Result<(), Error> {
        up(v1_0::notify::Host::send(self, title, body))
    }
}

impl wit::theme::Host for PluginState {
    fn current(&mut self) -> wit::theme::ThemeInfo {
        v1_0::theme::Host::current(self).up()
    }
}

impl wit::random::Host for PluginState {
    fn bytes(&mut self, len: u32) -> Result<Vec<u8>, Error> {
        up(v1_0::random::Host::bytes(self, len))
    }
}

impl wit::peers::Host for PluginState {
    fn send(&mut self, to: wit::peers::Target, kind: u8, payload: Vec<u8>) -> Result<(), Error> {
        up(v1_0::peers::Host::send(self, down(to)?, kind, payload))
    }

    fn start_game_burst(&mut self) -> Result<(), Error> {
        up(v1_0::peers::Host::start_game_burst(self))
    }

    fn presence(&mut self) -> Result<Vec<wit::types::Peer>, Error> {
        up(v1_0::peers::Host::presence(self))
    }

    fn bot(&mut self, user: u32) -> Result<Option<wit::types::BotInfo>, Error> {
        up(v1_0::peers::Host::bot(self, user))
    }
}

impl wit::ui::Host for PluginState {
    /// The stage slot is filled through `stage.set-tree`, so it is `invalid` here (ui.wit).
    fn set_tree(&mut self, slot: wit::ui::Slot, nodes: Vec<wit::ui::Node>) -> Result<(), Error> {
        up(v1_0::ui::Host::set_tree(self, down(slot)?, down(nodes)?))
    }

    fn clear(&mut self, slot: wit::ui::Slot) {
        if let Some(slot) = slot.down() {
            v1_0::ui::Host::clear(self, slot);
        }
    }

    fn open_dialog(&mut self, title: String) -> Result<(), Error> {
        up(v1_0::ui::Host::open_dialog(self, title))
    }

    fn open_dialog_with(&mut self, title: String, subtitle: Option<String>) -> Result<(), Error> {
        up(v1_0::ui::Host::open_dialog_with(self, title, subtitle))
    }

    fn close_dialog(&mut self) {
        v1_0::ui::Host::close_dialog(self);
    }

    fn set_user_menu_entry(&mut self, label: Option<String>) -> Result<(), Error> {
        up(v1_0::ui::Host::set_user_menu_entry(self, label))
    }

    fn set_user_menu_entry_for(
        &mut self,
        label: String,
        filter: wit::ui::UserMenuFilter,
    ) -> Result<(), Error> {
        up(v1_0::ui::Host::set_user_menu_entry_for(
            self,
            label,
            down(filter)?,
        ))
    }

    fn open_panel_popover(&mut self) -> Result<(), Error> {
        up(v1_0::ui::Host::open_panel_popover(self))
    }

    fn close_panel_popover(&mut self) {
        v1_0::ui::Host::close_panel_popover(self);
    }

    fn open_permissions(&mut self) -> Result<(), Error> {
        up(v1_0::ui::Host::open_permissions(self))
    }
}

impl wit::chat::Host for PluginState {
    fn send(&mut self, text: String) -> Result<(), Error> {
        up(v1_0::chat::Host::send(self, text))
    }

    fn line_hash(&mut self, text: String) -> u32 {
        v1_0::chat::Host::line_hash(self, text)
    }

    fn supersede(&mut self, card_id: String, sender: u32, hash: u32) -> Result<(), Error> {
        up(v1_0::chat::Host::supersede(self, card_id, sender, hash))
    }

    fn current_message(&mut self) -> Option<u64> {
        v1_0::chat::Host::current_message(self)
    }

    fn is_replay(&mut self) -> bool {
        v1_0::chat::Host::is_replay(self)
    }

    fn replace(&mut self, message: u64, card_id: String) -> Result<(), Error> {
        up(v1_0::chat::Host::replace(self, message, card_id))
    }

    fn hide(&mut self, message: u64) -> Result<(), Error> {
        up(v1_0::chat::Host::hide(self, message))
    }

    fn post_image(&mut self, png: Vec<u8>, alt: String) -> Result<wit::chat::PostOutcome, Error> {
        self.ctx
            .grants
            .check(&self.id, host_calls::POST_IMAGE_PERMISSION)
            .map_err(Up::up)?;
        host_calls::check_post_image(&png, &alt).map_err(Up::up)?;
        let waiter = self
            .io()
            .map_err(Up::up)?
            .chat_post_image(&self.id, png, alt, self.chat.server)
            .map_err(Up::up)?;
        let outcome = self
            .clock
            .waiting(|| waiter.wait(host_calls::POST_WAIT_CAP));
        let result = outcome.unwrap_or(Err(v1_0::types::Error::NoRecipients));
        tracing::info!(plugin = %self.id, ok = result.is_ok(), "plugin chat.post-image");
        match result.map_err(Up::up)? {
            host_calls::PostOutcome::Image => Ok(wit::chat::PostOutcome::Image),
            host_calls::PostOutcome::FileOffer => Ok(wit::chat::PostOutcome::FileOffer),
        }
    }
}

impl wit::secrets::Host for PluginState {
    fn exists(&mut self, name: String) -> Result<bool, Error> {
        up(v1_0::secrets::Host::exists(self, name))
    }

    fn delete(&mut self, name: String) -> Result<(), Error> {
        up(v1_0::secrets::Host::delete(self, name))
    }
}

impl wit::http::Host for PluginState {
    fn request(&mut self, req: wit::http::HttpRequest) -> Result<u32, Error> {
        self.http_request(down(req)?)
            .map_err(|refused| match refused {
                crate::api::http::Refused::Api(error) => error.up(),
                crate::api::http::Refused::OriginNotAllowed => Error::OriginNotAllowed,
            })
    }
}

impl wit::webview::Host for PluginState {
    fn show(&mut self, place: wit::webview::Place, entry: String) -> Result<(), Error> {
        let place = WebviewPlace::from(place);
        check_webview_place(self, place).map_err(Up::up)?;
        up(self
            .io()
            .and_then(|io| io.webview_show(&self.id, place, entry)))
    }

    fn hide(&mut self) {
        v1_0::webview::Host::hide(self);
    }

    fn post(&mut self, message: String) -> Result<(), Error> {
        up(v1_0::webview::Host::post(self, message))
    }

    fn set_size_hint(&mut self, min_width: u32, min_height: u32) -> Result<(), Error> {
        check_webview_place(self, WebviewPlace::SidePanel).map_err(Up::up)?;
        up(self
            .io()
            .and_then(|io| io.webview_set_size_hint(&self.id, min_width, min_height)))
    }
}

/// `ui.webview` plus the permission `place` needs (`ui.side-panel`, `ui.stage`).
fn check_webview_place(state: &PluginState, place: WebviewPlace) -> Result<(), v1_0::types::Error> {
    let grants = &state.ctx.grants;
    grants.check(&state.id, crate::api::webview::PERMISSION)?;
    place
        .extra_permission()
        .map_or(Ok(()), |extra| grants.check(&state.id, extra))
}

impl wit::stage::Host for PluginState {
    fn start(&mut self, title: String, content: wit::stage::StageContent) -> Result<String, Error> {
        let content = match content {
            wit::stage::StageContent::Tree(nodes) => {
                crate::api::io::StageContent::Tree(down(nodes)?)
            }
            wit::stage::StageContent::Web(entry) => crate::api::io::StageContent::Web(entry),
        };
        crate::api::stage::start(self, title, content).map_err(Up::up)
    }

    fn join(&mut self, activity: String) -> Result<(), Error> {
        crate::api::stage::join(self, &activity).map_err(Up::up)
    }

    fn leave(&mut self) {
        crate::api::stage::leave(self);
    }

    fn end(&mut self) {
        crate::api::stage::end(self);
    }

    fn set_tree(&mut self, nodes: Vec<wit::ui::Node>) -> Result<(), Error> {
        crate::api::stage::set_tree(self, down(nodes)?).map_err(Up::up)
    }
}

impl wit::app_menu::Host for PluginState {
    fn set_state(
        &mut self,
        item: String,
        enabled: bool,
        reason: Option<String>,
    ) -> Result<(), Error> {
        crate::api::app_menu::set_state(self, &item, enabled, reason).map_err(Up::up)
    }
}

impl wit::relay::Host for PluginState {
    fn status(&mut self) -> wit::relay::RelayStatus {
        wit::relay::RelayStatus::NotAllowed
    }

    fn send_channel(&mut self, _data: Vec<u8>, _fallback: bool) -> Result<(), Error> {
        Err(Error::NotPermitted)
    }

    fn send_direct(&mut self, _peer: u32, _data: Vec<u8>, _fallback: bool) -> Result<(), Error> {
        Err(Error::NotPermitted)
    }
}

impl wit::audio::Host for PluginState {
    fn play(&mut self, _sound: String, _gain: Option<f32>) -> Result<(), Error> {
        Err(Error::NotPermitted)
    }

    fn stop_all(&mut self) {}
}

impl wit::files::Host for PluginState {
    fn save(&mut self, suggested_name: String, data: Vec<u8>) -> Result<bool, Error> {
        self.ctx
            .grants
            .check(&self.id, host_calls::SAVE_PERMISSION)
            .map_err(Up::up)?;
        let name = host_calls::check_save(&suggested_name, &data).map_err(Up::up)?;
        let size = data.len();
        let waiter = self
            .io()
            .map_err(Up::up)?
            .files_save(&self.id, name, data)
            .map_err(Up::up)?;
        // The user takes their time in the dialog: waiting, outside the call's wall cap.
        let saved = self.wait_for_user(waiter).unwrap_or(false);
        tracing::info!(plugin = %self.id, size, saved, "plugin files.save");
        Ok(saved)
    }

    fn import_picture(&mut self) -> Result<Option<wit::files::Picture>, Error> {
        self.ctx
            .grants
            .check(&self.id, host_calls::IMPORT_PICTURE_PERMISSION)
            .map_err(Up::up)?;
        let waiter = self
            .io()
            .map_err(Up::up)?
            .files_import_picture(&self.id)
            .map_err(Up::up)?;
        let picked = self.wait_for_user(waiter).unwrap_or(Ok(None));
        tracing::info!(
            plugin = %self.id,
            size = picked.as_ref().ok().and_then(Option::as_ref).map(|p| p.data.len()),
            ok = picked.is_ok(),
            "plugin files.import-picture"
        );
        Ok(picked.map_err(Up::up)?.map(|p| wit::files::Picture {
            name: p.name,
            data: p.data,
        }))
    }
}

impl PluginState {
    /// Waits for the user in a host dialog (FR-9.103, FR-9.105). The plugin handles no events
    /// meanwhile; its queue stays bounded and drops the oldest, so only their count is logged.
    fn wait_for_user<T>(&mut self, waiter: host_calls::Waiter<T>) -> Option<T> {
        let dropped = self.diag.snapshot().dropped_events;
        let answer = self
            .clock
            .waiting_on_user(|| waiter.wait(host_calls::DIALOG_WAIT_CAP));
        let dropped = self.diag.snapshot().dropped_events.saturating_sub(dropped);
        if dropped > 0 {
            tracing::info!(plugin = %self.id, dropped, "plugin events dropped while a dialog was open");
        }
        answer
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wit::events::{ActionSource, Event, UiAction};

    fn peer() -> wit::types::Peer {
        wit::types::Peer {
            session: 3,
            name: "Ada".into(),
            key: None,
        }
    }

    #[test]
    fn a_1_0_plugin_never_gets_a_1_1_event() {
        let only_1_1 = [
            Event::StageActivityStarted((peer(), "a".into())),
            Event::StageActivityEnded("a".into()),
            Event::StageJoined(peer()),
            Event::StageLeft(peer()),
            Event::StageVisibilityChanged(true),
            Event::AppMenuActivated("open".into()),
            Event::RelayMessage((peer(), vec![1])),
            Event::RelayStatusChanged(wit::relay::RelayStatus::Available),
            Event::SurfaceClosed(wit::webview::Place::SidePanel),
        ];
        for event in only_1_1 {
            assert!(event.down().is_none());
        }
        let stage_action = Event::UiAction(UiAction {
            source: ActionSource::Slot(wit::ui::Slot::Stage),
            action: "go".into(),
            value: wit::events::ActionValue::None,
        });
        assert!(stage_action.down().is_none());
    }

    #[test]
    fn events_of_1_0_survive_the_round_trip() {
        let talking = v1_0::events::Event::Talking((
            v1_0::types::Peer {
                session: 3,
                name: "Ada".into(),
                key: Some("k".into()),
            },
            true,
        ));
        let back = Event::from(talking).down();
        assert!(matches!(
            back,
            Some(v1_0::events::Event::Talking((p, true))) if p.session == 3 && p.key.as_deref() == Some("k")
        ));
        let action = Event::from(v1_0::events::Event::UiAction(v1_0::events::UiAction {
            source: v1_0::events::ActionSource::Slot(v1_0::ui::Slot::ChatCard("c".into())),
            action: "vote".into(),
            value: v1_0::events::ActionValue::Number(2.0),
        }));
        assert!(matches!(
            action.down(),
            Some(v1_0::events::Event::UiAction(a))
                if matches!(&a.source, v1_0::events::ActionSource::Slot(v1_0::ui::Slot::ChatCard(c)) if c == "c")
        ));
    }

    #[test]
    fn errors_and_places_convert() {
        assert!(matches!(
            v1_0::types::Error::QuotaExceeded.up(),
            Error::QuotaExceeded
        ));
        assert!(Error::NotFound.down().is_none());
        assert!(wit::webview::Place::Stage.down().is_none());
        assert!(matches!(
            wit::webview::Place::Dialog.down(),
            Some(v1_0::webview::Place::Dialog)
        ));
    }
}
