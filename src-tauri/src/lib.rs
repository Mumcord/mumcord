//! Tauri shell: registers plugins and exposes the engine to the UI via typed commands.

mod autostart;
mod aux_window;
mod browser;
mod cache_dirs;
mod commands;
mod deeplink;
#[cfg(target_os = "linux")]
mod desktop_entry;
mod display;
mod dnd;
#[cfg(target_os = "linux")]
mod gst_fallback;
mod hotkeys;
mod ingame_overlay;
mod kde;
#[cfg(target_os = "linux")]
mod kde_x11;
mod notifications;
mod now_gaming;
mod now_playing;
mod plugins;
mod portable;
mod ptt;
mod screen_frames;
mod tray;
mod updater;
mod version;
mod watch_player;
#[cfg(target_os = "linux")]
mod webkit_compositing;
#[cfg(target_os = "windows")]
mod webview2;
mod window;

use mumcord_engine::{Engine, EngineEvent, SecretStore, diag};
use tauri::{AppHandle, Manager};
use tauri_specta::{Builder, Event, collect_commands, collect_events};

use commands::{
    app, audio, blocking, channel_audio, channels, chat, diagnostics, dropped, follow, friends,
    history, identity, links, mumble_import, notifications as notify, people, pm_overview,
    pm_rules, profile, servers, session, settings,
};

