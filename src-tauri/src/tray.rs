//! Tray icon showing connection and talk state, with Show / Mute / Deafen / Overlay / Quit
//! (FR-1.37, FR-1.45). On Linux it is our own StatusNotifierItem (see `linux`), with the Tauri
//! tray as fallback when no StatusNotifierWatcher runs.

use std::sync::Mutex;

use mumcord_engine::{ConnState, Engine, EngineEvent, UserView};
use serde::Deserialize;
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

use crate::window;

#[cfg(target_os = "linux")]
mod linux;

const SHOW: &str = "show";
const MUTE: &str = "mute";
const DEAFEN: &str = "deafen";
const QUIT: &str = "quit";

/// Side length of the generated tray images; trays render at 16–32 px.
const ICON_SIZE: u32 = 64;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Conn {
    #[default]
    Disconnected,
    Connecting,
    Connected,
}

/// Everything the tray shows, derived from engine events.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct TrayState {
    conn: Conn,
    self_session: Option<u32>,
    talking: bool,
    muted: bool,
    deafened: bool,
}

/// The state shown by the icon's status dot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Look {
    Disconnected,
    Connecting,
    Idle,
    Talking,
    Muted,
}

impl TrayState {
    /// Applies an engine event; returns whether anything visible may have changed.
    fn apply(&mut self, event: &EngineEvent) -> bool {
        let before = *self;
        match event {
            EngineEvent::StateChanged(state) => {
                self.conn = match state {
                    ConnState::Disconnected { .. } => Conn::Disconnected,
                    ConnState::Connected { .. } => Conn::Connected,
                    _ => Conn::Connecting,
                };
                if self.conn != Conn::Connected {
                    *self = Self {
                        conn: self.conn,
                        ..Self::default()
                    };
                }
            }
            EngineEvent::ServerSynced(snapshot) => {
                self.self_session = Some(snapshot.self_session);
                if let Some(me) = snapshot
                    .users
                    .iter()
                    .find(|u| u.session == snapshot.self_session)
                {
                    self.take_voice(me);
                }
            }
            EngineEvent::UserUpserted(user) if Some(user.session) == self.self_session => {
                self.take_voice(user);
            }
            EngineEvent::TalkStateChanged { session, talking }
                if Some(*session) == self.self_session =>
            {
                self.talking = *talking;
            }
            _ => {}
        }
        *self != before
    }

    fn take_voice(&mut self, me: &UserView) {
        self.muted = me.self_mute;
        self.deafened = me.self_deaf;
    }

    fn look(&self) -> Look {
        match self.conn {
            Conn::Disconnected => Look::Disconnected,
            Conn::Connecting => Look::Connecting,
            Conn::Connected if self.muted || self.deafened => Look::Muted,
            Conn::Connected if self.talking => Look::Talking,
            Conn::Connected => Look::Idle,
        }
    }
}

/// Tray texts, read from the UI's `tray` locale files so every language lives in one place (the
/// tray itself sits outside the web UI and its i18n).
#[derive(Deserialize)]
struct Labels {
    show: String,
    mute: String,
    deafen: String,
    quit: String,
    disconnected: String,
    connecting: String,
    connected: String,
    talking: String,
    muted: String,
}

const LOCALES: [(&str, &str); 24] = [
    ("bg", include_str!("../../ui/src/locales/bg/tray.json")),
    ("cs", include_str!("../../ui/src/locales/cs/tray.json")),
    ("da", include_str!("../../ui/src/locales/da/tray.json")),
    ("de", include_str!("../../ui/src/locales/de/tray.json")),
    ("el", include_str!("../../ui/src/locales/el/tray.json")),
    ("en", include_str!("../../ui/src/locales/en/tray.json")),
    ("es", include_str!("../../ui/src/locales/es/tray.json")),
    ("et", include_str!("../../ui/src/locales/et/tray.json")),
    ("fi", include_str!("../../ui/src/locales/fi/tray.json")),
    ("fr", include_str!("../../ui/src/locales/fr/tray.json")),
    ("ga", include_str!("../../ui/src/locales/ga/tray.json")),
    ("hr", include_str!("../../ui/src/locales/hr/tray.json")),
    ("hu", include_str!("../../ui/src/locales/hu/tray.json")),
    ("it", include_str!("../../ui/src/locales/it/tray.json")),
    ("lt", include_str!("../../ui/src/locales/lt/tray.json")),
    ("lv", include_str!("../../ui/src/locales/lv/tray.json")),
    ("mt", include_str!("../../ui/src/locales/mt/tray.json")),
    ("nl", include_str!("../../ui/src/locales/nl/tray.json")),
    ("pl", include_str!("../../ui/src/locales/pl/tray.json")),
    ("pt", include_str!("../../ui/src/locales/pt/tray.json")),
    ("ro", include_str!("../../ui/src/locales/ro/tray.json")),
    ("sk", include_str!("../../ui/src/locales/sk/tray.json")),
    ("sl", include_str!("../../ui/src/locales/sl/tray.json")),
    ("sv", include_str!("../../ui/src/locales/sv/tray.json")),
];

