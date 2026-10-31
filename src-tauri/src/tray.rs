//! Tray icon and menu (FR-1.37, FR-1.45, FR-3.132): the state of every server, Mute, Deafen,
//! Do Not Disturb, voice server and channel, recent chats, Open, Settings and Quit; the icon
//! shows talking, muted, unread and mentions. On Linux it is our own StatusNotifierItem (see
//! `linux`), with the Tauri tray as fallback when no StatusNotifierWatcher runs. Without any
//! tray (GNOME without the AppIndicator extension) there is no menu and closing the window
//! follows the close setting.

use std::sync::Mutex;

use mumcord_engine::{Engine, EngineEvent};
use serde::Deserialize;
use tauri::image::Image;
use tauri::menu::{
    CheckMenuItem, IconMenuItem, IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu,
};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Manager, Wry};
use tauri_specta::Event;

use crate::commands::dnd::DndState;
use crate::commands::notifications::NotificationActivated;
use crate::commands::tray::{TrayAttention, TraySettingsRequested};
use crate::commands::types::{ServerId, to_engine_server_id};
use crate::window;
use icon::compose;
use menu::{Action, Entry, menu_model};
use state::{Change, Dnd, Look, TrayState};

mod icon;
#[cfg(target_os = "linux")]
mod linux;
mod menu;
mod state;

/// Side length of the generated tray images; trays render at 16–32 px.
const ICON_SIZE: u32 = 64;
/// Side length of the dots in front of menu rows.
const DOT_SIZE: u32 = 16;

/// Tray texts, read from the UI's `tray` locale files so every language lives in one place (the
/// tray itself sits outside the web UI and its i18n). `{name}` placeholders are filled in.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Labels {
    show: String,
    settings: String,
    mute: String,
    deafen: String,
    dnd: String,
    dnd_system: String,
    quit: String,
    disconnected: String,
    connecting: String,
    reconnecting: String,
    connected: String,
    connected_in: String,
    server_state: String,
    talking: String,
    muted: String,
    voice_menu: String,
    voice_server: String,
    channels_of: String,
    more_channels: String,
    recent_chats: String,
    chat_unread: String,
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
    /// The tooltip's status for the icon's look.
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

/// Everything a backend draws, taken from the state under one lock.
struct View {
    icon: Option<Image<'static>>,
    tooltip: String,
    /// Only built when the menu may have changed.
    menu: Option<Vec<Entry>>,
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

impl Backend {
    /// The Tauri tray; the StatusNotifierItem runs its menu actions itself.
    fn tauri(&self) -> Option<&TauriTray> {
        match self {
            Self::Tauri(handles) => Some(handles),
            #[cfg(target_os = "linux")]
            Self::Ksni(_) => None,
        }
    }
}

/// The Tauri tray icon and the menu it shows. The menu is rebuilt only when its entries change;
/// the replaced menu is dropped with its items.
struct TauriTray {
    icon: TrayIcon,
    menu: Mutex<Shown>,
}

#[derive(Default)]
struct Shown {
    entries: Vec<Entry>,
    /// The action of each clickable item; item ids are indices into it.
    actions: Vec<Action>,
}

fn base_icon(app: &AppHandle) -> Option<Image<'static>> {
    app.default_window_icon().map(|i| icon::scale(i, ICON_SIZE))
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
                    state: Mutex::new(initial_state(&app)),
                });
                refresh(&app, Change::Menu);
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
            refresh(app, Change::Menu);
        }
        Err(e) => tracing::warn!(error = %e, "creating the tray icon failed"),
    }
}

fn build(app: &AppHandle) -> tauri::Result<Tray> {
    let base = base_icon(app);
    let mut builder = TrayIconBuilder::with_id("main")
        .menu(&Menu::new(app)?)
        // A menu bar item opens its menu on click (macOS); elsewhere a click shows the window
        // and the menu is on right click.
        .show_menu_on_left_click(cfg!(target_os = "macos"))
        .on_menu_event(on_menu_event);
    #[cfg(not(target_os = "macos"))]
    {
        use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
        builder = builder.on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                window::show(tray.app_handle());
            }
        });
    }
    if let Some(base) = &base {
        builder = builder.icon(compose(base, TrayState::default().icon()));
    }
    Ok(Tray {
        backend: Backend::Tauri(Box::new(TauriTray {
            icon: builder.build(app)?,
            menu: Mutex::new(Shown::default()),
        })),
        base,
        state: Mutex::new(initial_state(app)),
    })
}

/// A new tray starts disconnected, with the current Do Not Disturb state.
fn initial_state(app: &AppHandle) -> TrayState {
    let dnd = match app.state::<Engine>().settings() {
        Ok(settings) => Dnd::from(&crate::dnd::state_from(&settings, crate::dnd::system(app))),
        Err(e) => {
            tracing::warn!(error = %e, "reading the Do Not Disturb setting for the tray failed");
            Dnd::default()
        }
    };
    TrayState {
        dnd,
        ..TrayState::default()
    }
}