/// Path of the generated TypeScript bindings, relative to this crate.
// Only debug builds and tests regenerate the bindings.
#[cfg(any(debug_assertions, test))]
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
            identity::identity_list,
            identity::identity_create,
            identity::identity_import,
            identity::identity_rename,
            identity::identity_export,
            identity::identity_delete,
            identity::identity_set_default,
            identity::server_set_identity,
            mumble_import::mumble_import_scan,
            mumble_import::mumble_import_apply,
            mumble_import::mumble_import_undo,
            servers::servers_list,
            servers::server_add,
            servers::server_test_connection,
            servers::server_update,
            servers::server_remove,
            servers::servers_reorder,
            servers::server_set_always_connect,
            servers::server_set_notifications_muted,
            servers::server_set_group_mentions_muted,
            servers::server_set_p2p_mode_override,
            servers::server_set_theme,
            servers::parse_mumble_link,
            session::connect,
            session::disconnect,
            session::reconnect_cancel,
            session::connections_list,
            session::voice_server_get,
            session::session_snapshot,
            session::connection_stats,
            session::latency_estimate,
            session::cert_trust_decide,
            session::set_self_mute,
            session::set_self_deaf,
            channels::channel_join,
            channels::voice_leave,
            channels::channel_join_with_password,
            channels::channel_passwords,
            channels::channel_password_rename,
            channels::channel_passwords_remove,
            channels::channel_description_request,
            people::person_move,
            people::person_set_volume,
            people::person_set_local_mute,
            people::person_set_note,
            people::person_profile,
            friends::friends_list,
            friends::friend_add,
            friends::friend_remove,
            pm_rules::pm_requests,
            pm_rules::pm_request_accept,
            pm_rules::pm_request_decline,
            pm_rules::chat_request_send,
            pm_rules::accepted_conversations,
            pm_rules::accepted_conversation_revoke,
            blocking::blocked_users,
            blocking::block_user,
            blocking::block_cert,
            blocking::unblock,
            pm_overview::private_conversations,
            pm_overview::pm_conversation_prefs,
            pm_overview::pm_conversation_set_pinned,
            pm_overview::pm_conversation_mute,
            pm_overview::pm_conversation_unmute,
            chat::send_channel_message,
            chat::send_private_message,
            chat::save_image,
            chat::copy_image,
            chat::save_text,
            chat::copy_text,
            dropped::read_dropped_image,
            dropped::clipboard_files,
            dropped::clipboard_image,
            commands::full_image::full_image_keep,
            commands::full_image::full_image_fetch,
            commands::full_image::full_image_cancel,
            links::link_preview,
            links::open_link,
            links::mumble_link_stats,
            links::mumble_link_join,
            commands::public_servers::public_servers_list,
            commands::public_servers::public_server_ping,
            links::inline_image,
            links::audio_info,
            links::audio_file,
            links::audio_save,
            links::preview_dismiss,
            links::embed_player_open,
            links::embed_player_close,
            links::gif_favorites,
            links::gif_favorite_add,
            links::gif_favorite_remove,
            links::gif_favorite_use,
            links::preview_cache_clear,
            commands::cache::cache_sizes,
            commands::cache::cache_clear,
            follow::channel_follow,
            follow::channel_unfollow,
            follow::channel_follow_all,
            follow::channel_unfollow_all,
            follow::conversation_mark_read,
            history::history_load,
            history::history_clear,
            history::history_cleared_at,
            history::history_set_saving,
            history::history_sync_request,
            profile::set_own_avatar,
            profile::default_avatar_get,
            profile::default_avatar_set,
            profile::set_own_comment,
            profile::register_self,
            settings::settings_get,
            settings::settings_update,
            settings::settings_default,
            settings::window_overlay_notice_pending,
            settings::window_overlay_notice_dismiss,
            settings::audio_device_notice_pending,
            settings::audio_device_notice_dismiss,
            audio::audio_devices,
            audio::bitrate_max,
            channel_audio::channel_audio_profiles,
            channel_audio::channel_audio_profile_set,
            channel_audio::active_audio_profile,
            audio::mic_test_start,
            audio::audio_test_sound,
            audio::mic_test_stop,
            commands::whisper::whisper_setup,
            commands::whisper::whisper_targets_save,
            commands::whisper::whisper_key_check,
            commands::whisper::whisper_start,
            commands::whisper::whisper_stop,
            commands::hotkeys::hotkey_check,
            commands::hotkeys::hotkeys_status,
            commands::ptt::ptt_availability,
            commands::ptt::ptt_capture_key,
            commands::ptt::ptt_capture_cancel,
            commands::ptt::ptt_mouse_status,
            commands::ptt::ptt_session,
            commands::ptt::ptt_open_kde_x11_settings,
            commands::ptt::ptt_kde_x11_access,
            commands::ptt::ptt_kde_x11_allow_mouse,
            commands::ptt::ptt_kde_x11_undo,
            app::close_choice,
            app::app_restart,
            app::link_handler_status,
            app::link_handler_register,
            app::deep_link_take_pending,
            app::plugins_take_pending_link,
            commands::plugins::plugins_list,
            commands::plugins::plugins_index,
            commands::plugins::plugin_install,
            commands::plugins::plugin_install_prepare,
            commands::plugins::plugin_index_image,
            commands::plugins::plugin_set_enabled,
            commands::plugins::plugin_set_permissions,
            commands::plugins::plugin_set_visibility,
            commands::plugins::plugin_set_notifications,
            commands::plugins::plugin_set_pinned,
            commands::plugins::plugin_remove,
            commands::plugins::plugin_updates,
            commands::plugins::plugin_apply_update,
            commands::plugins::plugin_export_data,
            commands::plugins::plugin_reset_data,
            commands::plugins::plugin_dev_load,
            commands::plugins::plugin_dev_reload,
            commands::plugins::plugin_dev_unload,
            commands::plugins::plugins_set_all_off,
            commands::plugins::plugin_set_badge,
            commands::plugins::plugin_diagnostics,
            commands::plugins::plugin_asset,
            commands::plugins::plugins_ui_trees,
            commands::plugins::plugins_set_appearance,
            commands::plugins::plugin_ui_action,
            commands::plugins::plugin_ui_event,
            commands::plugins::plugins_mentioned,
            commands::plugin_repos::plugin_repos_list,
            commands::plugin_repos::plugin_repo_preview,
            commands::plugin_repos::plugin_repo_add,
            commands::plugin_repos::plugin_repo_set_enabled,
            commands::plugin_repos::plugin_repo_remove,
            commands::plugin_repos::plugin_repo_review_key,
            commands::plugin_repos::plugin_repo_accept_key,
            commands::plugins::plugin_commands,
            commands::plugins::plugin_run_command,
            plugins::suggestions::plugin_suggestion_not_now,
            plugins::suggestions::plugin_suggestions_dismiss_server,
            diagnostics::diagnostics_save,
            diagnostics::app_info,
            commands::inspector::packet_capture_set,
            commands::inspector::packet_capture_background,
            commands::inspector::packet_capture_poll,
            commands::inspector::packet_capture_clear,
            commands::inspector::packet_capture_export,
            commands::inspector::packet_inspector_open,
            commands::inspector::packet_inspector_close,
            commands::inspector::packet_inspector_share,
            commands::inspector::packet_inspector_view,
            commands::licences::licences_open,
            commands::licences::licences_close,
            commands::licences::window_look_share,
            commands::licences::window_look,
            commands::changelog::changelog_open,
            commands::changelog::changelog_close,
            commands::changelog::whats_new_mark_seen,
            notify::notify_message,
            commands::dnd::dnd_state,
            commands::dnd::dnd_set,
            commands::dnd::dnd_set_follow_system,
            notify::notification_dismiss,
            commands::tray::tray_attention_set,
            commands::now_playing::now_playing_status,
            commands::now_playing::now_playing_cover,
            commands::now_gaming::now_gaming_status,
            commands::now_gaming::now_gaming_icon,
            commands::now_gaming::now_gaming_join,
            commands::positional::positional_status,
            commands::positional::game_plugins_list,
            commands::positional::game_plugin_set_enabled,
            commands::ingame_overlay::ingame_overlay_state,
            commands::ingame_overlay::ingame_overlay_set_mode,
            commands::ingame_overlay::ingame_overlay_set_game,
            commands::ingame_overlay::ingame_overlay_remove_game,
            commands::user_stats::user_stats_watch,
            commands::p2p::p2p_consent_decide,
            commands::p2p::relay_info,
            commands::p2p::p2p_link,
            commands::p2p::relay_trust_new,
            commands::p2p::relay_keep_blocked,
            commands::p2p::relay_forget,
            commands::watch_party::watch_party_start,
            commands::watch_party::watch_party_command,
            commands::watch_party::watch_party_report,
            commands::watch_party::watch_party_join,
            commands::watch_party::watch_party_get,
            commands::watch_party::watch_party_leave,
            commands::watch_party::watch_party_add,
            commands::watch_party::watch_party_remove,
            commands::watch_party::watch_party_move,
            commands::watch_party::watch_party_previous,
            commands::watch_party::watch_party_next,
            commands::watch_party::watch_party_player_url,
            commands::screen_share::screen_share_start,
            commands::screen_share::screen_share_stop,
            commands::screen_share::screen_share_set_paused,
            commands::screen_share::screen_share_set_sound,
            commands::screen_share::screen_share_set_sound_level,
            commands::screen_share::screen_share_watch,
            commands::screen_share::screen_share_unwatch,
            commands::screen_share::screen_share_set_decode_fallback,
            commands::screen_share::screen_share_request_keyframe,
            commands::screen_share::screen_share_list,
            commands::screen_share::screen_share_block,
            commands::screen_share::screen_share_quality,
            commands::screen_share::screen_share_thumbnail,
            commands::screen_share::screen_share_watch_stats,
            commands::screen_share::screen_share_frames_attach,
            commands::media_popout::media_popout_open,
            commands::media_popout::media_popout_close,
            commands::media_popout::media_popout_set_on_top,
            commands::media_popout::media_popout_set_title,
            commands::media_popout::media_popout_state,
            commands::updates::update_status,
            commands::updates::update_check_now,
            commands::updates::update_download,
            commands::updates::update_cancel,
            commands::updates::update_restart,
            commands::updates::appimage_menu_entry_add,
        ])
        .events(collect_events![
            servers::DeepLinkOpened,
            session::ConnectionStateChanged,
            session::CertTrustRequired,
            session::ServerSynced,
            session::SyncProgress,
            session::TransportChanged,
            session::ServerDenied,
            channels::ChannelPasswordPrompt,
            channels::ChannelUpserted,
            channels::ChannelsUpserted,
            channels::ChannelRemoved,
            people::UserUpserted,
            people::UserRemoved,
            people::TalkStateChanged,
            friends::FriendsChanged,
            pm_rules::PmRequestsChanged,
            pm_rules::AcceptedConversationsChanged,
            blocking::BlockedUsersChanged,
            pm_overview::PmConversationPrefsChanged,
            chat::MessageReceived,
            history::HistorySynced,
            history::HistorySyncStatus,
            commands::full_image::FullImageProgress,
            follow::FollowedChannelsChanged,
            follow::UnreadChanged,
            audio::AudioDevicesChanged,
            audio::AudioDeviceFallback,
            audio::BitrateMaxChanged,
            channel_audio::ChannelAudioProfilesChanged,
            channel_audio::ActiveAudioProfileChanged,
            audio::MicLevel,
            audio::MicTestPhaseChanged,
            app::CloseRequested,
            profile::DefaultAvatarSkipped,
            identity::ServerIdentityMissing,
            notify::NotificationActivated,
            commands::dnd::DndChanged,
            commands::now_playing::NowPlayingChanged,
            commands::now_gaming::NowGamingChanged,
            commands::positional::PositionalStatusChanged,
            commands::positional::PositionalSendingChanged,
            commands::whisper::WhisperStateChanged,
            commands::whisper::IncomingWhisperChanged,
            commands::ingame_overlay::InGameOverlayChanged,
            session::VoiceServerChanged,
            commands::user_stats::UserStatsChanged,
            commands::p2p::DirectConsentRequired,
            commands::p2p::RelayChanged,
            commands::p2p::P2pLinkChanged,
            commands::watch_party::WatchPartyChanged,
            commands::screen_share::ScreenSharesChanged,
            commands::updates::UpdateStatusChanged,
            commands::inspector::InspectorViewChanged,
            commands::licences::WindowLookChanged,
            commands::media_popout::MediaPopoutChanged,
            commands::tray::TraySettingsRequested,
            commands::hotkeys::HotkeyPressed,
            plugins::PluginsChanged,
            deeplink::PluginsOpenDetails,
            plugins::PluginInstallProgress,
            plugins::ui::PluginUi,
            plugins::PluginNotification,
            plugins::PluginSupersede,
            plugins::suggestions::PluginSuggestion,
        ])
}