fn labels_for(code: &str) -> Labels {
    let find = |wanted: &str| LOCALES.iter().find(|(lang, _)| *lang == wanted);
    let (_, json) = find(code)
        .or_else(|| find("en"))
        .expect("English is bundled");
    serde_json::from_str(json).expect("tray locale files are valid (checked by tests)")
}

/// Picks the tray language: the app setting, else the system locale, else English.
fn labels(setting: Option<&str>, system: Option<&str>) -> Labels {
    let lang = setting.or(system).unwrap_or("en");
    let code = lang.get(..2).map(str::to_ascii_lowercase);
    labels_for(code.as_deref().unwrap_or("en"))
}

impl Labels {
    fn status(&self, look: Look) -> &str {
        match look {
            Look::Disconnected => &self.disconnected,
            Look::Connecting => &self.connecting,
            Look::Idle => &self.connected,
            Look::Talking => &self.talking,
            Look::Muted => &self.muted,
        }
    }
}

/// Tooltip text shown when hovering the tray icon.
fn tooltip(status: &str) -> String {
    format!("Mumcord – {status}")
}

/// What a tray menu item does when clicked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Show,
    Mute,
    Deafen,
    Quit,
}

impl Action {
    fn from_id(id: &str) -> Option<Self> {
        match id {
            SHOW => Some(Self::Show),
            MUTE => Some(Self::Mute),
            DEAFEN => Some(Self::Deafen),
            QUIT => Some(Self::Quit),
            _ => None,
        }
    }
}

/// One row of the tray menu as the StatusNotifierItem draws it (the Tauri tray keeps handles to
/// its fixed items instead).
#[cfg(target_os = "linux")]
#[derive(Clone, Debug, PartialEq, Eq)]
enum Entry {
    /// The greyed-out status line at the top.
    Status(String),
    Separator,
    Button(Action, String),
    Toggle {
        action: Action,
        label: String,
        enabled: bool,
        checked: bool,
    },
}

/// The tray menu for `state`: status, Show, Mute, Deafen, Quit.
#[cfg(target_os = "linux")]
fn menu_model(state: TrayState, labels: &Labels) -> Vec<Entry> {
    let connected = state.conn == Conn::Connected;
    vec![
        Entry::Status(labels.status(state.look()).to_owned()),
        Entry::Separator,
        Entry::Button(Action::Show, labels.show.clone()),
        Entry::Toggle {
            action: Action::Mute,
            label: labels.mute.clone(),
            enabled: connected,
            checked: state.muted,
        },
        Entry::Toggle {
            action: Action::Deafen,
            label: labels.deafen.clone(),
            enabled: connected,
            checked: state.deafened,
        },
        Entry::Separator,
        Entry::Button(Action::Quit, labels.quit.clone()),
    ]
}

/// The tray's live state, kept in Tauri state.
pub struct Tray {
    backend: Backend,
    base: Option<Image<'static>>,
    state: Mutex<TrayState>,
}

enum Backend {
    Tauri(Box<TauriTray>),
    /// Our own StatusNotifierItem: Tauri's Linux tray cannot show a tooltip (FR-2.52).
    #[cfg(target_os = "linux")]
    Ksni(ksni::Handle<linux::KsniTray>),
}

/// Handles of the Tauri tray icon and its menu items.
struct TauriTray {
    icon: TrayIcon,
    status: MenuItem<Wry>,
    show: MenuItem<Wry>,
    mute: CheckMenuItem<Wry>,
    deafen: CheckMenuItem<Wry>,
    quit: MenuItem<Wry>,
}

fn base_icon(app: &AppHandle) -> Option<Image<'static>> {
    app.default_window_icon().map(|i| scale(i, ICON_SIZE))
}