impl From<&DndState> for Dnd {
    fn from(state: &DndState) -> Self {
        Self {
            manual: state.manual,
            system: state.follow_system && state.system.on == Some(true),
        }
    }
}

fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    let action = app.try_state::<Tray>().and_then(|tray| {
        let handles = tray.backend.tauri()?;
        let index: usize = event.id().as_ref().parse().ok()?;
        let shown = handles.menu.lock().unwrap_or_else(|e| e.into_inner());
        shown.actions.get(index).cloned()
    });
    if let Some(action) = action {
        perform(app, action);
    }
    // A check item toggles itself on click; the engine's echo decides the real state.
    refresh(app, Change::Menu);
}

/// Runs a clicked menu action.
fn perform(app: &AppHandle, action: Action) {
    let Some(state) = app
        .try_state::<Tray>()
        .map(|tray| lock(&tray.state).clone())
    else {
        return;
    };
    let engine = app.state::<Engine>();
    let target = state.voice_target();
    let result = match action {
        Action::Show => {
            window::show(app);
            Ok(())
        }
        Action::Mute => engine.set_self_mute(!target.is_some_and(|s| s.muted)),
        Action::Deafen => engine.set_self_deaf(!target.is_some_and(|s| s.deafened)),
        Action::Dnd => {
            let (app, on) = (app.clone(), !state.dnd.manual);
            tauri::async_runtime::spawn(async move {
                if let Err(e) = crate::dnd::set(&app, |s| s.do_not_disturb = on).await {
                    tracing::warn!(error = %e, "switching Do Not Disturb from the tray failed");
                }
            });
            Ok(())
        }
        Action::VoiceServer(server_id) => match state
            .connected()
            .find(|s| s.id == server_id)
            .and_then(|s| s.own_channel())
        {
            Some(channel_id) => engine.join_channel(to_engine_server_id(server_id), channel_id),
            None => Ok(()),
        },
        Action::Join {
            server_id,
            channel_id,
        } => engine.join_channel(to_engine_server_id(server_id), channel_id),
        Action::OpenChat {
            server_id,
            conversation,
        } => {
            window::show(app);
            if let Err(e) = (NotificationActivated {
                server_id,
                conversation,
            })
            .emit(app)
            {
                tracing::warn!(error = %e, "telling the UI to open a chat from the tray failed");
            }
            Ok(())
        }
        Action::Settings => {
            window::show(app);
            if let Err(e) = (TraySettingsRequested {}).emit(app) {
                tracing::warn!(error = %e, "telling the UI to open the settings failed");
            }
            Ok(())
        }
        Action::Quit => {
            window::quit(app);
            Ok(())
        }
    };
    if let Err(e) = result {
        tracing::warn!(error = %e, "tray action failed");
    }
}

fn lock(state: &Mutex<TrayState>) -> std::sync::MutexGuard<'_, TrayState> {
    state.lock().unwrap_or_else(|e| e.into_inner())
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

/// Updates the tray from an engine event of `server_id` (called for every forwarded event).
pub fn on_engine_event(app: &AppHandle, server_id: ServerId, event: &EngineEvent) {
    let Some(tray) = app.try_state::<Tray>() else {
        return;
    };
    let name = || {
        app.state::<Engine>()
            .server_name(to_engine_server_id(server_id))
            .unwrap_or_else(|e| {
                tracing::warn!(error = %e, "reading the server name for the tray failed");
                None
            })
            .unwrap_or_default()
    };
    let change = lock(&tray.state).apply(server_id, event, name);
    refresh(app, change);
}

/// The UI reported new unread counts, mentions or recent chats.
pub fn attention_changed(app: &AppHandle, attention: TrayAttention) {
    if let Some(tray) = app.try_state::<Tray>() {
        let mut state = lock(&tray.state);
        if state.attention == attention {
            return;
        }
        state.attention = attention;
    }
    refresh(app, Change::Menu);
}

/// Do Not Disturb was switched, or the system's state changed.
pub fn dnd_changed(app: &AppHandle, state: &DndState) {
    let dnd = Dnd::from(state);
    if let Some(tray) = app.try_state::<Tray>() {
        let mut state = lock(&tray.state);
        if state.dnd == dnd {
            return;
        }
        state.dnd = dnd;
    }
    refresh(app, Change::Menu);
}

/// The app language changed, so the menu texts follow it.
pub fn language_changed(app: &AppHandle) {
    refresh(app, Change::Menu);
}

fn view(app: &AppHandle, tray: &Tray, change: Change) -> View {
    let labels = current_labels(app);
    let state = lock(&tray.state);
    View {
        icon: tray.base.as_ref().map(|base| compose(base, state.icon())),
        tooltip: tooltip(labels.status(state.look())),
        menu: (change == Change::Menu).then(|| menu_model(&state, &labels)),
    }
}