#[cfg(any(debug_assertions, test))]
fn export_bindings(builder: &Builder<tauri::Wry>) -> Result<(), specta_typescript::Error> {
    builder.export(specta_typescript::Typescript::default(), BINDINGS_PATH)
}

/// Starts the desktop app.
pub fn run() {
    #[cfg(target_os = "windows")]
    if !webview2::ensure_available() {
        std::process::exit(1);
    }
    let context = tauri::generate_context!();
    // Before anything else starts a thread or GTK: it may switch to XWayland (FR-1.48).
    display::prepare(&context.config().identifier);
    // Before any web view holds the web cache files open.
    cache_dirs::apply_pending(&context.config().identifier);
    #[cfg(target_os = "linux")]
    {
        // Before WebKit starts its web process, which inherits the GStreamer environment.
        gst_fallback::prepare(&context.config().identifier);
        // WebKit reads this when its web process starts (flicker workaround).
        webkit_compositing::prepare();
        // The Wayland app ID comes from the program name; it must match the desktop entry
        // (`desktop_entry`) for the taskbar icon. Set before GTK starts.
        gdk::glib::set_prgname(Some(context.config().identifier.as_str()));
        ptt::set_host_app_id(&context.config().identifier);
    }

    let builder = specta_builder();
    let invoke = builder.invoke_handler();

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
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            // A login start while Mumcord already runs must not pop the window up.
            if !autostart::launched_at_login(&argv) {
                window::show(app);
            }
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(move |call| {
            let label = call.message.webview_ref().label().to_owned();
            if !aux_window::command_allowed(&label, call.message.command()) {
                call.resolver.reject(commands::types::ApiError::new(
                    commands::types::ErrorCode::PermissionDenied,
                    "this command is not available in this window",
                ));
                return true;
            }
            invoke(call)
        })
        .manage(dropped::DroppedFiles::default())
        .manage(aux_window::OpenLock::default())
        .manage(commands::inspector::SharedView::default())
        .manage(commands::inspector::CaptureMode::default())
        .manage(commands::licences::SharedLook::default())
        .manage(watch_player::PlayerServer::default())
        .on_window_event(|window, event| {
            dropped::on_window_event(window, event);
            window::on_window_event(window, event);
            aux_window::on_window_event(window, event);
            commands::inspector::on_window_event(window, event);
            commands::media_popout::on_window_event(window, event);
        })
        .setup(move |app| {
            builder.mount_events(app);
            app.manage(commands::media_popout::MediaPopouts::load(app.handle()));
            // Before the engine: its event hook uses the notifier.
            notifications::init(app.handle());
            create_main_window(app.handle())?;
            start_engine(app.handle())?;
            start_window_and_autostart(app.handle());
            tray::init(app.handle());
            dnd::init(app.handle());
            ingame_overlay_init(app.handle());
            deeplink::init(app.handle());
            #[cfg(target_os = "linux")]
            {
                desktop_entry::ensure(app.handle());
            }
            Ok(())
        })
        .run(context);

    if let Err(err) = app {
        eprintln!("error while running Mumcord: {err}");
        std::process::exit(1);
    }
}