/// Creates the tray. Failing to create it is logged; the app works without one.
#[cfg(target_os = "linux")]
pub fn init(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        match linux::spawn(&app).await {
            Ok(handle) => {
                app.manage(Tray {
                    backend: Backend::Ksni(handle),
                    base: base_icon(&app),
                    state: Mutex::new(TrayState::default()),
                });
                refresh(&app, true);
            }
            Err(e) => {
                tracing::warn!(error = %e, "no StatusNotifierItem tray available; falling back to the AppIndicator tray");
                let main = app.clone();
                if let Err(e) = app.run_on_main_thread(move || init_tauri(&main)) {
                    tracing::warn!(error = %e, "creating the tray icon failed");
                }
            }
        }
    });
}

#[cfg(not(target_os = "linux"))]
pub fn init(app: &AppHandle) {
    init_tauri(app);
}

fn init_tauri(app: &AppHandle) {
    if !indicator_library_present() {
        tracing::warn!(
            "no tray library (libayatana-appindicator3 or libappindicator3) found; running without a tray icon"
        );
        return;
    }
    match build(app) {
        Ok(tray) => {
            app.manage(tray);
            refresh(app, true);
        }
        Err(e) => tracing::warn!(error = %e, "creating the tray icon failed"),
    }
}

fn build(app: &AppHandle) -> tauri::Result<Tray> {
    let status = MenuItem::with_id(app, "status", "", false, None::<&str>)?;
    let show = MenuItem::with_id(app, SHOW, "", true, None::<&str>)?;
    let mute = CheckMenuItem::with_id(app, MUTE, "", false, false, None::<&str>)?;
    let deafen = CheckMenuItem::with_id(app, DEAFEN, "", false, false, None::<&str>)?;
    let quit = MenuItem::with_id(app, QUIT, "", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let separator2 = PredefinedMenuItem::separator(app)?;
    let items: [&dyn IsMenuItem<Wry>; 7] = [
        &status,
        &separator,
        &show,
        &mute,
        &deafen,
        &separator2,
        &quit,
    ];
    let menu = Menu::with_items(app, &items)?;
    let base = base_icon(app);
    let mut builder = TrayIconBuilder::with_id("main")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(on_menu_event)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                window::show(tray.app_handle());
            }
        });
    if let Some(base) = &base {
        builder = builder.icon(with_dot(base, Look::Disconnected));
    }
    Ok(Tray {
        backend: Backend::Tauri(Box::new(TauriTray {
            icon: builder.build(app)?,
            status,
            show,
            mute,
            deafen,
            quit,
        })),
        base,
        state: Mutex::new(TrayState::default()),
    })
}

fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    if let Some(action) = Action::from_id(event.id().as_ref()) {
        perform(app, action);
    }
    // A check item toggles itself on click; the engine's echo decides the real state.
    refresh(app, false);
}

/// Runs a clicked menu action.
fn perform(app: &AppHandle, action: Action) {
    let Some(state) = current_state(app) else {
        return;
    };
    let engine = app.state::<Engine>();
    let result = match action {
        Action::Show => {
            window::show(app);
            Ok(())
        }
        Action::Mute => engine.set_self_mute(!state.muted),
        Action::Deafen => engine.set_self_deaf(!state.deafened),
        Action::Quit => {
            window::quit(app);
            Ok(())
        }
    };
    if let Err(e) = result {
        tracing::warn!(error = %e, "tray action failed");
    }
}

fn current_state(app: &AppHandle) -> Option<TrayState> {
    let tray = app.try_state::<Tray>()?;
    Some(*tray.state.lock().unwrap_or_else(|e| e.into_inner()))
}

/// The tray texts in the current app language.
fn current_labels(app: &AppHandle) -> Labels {
    let setting = app
        .state::<Engine>()
        .settings()
        .ok()
        .and_then(|s| s.language);
    let system = std::env::var("LANG").ok();
    labels(setting.as_deref(), system.as_deref())
}

/// Updates the tray from an engine event (called for every forwarded event).
pub fn on_engine_event(app: &AppHandle, event: &EngineEvent) {
    let Some(tray) = app.try_state::<Tray>() else {
        return;
    };
    let changed = tray
        .state
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .apply(event);
    if changed {
        refresh(app, matches!(event, EngineEvent::StateChanged(_)));
    }
}

/// The app language changed, so the menu texts follow it.
pub fn language_changed(app: &AppHandle) {
    refresh(app, true);
}

