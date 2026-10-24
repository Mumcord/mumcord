//! Tauri shell: registers plugins and exposes the engine to the UI via typed commands.

mod commands;
mod deeplink;
#[cfg(target_os = "linux")]
mod desktop_entry;
mod display;
mod ptt;
mod tray;
mod window;

use mumcord_engine::{Engine, EngineEvent, SecretStore, diag};
use tauri::{AppHandle, Manager};
use tauri_specta::{Builder, Event, collect_commands, collect_events};

use commands::{
    app, audio, channels, chat, diagnostics, follow, history, identity, people, profile, servers,
    session, settings,
};

/// Path of the generated TypeScript bindings, relative to this crate.
const BINDINGS_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../ui/src/bindings/commands.ts"
);

/// Collects all typed commands and events shared with the UI (contract: docs/engine-api.md).
fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        // One TS type per Rust type: `Option` fields stay required `T | null` instead of
        // splitting every type into `_Serialize`/`_Deserialize` variants.
        .disable_serde_phases()
        .commands(collect_commands![
            identity::identity_get,
            identity::identity_create,
            identity::identity_import,
            identity::identity_export,
            servers::servers_list,
            servers::server_add,
            servers::server_update,
            servers::server_remove,
            servers::servers_reorder,
            servers::parse_mumble_link,
            session::connect,
            session::disconnect,
            session::reconnect_cancel,
            session::connection_state,
            session::session_snapshot,
            session::connection_stats,
            session::cert_trust_decide,
            session::set_self_mute,
            session::set_self_deaf,
            channels::channel_join,
            people::person_set_volume,
            people::person_set_local_mute,
            people::person_profile,
            chat::send_channel_message,
            chat::send_private_message,
            follow::channel_follow,
            follow::channel_unfollow,
            follow::conversation_mark_read,
            history::history_load,
            history::history_clear,
            history::history_set_saving,
            profile::set_own_avatar,
            profile::default_avatar_get,
            profile::default_avatar_set,
            profile::set_own_comment,
            profile::register_self,
            settings::settings_get,
            settings::settings_update,
            audio::audio_devices,
            audio::mic_test_start,
            audio::mic_test_stop,
            commands::ptt::ptt_availability,
            commands::ptt::ptt_capture_key,
            commands::ptt::ptt_capture_cancel,
            commands::ptt::ptt_mouse_status,
            commands::ptt::ptt_session,
            app::close_choice,
            app::app_restart,
            app::link_handler_status,
            app::link_handler_register,
            app::deep_link_take_pending,
            diagnostics::diagnostics_save,
            diagnostics::app_info,
        ])
        .events(collect_events![
            servers::DeepLinkOpened,
            session::ConnectionStateChanged,
            session::CertTrustRequired,
            session::ServerSynced,
            session::TransportChanged,
            session::ServerDenied,
            channels::ChannelUpserted,
            channels::ChannelRemoved,
            people::UserUpserted,
            people::UserRemoved,
            people::TalkStateChanged,
            chat::MessageReceived,
            follow::FollowedChannelsChanged,
            follow::UnreadChanged,
            audio::AudioDevicesChanged,
            audio::MicLevel,
            audio::MicTestPhaseChanged,
            app::CloseRequested,
            profile::DefaultAvatarSkipped,
        ])
}

fn export_bindings(builder: &Builder<tauri::Wry>) -> Result<(), specta_typescript::Error> {
    builder.export(specta_typescript::Typescript::default(), BINDINGS_PATH)
}

/// Starts the desktop app.
pub fn run() {
    let context = tauri::generate_context!();
    // Before anything else starts a thread or GTK: it may switch to XWayland (FR-1.48).
    display::prepare(&context.config().identifier);
    #[cfg(target_os = "linux")]
    {
        // The Wayland app ID comes from the program name; it must match the desktop entry
        // (`desktop_entry`) for the taskbar icon. Set before GTK starts.
        gdk::glib::set_prgname(Some(context.config().identifier.as_str()));
        ptt::set_host_app_id(&context.config().identifier);
    }

    let builder = specta_builder();

    // Keep the TypeScript bindings in sync during development.
    // E2E runs set MUMCORD_SKIP_BINDINGS_EXPORT so they never rewrite the tracked file.
    #[cfg(debug_assertions)]
    if std::env::var_os("MUMCORD_SKIP_BINDINGS_EXPORT").is_none()
        && let Err(err) = export_bindings(&builder)
    {
        eprintln!("failed to export TypeScript bindings: {err}");
    }

    let app = tauri::Builder::default()
        // Must be registered first so a second launch hands over to the running instance.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            window::show(app);
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(builder.invoke_handler())
        .on_window_event(window::on_window_event)
        .setup(move |app| {
            builder.mount_events(app);
            start_engine(app.handle())?;
            tray::init(app.handle());
            deeplink::init(app.handle());
            #[cfg(target_os = "linux")]
            desktop_entry::ensure(app.handle());
            Ok(())
        })
        .run(context);

    if let Err(err) = app {
        eprintln!("error while running Mumcord: {err}");
        std::process::exit(1);
    }
}