/// Creates the main window from its entry in tauri.conf.json (`create: false` there), so that
/// portable mode can point the web view's data at the portable folder.
fn create_main_window(app: &AppHandle) -> tauri::Result<()> {
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == "main")
        .ok_or(tauri::Error::WindowNotFound)?
        .clone();
    let mut builder = tauri::WebviewWindowBuilder::from_config(app, &config)?;
    if let Some(dir) = portable::webview_data_dir() {
        builder = builder.data_directory(dir);
    }
    builder.build()?;
    Ok(())
}

/// Opens the engine, starts logging and wires engine events, diagnostics and push-to-talk.
fn start_engine(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = portable::data_dir(app)?;
    let logs_dir = portable::log_dir(app)?;
    std::fs::create_dir_all(&data_dir)?;
    std::fs::create_dir_all(&logs_dir)?;

    let runtime = tauri::async_runtime::handle().inner().clone();
    let (engine, mut events) = Engine::new(&data_dir, SecretStore::os(), runtime)?;
    let stored = engine.settings()?;
    if let Err(e) = diag::init(&logs_dir, settings::diag_level(stored.log_level), 7) {
        eprintln!("file logging unavailable: {e}");
    }
    tracing::info!(
        version = version::DISPLAY,
        portable = portable::is_active(),
        "Mumcord starting"
    );
    mumcord_protocol::set_client_release(format!("Mumcord {}", version::DISPLAY));

    let info_engine = engine.clone();
    let diag_version = if portable::is_active() {
        format!("{} (portable)", version::DISPLAY)
    } else {
        version::DISPLAY.to_owned()
    };
    diag::set_info_provider(move || info_engine.diagnostic_info(&diag_version));

    let controls = engine.dsp_controls();
    ptt::set_handler(move |pressed| controls.set_ptt_pressed(pressed));
    ptt::set_evdev_mouse_allowed(stored.allow_mouse_buttons);
    settings::configure_ptt(app, settings::active_ptt_key(&stored));
    commands::whisper::install_key_handler(app);
    commands::whisper::configure_keys(app, &stored);
    commands::hotkeys::configure(app, &stored);

    let screen_frames = std::sync::Arc::new(screen_frames::ScreenFrames::default());
    engine.set_screen_frame_sink(screen_frames.clone());
    app.manage(screen_frames);
    now_playing::start(engine.clone());
    now_gaming::start(engine.clone());
    start_plugin_host(app, &engine, &data_dir, &stored.enabled_plugins);
    plugins::start(app, &engine);
    app.manage(engine.clone());
    let at_start = engine.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = at_start.connect_at_start().await {
            tracing::warn!(error = %e, "connecting the servers at start failed");
        }
    });
    let updater = updater::Updater::new(app.clone(), engine);
    app.manage(updater.clone());
    updater.start();

    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        // App-wide events carry no server; their UI events ignore it.
        while let Some((server_id, event)) = events.recv_tagged().await {
            let server_id = server_id.map_or(0, commands::types::to_api_server_id);
            if let Err(e) = emit_engine_event(&handle, server_id, event) {
                tracing::warn!(error = %e, "forwarding an engine event to the UI failed");
            }
        }
    });
    Ok(())
}