/// Redraws icon, tooltip and menu; `texts` also reloads the language-dependent labels.
fn refresh(app: &AppHandle, texts: bool) {
    let Some(tray) = app.try_state::<Tray>() else {
        return;
    };
    match &tray.backend {
        Backend::Tauri(handles) => refresh_tauri(app, &tray, handles, texts),
        #[cfg(target_os = "linux")]
        Backend::Ksni(handle) => {
            let handle = handle.clone();
            let app = app.clone();
            // The update reads the newest state when it runs, so out-of-order tasks are harmless.
            tauri::async_runtime::spawn(async move {
                handle.update(|t| t.sync(&app)).await;
            });
        }
    }
}

fn refresh_tauri(app: &AppHandle, tray: &Tray, handles: &TauriTray, texts: bool) {
    let state = *tray.state.lock().unwrap_or_else(|e| e.into_inner());
    let labels = current_labels(app);
    let look = state.look();
    let connected = state.conn == Conn::Connected;
    let mut result = handles
        .status
        .set_text(labels.status(look))
        .and_then(|()| handles.icon.set_tooltip(Some(tooltip(labels.status(look)))))
        .and_then(|()| handles.mute.set_enabled(connected))
        .and_then(|()| handles.mute.set_checked(state.muted))
        .and_then(|()| handles.deafen.set_enabled(connected))
        .and_then(|()| handles.deafen.set_checked(state.deafened));
    if texts {
        result = result
            .and_then(|()| handles.show.set_text(&labels.show))
            .and_then(|()| handles.mute.set_text(&labels.mute))
            .and_then(|()| handles.deafen.set_text(&labels.deafen))
            .and_then(|()| handles.quit.set_text(&labels.quit));
    }
    if let Some(base) = &tray.base {
        result = result.and_then(|()| handles.icon.set_icon(Some(with_dot(base, look))));
    }
    if let Err(e) = result {
        tracing::warn!(error = %e, "updating the tray icon failed");
    }
}

/// The Linux tray loads the AppIndicator library at runtime (libappindicator-sys) and panics
/// when it is missing, which would crash the app at startup. Probing the same library names up
/// front lets the app run without a tray instead.
#[cfg(target_os = "linux")]
fn indicator_library_present() -> bool {
    any_library_loadable(&[c"libayatana-appindicator3.so.1", c"libappindicator3.so.1"])
}

#[cfg(not(target_os = "linux"))]
fn indicator_library_present() -> bool {
    true
}

/// Whether one of `names` can be loaded with the dynamic loader's normal search path.
#[cfg(target_os = "linux")]
fn any_library_loadable(names: &[&std::ffi::CStr]) -> bool {
    names.iter().any(|name| {
        // SAFETY: `name` is a valid C string. A library found here stays loaded (no dlclose):
        // the tray loads the very same library right after, so its initialisers run only once.
        let handle = unsafe { libc::dlopen(name.as_ptr(), libc::RTLD_LAZY | libc::RTLD_LOCAL) };
        !handle.is_null()
    })
}

/// Whether a tray icon is actually on screen to bring a hidden window back.
pub async fn is_visible(app: &AppHandle) -> bool {
    if app.try_state::<Tray>().is_none() {
        return false;
    }
    host_present().await
}

/// On Linux the tray icon is a StatusNotifierItem; without a watcher (e.g. GNOME without
/// the AppIndicator extension) it is invisible even though creating it succeeded.
#[cfg(target_os = "linux")]
async fn host_present() -> bool {
    use ashpd::zbus;
    let check = async {
        let conn = zbus::Connection::session().await?;
        let proxy = zbus::fdo::DBusProxy::new(&conn).await?;
        let name = zbus::names::BusName::try_from("org.kde.StatusNotifierWatcher")?;
        Ok::<bool, zbus::Error>(proxy.name_has_owner(name).await?)
    };
    check.await.unwrap_or_else(|e| {
        tracing::warn!(error = %e, "checking for a system tray failed");
        false
    })
}

#[cfg(not(target_os = "linux"))]
async fn host_present() -> bool {
    true
}