/// Redraws what `change` touched: the icon and tooltip, and for [`Change::Menu`] the menu.
fn refresh(app: &AppHandle, change: Change) {
    if change == Change::None {
        return;
    }
    let Some(tray) = app.try_state::<Tray>() else {
        return;
    };
    let view = view(app, &tray, change);
    match &tray.backend {
        // Tauri's tray and menu calls wait for the main thread. Made from the engine event task
        // while holding the menu lock, they deadlock with a refresh the main thread makes itself
        // (a sync command or a menu click), freezing the window and all engine events. On the
        // main thread they run directly (and right away when already there).
        Backend::Tauri(_) => {
            let main = app.clone();
            let result = app.run_on_main_thread(move || {
                if let Some(tray) = main.try_state::<Tray>()
                    && let Some(handles) = tray.backend.tauri()
                {
                    refresh_tauri(&main, handles, view);
                }
            });
            if let Err(e) = result {
                tracing::warn!(error = %e, "updating the tray icon failed");
            }
        }
        #[cfg(target_os = "linux")]
        Backend::Ksni(handle) => {
            let handle = handle.clone();
            // Updates run in order of spawning per handle only loosely; each carries a whole
            // view, and the newest event refreshes again, so a late one is soon replaced.
            tauri::async_runtime::spawn(async move {
                handle.update(|t| t.show(view)).await;
            });
        }
    }
}

fn refresh_tauri(app: &AppHandle, handles: &TauriTray, view: View) {
    let mut result = handles.icon.set_tooltip(Some(view.tooltip));
    if let Some(icon) = view.icon {
        result = result.and_then(|()| handles.icon.set_icon(Some(icon)));
    }
    if let Some(entries) = view.menu {
        let mut shown = handles.menu.lock().unwrap_or_else(|e| e.into_inner());
        if shown.entries != entries {
            let mut actions = Vec::new();
            result = result
                .and_then(|()| tauri_menu(app, &entries, &mut actions))
                .and_then(|menu| handles.icon.set_menu(Some(menu)));
            *shown = Shown { entries, actions };
        }
    }
    if let Err(e) = result {
        tracing::warn!(error = %e, "updating the tray icon failed");
    }
}

fn tauri_menu(
    app: &AppHandle,
    entries: &[Entry],
    actions: &mut Vec<Action>,
) -> tauri::Result<Menu<Wry>> {
    let menu = Menu::new(app)?;
    for entry in entries {
        menu.append(&*tauri_item(app, entry, actions)?)?;
    }
    Ok(menu)
}

/// One Tauri menu item; clickable items get the index of their action as id.
fn tauri_item(
    app: &AppHandle,
    entry: &Entry,
    actions: &mut Vec<Action>,
) -> tauri::Result<Box<dyn IsMenuItem<Wry>>> {
    fn id(actions: &mut Vec<Action>, action: &Action) -> String {
        actions.push(action.clone());
        (actions.len() - 1).to_string()
    }
    let dot = |dot: Option<menu::Dot>| dot.map(|d| icon::dot(d.colour(), DOT_SIZE));
    Ok(match entry {
        Entry::Info { label, dot: d } => match dot(*d) {
            Some(image) => Box::new(IconMenuItem::new(
                app,
                label,
                false,
                Some(image),
                None::<&str>,
            )?),
            None => Box::new(MenuItem::new(app, label, false, None::<&str>)?),
        },
        Entry::Separator => Box::new(PredefinedMenuItem::separator(app)?),
        Entry::Button {
            action,
            label,
            dot: d,
        } => {
            let id = id(actions, action);
            match dot(*d) {
                Some(image) => Box::new(IconMenuItem::with_id(
                    app,
                    id,
                    label,
                    true,
                    Some(image),
                    None::<&str>,
                )?),
                None => Box::new(MenuItem::with_id(app, id, label, true, None::<&str>)?),
            }
        }
        Entry::Toggle {
            action,
            label,
            enabled,
            checked,
        } => Box::new(CheckMenuItem::with_id(
            app,
            id(actions, action),
            label,
            *enabled,
            *checked,
            None::<&str>,
        )?),
        Entry::Submenu { label, entries } => {
            let submenu = Submenu::new(app, label, !entries.is_empty())?;
            for entry in entries {
                submenu.append(&*tauri_item(app, entry, actions)?)?;
            }
            Box::new(submenu)
        }
    })
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

#[cfg(test)]
mod tests {
    use super::*;

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
            for (text, placeholders) in [
                (&parsed.connected_in, &["{channel}"][..]),
                (&parsed.server_state, &["{server}", "{state}"]),
                (&parsed.channels_of, &["{server}"]),
                (&parsed.chat_unread, &["{name}", "{count}"]),
            ] {
                for placeholder in placeholders {
                    assert!(
                        text.contains(placeholder),
                        "{code}: {text} lacks {placeholder}"
                    );
                }
            }
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

    #[test]
    fn tooltip_names_app_and_status() {
        assert_eq!(tooltip("Connected"), "Mumcord – Connected");
    }
}
