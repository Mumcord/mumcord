//! Tauri shell: registers plugins and exposes the engine to the UI via typed commands.

mod commands;
mod deeplink;
mod ptt;
mod tray;
mod window;

use tauri_specta::{Builder, collect_commands, collect_events};

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
            app::close_choice,
            app::link_handler_status,
            app::link_handler_register,
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
            app::CloseRequested,
        ])
}

fn export_bindings(builder: &Builder<tauri::Wry>) -> Result<(), specta_typescript::Error> {
    builder.export(specta_typescript::Typescript::default(), BINDINGS_PATH)
}

/// Starts the desktop app.
pub fn run() {
    let builder = specta_builder();

    // Keep the TypeScript bindings in sync during development.
    #[cfg(debug_assertions)]
    if let Err(err) = export_bindings(&builder) {
        eprintln!("failed to export TypeScript bindings: {err}");
    }

    let app = tauri::Builder::default()
        // Must be registered first so a second launch hands over to the running instance.
        .plugin(tauri_plugin_single_instance::init(|_app, _argv, _cwd| {}))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);
            Ok(())
        })
        .run(tauri::generate_context!());

    if let Err(err) = app {
        eprintln!("error while running Mumcord: {err}");
        std::process::exit(1);
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