/// Box-filters the app icon down to `size` × `size` RGBA.
fn scale(icon: &Image<'_>, size: u32) -> Image<'static> {
    let (w, h) = (icon.width().max(1), icon.height().max(1));
    let src = icon.rgba();
    let mut out = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let (x0, x1) = (x * w / size, ((x + 1) * w / size).max(x * w / size + 1));
            let (y0, y1) = (y * h / size, ((y + 1) * h / size).max(y * h / size + 1));
            let mut sum = [0u32; 4];
            for sy in y0..y1.min(h) {
                for sx in x0..x1.min(w) {
                    let i = ((sy * w + sx) * 4) as usize;
                    for (c, s) in sum.iter_mut().enumerate() {
                        *s += u32::from(src.get(i + c).copied().unwrap_or(0));
                    }
                }
            }
            let n = ((x1.min(w) - x0) * (y1.min(h) - y0)).max(1);
            let o = ((y * size + x) * 4) as usize;
            for c in 0..4 {
                out[o + c] = u8::try_from(sum[c] / n).unwrap_or(u8::MAX);
            }
        }
    }
    Image::new_owned(out, size, size)
}

fn dot_colour(look: Look) -> [u8; 3] {
    match look {
        Look::Disconnected => [128, 132, 142],
        Look::Connecting => [240, 178, 50],
        Look::Idle => [88, 101, 242],
        Look::Talking => [35, 165, 90],
        Look::Muted => [218, 55, 60],
    }
}