/// Starts the host for the official Mumble game plugins (FR-1.42b) and applies the enabled
/// ones. Without it positional audio still works through Mumble Link.
fn start_plugin_host(
    app: &AppHandle,
    engine: &Engine,
    data_dir: &std::path::Path,
    enabled: &[String],
) {
    let dirs = mumcord_engine::positional_sources::plugins::plugin_dirs(data_dir.join("plugins"));
    match mumcord_engine::positional_sources::PluginHost::start(dirs) {
        Ok(host) => {
            if let Err(e) = commands::positional::apply_plugins(engine, &host, enabled) {
                tracing::warn!(error = %e, "enabling the game plugins failed");
            }
            app.manage(host);
        }
        Err(e) => tracing::error!(error = %e, "starting the game plugin host failed"),
    }
}

/// Sets up the in-game overlay (FR-1.46b): starts its IPC server, installs the Vulkan layer
/// manifest if the overlay is on, and manages the runtime so commands and events can reach it.
fn ingame_overlay_init(app: &AppHandle) {
    let ingame = match app.state::<Engine>().settings() {
        Ok(stored) => stored.ingame_overlay,
        Err(e) => {
            tracing::warn!(error = %e, "reading the in-game overlay settings failed");
            return;
        }
    };
    let shared = std::sync::Arc::new(ingame_overlay::Shared::default());
    let runtime = ingame_overlay::init(shared, ingame);
    runtime.shared.rebuild(&app.state::<Engine>());
    app.manage(runtime);
}