/// Opens the engine, starts logging and wires engine events, diagnostics and push-to-talk.
fn start_engine(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = app.path().app_data_dir()?;
    let logs_dir = app.path().app_log_dir()?;
    std::fs::create_dir_all(&data_dir)?;
    std::fs::create_dir_all(&logs_dir)?;

    let runtime = tauri::async_runtime::handle().inner().clone();
    let (engine, mut events) = Engine::new(&data_dir, SecretStore::os(), runtime)?;
    let stored = engine.settings()?;
    if let Err(e) = diag::init(&logs_dir, settings::diag_level(stored.log_level), 7) {
        eprintln!("file logging unavailable: {e}");
    }
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "Mumcord starting");

    let info_engine = engine.clone();
    diag::set_info_provider(move || info_engine.diagnostic_info(env!("CARGO_PKG_VERSION")));

    let controls = engine.dsp_controls();
    ptt::set_handler(move |pressed| controls.set_ptt_pressed(pressed));
    ptt::set_evdev_mouse_allowed(stored.allow_mouse_buttons);
    settings::configure_ptt(app, settings::active_ptt_key(&stored));

    app.manage(engine);

    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(event) = events.recv().await {
            if let Err(e) = emit_engine_event(&handle, event) {
                tracing::warn!(error = %e, "forwarding an engine event to the UI failed");
            }
        }
    });
    Ok(())
}

/// Converts an engine event into its UI event and emits it.
fn emit_engine_event(app: &AppHandle, event: EngineEvent) -> tauri::Result<()> {
    tray::on_engine_event(app, &event);
    match event {
        EngineEvent::StateChanged(state) => session::ConnectionStateChanged {
            state: state.into(),
        }
        .emit(app),
        EngineEvent::CertTrustRequired {
            server_id,
            fingerprint,
            previous_fingerprint,
        } => session::CertTrustRequired {
            server_id: commands::types::to_api_server_id(server_id),
            fingerprint,
            previous_fingerprint,
        }
        .emit(app),
        EngineEvent::ServerSynced(snapshot) => session::ServerSynced {
            snapshot: (*snapshot).into(),
        }
        .emit(app),
        EngineEvent::TransportChanged(transport) => session::TransportChanged {
            transport: transport.into(),
        }
        .emit(app),
        EngineEvent::ServerDenied { error, channel_id } => session::ServerDenied {
            error: error.into(),
            channel_id,
        }
        .emit(app),
        EngineEvent::ChannelUpserted(channel) => channels::ChannelUpserted {
            channel: channel.into(),
        }
        .emit(app),
        EngineEvent::ChannelRemoved(channel_id) => {
            channels::ChannelRemoved { channel_id }.emit(app)
        }
        EngineEvent::UserUpserted(user) => people::UserUpserted { user: user.into() }.emit(app),
        EngineEvent::UserRemoved(session) => people::UserRemoved { session }.emit(app),
        EngineEvent::TalkStateChanged { session, talking } => {
            people::TalkStateChanged { session, talking }.emit(app)
        }
        EngineEvent::FollowedChannelsChanged(channel_ids) => {
            follow::FollowedChannelsChanged { channel_ids }.emit(app)
        }
        EngineEvent::AudioDevicesChanged(devices) => audio::AudioDevicesChanged {
            devices: devices.into(),
        }
        .emit(app),
        EngineEvent::MicLevel(level) => audio::MicLevel { level }.emit(app),
        EngineEvent::MicTestPhaseChanged(phase) => audio::MicTestPhaseChanged {
            phase: phase.into(),
        }
        .emit(app),
        EngineEvent::MessageReceived(message) => chat::MessageReceived {
            message: (*message).into(),
        }
        .emit(app),
        EngineEvent::UnreadChanged(unread) => follow::UnreadChanged {
            conversation: unread.conversation.into(),
            count: unread.count,
        }
        .emit(app),
        EngineEvent::DefaultAvatarSkipped { server_id } => profile::DefaultAvatarSkipped {
            server_id: commands::types::to_api_server_id(server_id),
        }
        .emit(app),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_typescript_bindings() {
        export_bindings(&specta_builder()).expect("bindings export failed");
    }
}