/// Draws a status dot with a dark ring in the bottom-right corner of the icon.
fn with_dot(base: &Image<'_>, look: Look) -> Image<'static> {
    let (w, h) = (base.width(), base.height());
    let mut rgba = base.rgba().to_vec();
    let size = w.min(h) as f32;
    let radius = size * 0.22;
    let ring = size * 0.06;
    let (cx, cy) = (w as f32 - radius - ring, h as f32 - radius - ring);
    let colour = dot_colour(look);
    for y in 0..h {
        for x in 0..w {
            let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            let px = if d <= radius {
                [colour[0], colour[1], colour[2], 255]
            } else if d <= radius + ring {
                [20, 20, 24, 255]
            } else {
                continue;
            };
            let i = ((y * w + x) * 4) as usize;
            rgba[i..i + 4].copy_from_slice(&px);
        }
    }
    Image::new_owned(rgba, w, h)
}

#[cfg(test)]
mod tests {
    use mumcord_engine::{ServerInfoView, Snapshot};

    use super::*;

    fn connected(state: &mut TrayState) {
        state.apply(&EngineEvent::StateChanged(ConnState::Connected {
            server_id: 1,
        }));
    }

    #[test]
    fn talk_events_of_other_people_are_ignored() {
        let mut state = TrayState::default();
        connected(&mut state);
        state.self_session = Some(7);
        assert!(!state.apply(&EngineEvent::TalkStateChanged {
            session: 8,
            talking: true
        }));
        assert!(state.apply(&EngineEvent::TalkStateChanged {
            session: 7,
            talking: true
        }));
        assert_eq!(state.look(), Look::Talking);
    }

    #[test]
    fn mute_wins_over_talking_and_disconnect_resets() {
        let mut state = TrayState {
            conn: Conn::Connected,
            self_session: Some(7),
            talking: true,
            muted: true,
            deafened: false,
        };
        assert_eq!(state.look(), Look::Muted);
        state.apply(&EngineEvent::StateChanged(ConnState::Disconnected {
            error: None,
        }));
        assert_eq!(state, TrayState::default());
        assert_eq!(state.look(), Look::Disconnected);
    }

    #[test]
    fn synced_snapshot_sets_own_session_and_voice_state() {
        let mut state = TrayState::default();
        connected(&mut state);
        let snapshot = Snapshot {
            server_id: 1,
            self_session: 3,
            info: ServerInfoView {
                version: "1.5".into(),
                release: None,
                os: None,
                follow_supported: true,
                welcome_text: None,
                default_channel_id: None,
                max_message_length: 5000,
                max_image_message_length: 0,
                allow_html: true,
                max_bandwidth: None,
                max_users: None,
                recording_allowed: false,
                server_label: None,
                server_address: None,
                server_port: None,
                tls_fingerprint: None,
                users_online: 0,
                session_id: None,
                registered: false,
                current_channel_permissions: None,
                mumcord_users: 0,
                tls_trusted: true,
                tls_issuer: None,
                tls_valid_from: None,
                tls_valid_until: None,
                tls_certificate_pem: None,
            },
            channels: vec![],
            users: vec![UserView {
                session: 3,
                channel_id: 0,
                name: "me".into(),
                cert_hash: None,
                registered: false,
                self_mute: true,
                self_deaf: true,
                server_mute: false,
                server_deaf: false,
                suppressed: false,
                listening_channel_ids: vec![],
                has_avatar: false,
                avatar_hash: None,
                has_comment: false,
                volume: 100,
                local_mute: false,
                mumcord_features: None,
            }],
            followed_channel_ids: vec![],
            unread: vec![],
            read_only: false,
            rejoin_channel_id: None,
        };
        state.apply(&EngineEvent::ServerSynced(Box::new(snapshot)));
        assert_eq!(state.self_session, Some(3));
        assert!(state.deafened && state.muted);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn library_probe_finds_only_loadable_libraries() {
        assert!(!any_library_loadable(&[c"libmumcord-does-not-exist.so.1"]));
        assert!(any_library_loadable(&[
            c"libmumcord-does-not-exist.so.1",
            c"libc.so.6"
        ]));
    }

    #[test]
    fn every_language_has_every_tray_text() {
        for (code, json) in LOCALES {
            let parsed: Labels = serde_json::from_str(json)
                .unwrap_or_else(|e| panic!("tray texts for {code} are incomplete: {e}"));
            assert!(
                !parsed.quit.is_empty() && !parsed.talking.is_empty(),
                "{code}"
            );
        }
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ui/src/locales");
        let folders = std::fs::read_dir(dir)
            .expect("locales folder exists")
            .count();
        assert_eq!(
            folders,
            LOCALES.len(),
            "a language folder is missing from LOCALES"
        );
    }

    #[test]
    fn tray_language_follows_setting_then_system() {
        assert_eq!(labels(Some("de"), Some("es_ES.UTF-8")).quit, "Beenden");
        assert_eq!(labels(None, Some("es_ES.UTF-8")).quit, "Salir");
        assert_eq!(labels(None, Some("fr_FR.UTF-8")).quit, "Quitter");
        assert_eq!(labels(None, Some("xx_XX")).quit, "Quit");
        assert_eq!(labels(None, None).quit, "Quit");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn menu_follows_connection_and_voice_state() {
        let labels = labels(Some("en"), None);
        let offline = menu_model(TrayState::default(), &labels);
        assert_eq!(offline.len(), 7);
        assert_eq!(offline[0], Entry::Status(labels.disconnected.clone()));
        assert_eq!(offline[2], Entry::Button(Action::Show, labels.show.clone()));
        assert_eq!(offline[6], Entry::Button(Action::Quit, labels.quit.clone()));
        assert!(matches!(
            offline[3],
            Entry::Toggle {
                action: Action::Mute,
                enabled: false,
                checked: false,
                ..
            }
        ));
        let muted = TrayState {
            conn: Conn::Connected,
            self_session: Some(1),
            talking: false,
            muted: true,
            deafened: false,
        };
        let online = menu_model(muted, &labels);
        assert_eq!(online[0], Entry::Status(labels.muted.clone()));
        assert!(matches!(
            online[3],
            Entry::Toggle {
                action: Action::Mute,
                enabled: true,
                checked: true,
                ..
            }
        ));
        assert!(matches!(
            online[4],
            Entry::Toggle {
                action: Action::Deafen,
                enabled: true,
                checked: false,
                ..
            }
        ));
    }

    #[test]
    fn menu_ids_map_to_actions() {
        assert_eq!(Action::from_id(MUTE), Some(Action::Mute));
        assert_eq!(Action::from_id(QUIT), Some(Action::Quit));
        assert_eq!(Action::from_id("status"), None);
    }

    #[test]
    fn tooltip_names_app_and_status() {
        assert_eq!(tooltip("Connected"), "Mumcord – Connected");
    }

    #[test]
    fn dot_is_drawn_in_the_corner() {
        let base = Image::new_owned(vec![0; 64 * 64 * 4], 64, 64);
        let img = with_dot(&base, Look::Talking);
        let at = |x: u32, y: u32| {
            let i = ((y * 64 + x) * 4) as usize;
            img.rgba()[i..i + 4].to_vec()
        };
        assert_eq!(at(0, 0), vec![0, 0, 0, 0]);
        assert_eq!(at(46, 46), vec![35, 165, 90, 255]);
    }

    #[test]
    fn scaling_averages_pixels() {
        let base = Image::new_owned(vec![200; 128 * 128 * 4], 128, 128);
        let img = scale(&base, 64);
        assert_eq!((img.width(), img.height()), (64, 64));
        assert!(img.rgba().iter().all(|&b| b == 200));
    }
}