/// Shows the window (or keeps it in the tray after a start at login) and brings the OS
/// autostart entry in line with the stored setting (FR-1.48d).
fn start_window_and_autostart(app: &AppHandle) {
    match app.state::<Engine>().settings() {
        Ok(stored) => {
            let at_login = autostart::launched_at_login(std::env::args_os());
            window::reveal_at_start(app, autostart::start_hidden(at_login, &stored));
            autostart::sync(app, stored.autostart);
        }
        Err(e) => {
            tracing::warn!(error = %e, "reading the start-up settings failed; showing the window");
            window::reveal_at_start(app, false);
        }
    }
}

/// Converts an engine event into its UI event and emits it; `server_id` is the server the
/// session events belong to.
fn emit_engine_event(
    app: &AppHandle,
    server_id: commands::types::ServerId,
    event: EngineEvent,
) -> tauri::Result<()> {
    tray::on_engine_event(app, server_id, &event);
    notifications::on_engine_event(app, server_id, &event);
    plugins::on_engine_event(app, &event);
    if let (Some(runtime), Some(engine)) = (
        app.try_state::<ingame_overlay::Runtime>(),
        app.try_state::<Engine>(),
    ) {
        ingame_overlay::on_engine_event(&runtime, &engine, &event);
    }
    match event {
        EngineEvent::StateChanged(state) => session::ConnectionStateChanged {
            server_id,
            state: state.into(),
        }
        .emit(app),
        EngineEvent::VoiceServerChanged(voice) => session::VoiceServerChanged {
            server_id: voice.map(commands::types::to_api_server_id),
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
        EngineEvent::SyncProgress { channels } => session::SyncProgress {
            server_id,
            channels,
        }
        .emit(app),
        EngineEvent::TransportChanged(transport) => session::TransportChanged {
            server_id,
            transport: transport.into(),
        }
        .emit(app),
        EngineEvent::ServerDenied { error, channel_id } => session::ServerDenied {
            server_id,
            error: error.into(),
            channel_id,
        }
        .emit(app),
        EngineEvent::ChannelPassword { channel_id, state } => channels::ChannelPasswordPrompt {
            server_id,
            channel_id,
            step: state.into(),
        }
        .emit(app),
        EngineEvent::ChannelUpserted(channel) => channels::ChannelUpserted {
            server_id,
            channel: channel.into(),
        }
        .emit(app),
        EngineEvent::ChannelsUpserted(channels) => channels::ChannelsUpserted {
            server_id,
            channels: channels.into_iter().map(Into::into).collect(),
        }
        .emit(app),
        EngineEvent::ChannelRemoved(channel_id) => channels::ChannelRemoved {
            server_id,
            channel_id,
        }
        .emit(app),
        EngineEvent::UserUpserted(user) => people::UserUpserted {
            server_id,
            user: user.into(),
        }
        .emit(app),
        EngineEvent::UserRemoved(session) => people::UserRemoved { server_id, session }.emit(app),
        EngineEvent::TalkStateChanged { session, talking } => people::TalkStateChanged {
            server_id,
            session,
            talking,
        }
        .emit(app),
        EngineEvent::FollowedChannelsChanged(channel_ids) => follow::FollowedChannelsChanged {
            server_id,
            channel_ids,
        }
        .emit(app),
        EngineEvent::AudioDevicesChanged(devices) => audio::AudioDevicesChanged {
            devices: devices.into(),
        }
        .emit(app),
        EngineEvent::AudioDeviceFallback { input } => {
            audio::AudioDeviceFallback { input }.emit(app)
        }
        EngineEvent::BitrateMaxChanged(max_bps) => audio::BitrateMaxChanged {
            kbps: max_bps.map(|b| b / 1000),
        }
        .emit(app),
        EngineEvent::ChannelAudioProfilesChanged(profiles) => {
            channel_audio::ChannelAudioProfilesChanged {
                server_id,
                profiles: profiles.into_iter().map(Into::into).collect(),
            }
            .emit(app)
        }
        EngineEvent::ActiveAudioProfileChanged(active) => {
            channel_audio::ActiveAudioProfileChanged {
                active: active.map(Into::into),
            }
            .emit(app)
        }
        EngineEvent::MicLevel(level) => audio::MicLevel { level }.emit(app),
        EngineEvent::MicTestPhaseChanged(phase) => audio::MicTestPhaseChanged {
            phase: phase.into(),
        }
        .emit(app),
        EngineEvent::MessageReceived(message) => chat::MessageReceived {
            server_id,
            message: (*message).into(),
        }
        .emit(app),
        EngineEvent::UnreadChanged(unread) => follow::UnreadChanged {
            server_id,
            conversation: unread.conversation.into(),
            count: unread.count,
        }
        .emit(app),
        EngineEvent::DefaultAvatarSkipped { server_id } => profile::DefaultAvatarSkipped {
            server_id: commands::types::to_api_server_id(server_id),
        }
        .emit(app),
        EngineEvent::ServerIdentityMissing { server_id } => identity::ServerIdentityMissing {
            server_id: commands::types::to_api_server_id(server_id),
        }
        .emit(app),
        EngineEvent::NowPlayingChanged { session, track } => {
            commands::now_playing::NowPlayingChanged {
                server_id,
                session,
                track: track.map(Into::into),
            }
            .emit(app)
        }
        EngineEvent::NowGamingChanged { session, game } => commands::now_gaming::NowGamingChanged {
            server_id,
            session,
            game: game.map(Into::into),
        }
        .emit(app),
        EngineEvent::HistorySynced {
            channel_id,
            messages,
        } => history::HistorySynced {
            server_id,
            channel_id,
            messages: messages.into_iter().map(Into::into).collect(),
        }
        .emit(app),
        EngineEvent::HistorySyncStatus {
            channel_id,
            running,
            added,
        } => history::HistorySyncStatus {
            server_id,
            channel_id,
            running,
            added,
        }
        .emit(app),
        EngineEvent::FriendsChanged(list) => friends::FriendsChanged {
            friends: friends::to_api_friends(list),
        }
        .emit(app),
        EngineEvent::PmRequestsChanged(requests) => pm_rules::PmRequestsChanged {
            server_id,
            requests: requests.into(),
        }
        .emit(app),
        EngineEvent::AcceptedConversationsChanged(list) => pm_rules::AcceptedConversationsChanged {
            conversations: pm_rules::to_api_accepted(list),
        }
        .emit(app),
        EngineEvent::BlockedUsersChanged(list) => blocking::BlockedUsersChanged {
            users: blocking::to_api_blocked(list),
        }
        .emit(app),
        EngineEvent::PmConversationPrefsChanged(list) => pm_overview::PmConversationPrefsChanged {
            prefs: pm_overview::to_api_prefs(list),
        }
        .emit(app),
        EngineEvent::DirectConsentRequired => {
            commands::p2p::DirectConsentRequired { server_id }.emit(app)
        }
        EngineEvent::FullImageProgress {
            reference,
            received,
            total,
        } => commands::full_image::FullImageProgress {
            server_id,
            reference,
            received,
            total,
        }
        .emit(app),
        EngineEvent::PositionalStatusChanged(status) => {
            commands::positional::PositionalStatusChanged {
                status: status.into(),
            }
            .emit(app)
        }
        EngineEvent::PositionalSendingChanged { session, sending } => {
            commands::positional::PositionalSendingChanged {
                server_id,
                session,
                sending,
            }
            .emit(app)
        }
        EngineEvent::IncomingWhisperChanged { session, kind } => {
            commands::whisper::IncomingWhisperChanged {
                server_id,
                session,
                kind: kind.map(Into::into),
            }
            .emit(app)
        }
        EngineEvent::UserStatsChanged(stats) => commands::user_stats::UserStatsChanged {
            server_id,
            session: stats.session,
            stats: stats.into(),
        }
        .emit(app),
        EngineEvent::WatchPartyChanged(party) => {
            let joined = party.as_ref().is_some_and(|p| p.joined);
            app.state::<watch_player::PlayerServer>()
                .party_changed(server_id, joined);
            commands::watch_party::WatchPartyChanged {
                server_id,
                party: party.map(Into::into),
            }
            .emit(app)
        }
        EngineEvent::RelayChanged(info) => commands::p2p::RelayChanged {
            server_id,
            info: info.into(),
        }
        .emit(app),
        EngineEvent::P2pLinkChanged { session, link } => commands::p2p::P2pLinkChanged {
            server_id,
            session,
            link: link.map(Into::into),
        }
        .emit(app),
        EngineEvent::ScreenSharesChanged(shares) => {
            let sharers: Vec<u32> = shares.iter().map(|s| s.session).collect();
            app.state::<std::sync::Arc<screen_frames::ScreenFrames>>()
                .retain(commands::types::to_engine_server_id(server_id), &sharers);
            commands::screen_share::ScreenSharesChanged {
                server_id,
                shares: shares.into_iter().map(Into::into).collect(),
            }
            .emit(app)
        }
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
