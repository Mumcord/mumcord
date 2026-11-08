//! The host side of `mumcord:plugin@1.2.0` (FR-13.1, FR-13.2).
//!
//! 1.2 has exactly the interfaces of 1.1 under a new package version, so its host code is the
//! one of 1.1 compiled against the 1.2 bindings (see `minor_host.rs`). The queued events are
//! 1.1 events (see [`crate::api::v1_1`]); [`From`] converts them for a 1.2 plugin.

use crate::api::v1_1::wit as v1_1;

pub(crate) mod bindings {
    wasmtime::component::bindgen!({ path: "wit/1.2.0", world: "plugin" });
}

/// The 1.2 WIT types as Rust types.
pub use bindings::mumcord::plugin as wit;

// Compiled once per minor on purpose: each needs its own bindgen types.
#[allow(clippy::duplicate_mod)]
#[path = "minor_host.rs"]
mod host;
pub use host::add_to_linker;

/// A 1.1 value as its 1.2 type: the same shape under another package.
trait Same {
    type To;
    fn same(self) -> Self::To;
}

macro_rules! primitives {
    ($($ty:ty),*) => {$(
        impl Same for $ty {
            type To = Self;
            fn same(self) -> Self {
                self
            }
        }
    )*};
}

primitives!(bool, u8, u16, u32, u64, f32, String);

impl<T: Same> Same for Option<T> {
    type To = Option<T::To>;
    fn same(self) -> Self::To {
        self.map(Same::same)
    }
}

impl<T: Same> Same for Vec<T> {
    type To = Vec<T::To>;
    fn same(self) -> Self::To {
        self.into_iter().map(Same::same).collect()
    }
}

impl<T: Same, E: Same> Same for Result<T, E> {
    type To = Result<T::To, E::To>;
    fn same(self) -> Self::To {
        self.map(Same::same).map_err(Same::same)
    }
}

impl<A: Same, B: Same> Same for (A, B) {
    type To = (A::To, B::To);
    fn same(self) -> Self::To {
        (self.0.same(), self.1.same())
    }
}

macro_rules! record {
    ($iface:ident :: $ty:ident { $($field:ident),* $(,)? }) => {
        impl Same for v1_1::$iface::$ty {
            type To = wit::$iface::$ty;
            fn same(self) -> Self::To {
                wit::$iface::$ty { $($field: self.$field.same()),* }
            }
        }
    };
}

macro_rules! variant {
    ($iface:ident :: $ty:ident { $($case:ident $(($v:ident))?),* $(,)? }) => {
        impl Same for v1_1::$iface::$ty {
            type To = wit::$iface::$ty;
            fn same(self) -> Self::To {
                match self {
                    $(Self::$case $(($v))? => wit::$iface::$ty::$case $(($v.same()))?,)*
                }
            }
        }
    };
}

// The types the events of `wit/1.1.0/events.wit` are made of.
record!(types::Peer { session, name, key });
variant!(theme::Mode { Light, Dark });
record!(theme::ThemeInfo {
    mode,
    tokens,
    reduced_motion
});
variant!(ui::Slot { Panel, MemberBadge(v), ProfileCard(v), ChatCard(v), ChatDecoration(v), Settings, Dialog, Stage });
variant!(webview::Place {
    PanelPopover,
    Dialog,
    SidePanel,
    Stage
});
variant!(relay::RelayStatus {
    Available,
    NotAllowed,
    NoRelay,
    SwitchedOff
});
record!(http::HttpResponse {
    status,
    headers,
    body
});
variant!(http::HttpError { HostNotAllowed, InvalidRequest, Redirect, Timeout, Tls(v), Connect(v), ResponseTooLarge });
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
variant!(events::Event {
    Talking(v), Joined(v), Left(v), ScreenShareStarted(v), ChatActivity(v), Mentioned(v),
    SelfChanged(v), ThemeChanged(v), LanguageChanged(v), SettingsChanged(v), UiAction(v),
    Timer(v), PeerMessage(v), ProfileCardOpened(v), PanelVisibilityChanged(v),
    UserMenuRequested(v), DialogClosed, HttpResponse(v), WebviewMessage(v), ChatTrigger(v),
    ChatMatch(v), PeerReady(v), StageActivityStarted(v), StageActivityEnded(v), StageJoined(v),
    StageLeft(v), StageVisibilityChanged(v), AppMenuActivated(v), RelayMessage(v),
    RelayStatusChanged(v), SurfaceClosed(v)
});

impl From<v1_1::events::Event> for wit::events::Event {
    fn from(event: v1_1::events::Event) -> Self {
        event.same()
    }
}
