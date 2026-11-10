//! Carries out the plugin host calls of API 1.1 (FR-9.103 … FR-9.108) once the service let them
//! through (permission and gesture checked): Mumcord's own save dialog and picture picker, and
//! posting a plugin's PNG into the chat.
//!
//! The plugin only ever hands over or receives bytes: it never learns a path, a folder or the
//! chat. Logs name the call, sizes and outcomes only, never contents, names or paths
//! (FR-9.108). The dialogs answer through callbacks and the file work runs on blocking threads,
//! so nothing here holds up the event pump; the plugin's own thread waits for the answer.

use std::io::Read;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use mumcord_engine::Engine;
use mumcord_engine::chat::IMAGE_SOFT_CAP;
use mumcord_engine::files::MAX_FILE_SIZE;
use mumcord_engine::files::sender::ChipState;
use mumcord_plugins::api::host_calls::{
    self, HostRequest, MAX_PICTURE_BYTES, Picture, PostOutcome, Reply,
};
use mumcord_plugins::test_hooks::{self, DialogAnswer};
use mumcord_plugins::wit::types::Error;
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::{DialogExt, FileDialogBuilder};
use tauri_specta::Event;

use super::feed::{escape_html, post_line};
use super::{PluginNotice, PluginNotification};
use crate::commands::chat::MessageReceived;
use crate::commands::types::to_api_server_id;

/// File name of a picture that goes out as a file offer because it is too large for the chat.
const OFFER_NAME: &str = "image.png";

/// Starts one host call; its answer goes back through the request's reply.
pub fn handle(app: &AppHandle, engine: &Engine, plugin: String, request: HostRequest) {
    match request {
        HostRequest::Save { name, data, reply } => save(app, name, data, reply),
        HostRequest::ImportPicture { reply } => import_picture(app, plugin, reply),
        HostRequest::PostImage {
            server,
            line,
            png,
            reply,
        } => {
            let app = app.clone();
            let engine = engine.clone();
            tauri::async_runtime::spawn(async move {
                post_image(&app, &engine, server, &line, png, reply).await;
            });
        }
    }
}

/// A file dialog in front of the main window.
fn dialog(app: &AppHandle) -> FileDialogBuilder<tauri::Wry> {
    let builder = app.dialog().file();
    // Not `get_webview_window`: that finds nothing while a plugin frame's child web view sits in
    // the window (macOS), which is when a plugin usually asks for a file.
    match app.get_window("main") {
        Some(window) => builder.set_parent(&window),
        None => builder,
    }
}

/// Runs `finish` with the dialog's path (`None`: cancelled) on a blocking thread. A test hook
/// answers instead of the native dialog in debug builds.
fn on_answer(
    hook: Option<DialogAnswer>,
    open: impl FnOnce(Box<dyn FnOnce(Option<PathBuf>) + Send>),
    finish: impl FnOnce(Option<PathBuf>) + Send + 'static,
) {
    let run = move |path: Option<PathBuf>| {
        tauri::async_runtime::spawn_blocking(move || finish(path));
    };
    match hook {
        Some(DialogAnswer::Cancel) => run(None),
        Some(DialogAnswer::Path(path)) => run(Some(path)),
        None => open(Box::new(run)),
    }
}

fn save(app: &AppHandle, name: String, data: Vec<u8>, reply: Reply<bool>) {
    let builder = dialog(app).set_file_name(name);
    on_answer(
        test_hooks::plugin_save_dialog(),
        move |run| {
            builder.save_file(move |picked| run(picked.and_then(|p| p.into_path().ok())));
        },
        move |path| {
            let saved = path.is_some_and(|path| write_saved(&path, &data, &reply));
            reply.send(saved);
        },
    );
}

/// Writes the plugin's bytes to the file the user chose, unless the plugin stopped waiting.
fn write_saved(path: &Path, data: &[u8], reply: &Reply<bool>) -> bool {
    if !reply.is_waiting() {
        tracing::info!("plugin files.save answered too late; nothing written");
        return false;
    }
    match std::fs::write(path, data) {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!(kind = ?e.kind(), "plugin files.save could not write the file");
            false
        }
    }
}

fn import_picture(app: &AppHandle, plugin: String, reply: Reply<Result<Option<Picture>, Error>>) {
    let builder = dialog(app).add_filter("Pictures", &["png", "jpg", "jpeg", "webp"]);
    let app = app.clone();
    on_answer(
        test_hooks::plugin_pick_dialog(),
        move |run| {
            builder.pick_file(move |picked| run(picked.and_then(|p| p.into_path().ok())));
        },
        move |path| {
            let Some(path) = path else {
                reply.send(Ok(None));
                return;
            };
            let result = read_picture(&path);
            if let Err(error) = &result {
                tracing::info!(?error, "plugin files.import-picture refused a file");
                let notice = PluginNotification {
                    plugin,
                    notice: PluginNotice::PictureRefused,
                };
                if let Err(e) = notice.emit(&app) {
                    tracing::warn!(error = %e, "could not show the picture notice");
                }
            }
            reply.send(result.map(Some));
        },
    );
}

/// Reads a picked picture: at most [`MAX_PICTURE_BYTES`] (checked before and while reading),
/// then checked by its content.
fn read_picture(path: &Path) -> Result<Picture, Error> {
    let file = std::fs::File::open(path).map_err(|_| Error::Invalid)?;
    let meta = file.metadata().map_err(|_| Error::Invalid)?;
    if !meta.is_file() {
        return Err(Error::Invalid);
    }
    if meta.len() > MAX_PICTURE_BYTES {
        return Err(Error::TooLarge);
    }
    let mut data = Vec::new();
    file.take(MAX_PICTURE_BYTES + 1)
        .read_to_end(&mut data)
        .map_err(|_| Error::Invalid)?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    host_calls::check_picture(&name, data)
}

/// The chat message of a posted picture: the text line, then the image with it as description.
fn image_html(line: &str, png: &[u8]) -> String {
    let line = escape_html(line);
    let data = base64::engine::general_purpose::STANDARD.encode(png);
    format!("<p>{line}</p><img src=\"data:image/png;base64,{data}\" alt=\"{line}\" />")
}

/// The server's image message limit in characters, as the engine applies it.
fn image_limit(max_image_message_length: u32) -> usize {
    match max_image_message_length {
        0 => IMAGE_SOFT_CAP as usize,
        n => n as usize,
    }
}

/// How a picture of this size goes out: `None` when it is too large for file sharing too.
fn outcome_for(html_len: usize, limit: usize, png_len: usize) -> Option<PostOutcome> {
    if html_len <= limit {
        Some(PostOutcome::Image)
    } else if png_len as u64 <= MAX_FILE_SIZE {
        Some(PostOutcome::FileOffer)
    } else {
        None
    }
}

async fn post_image(
    app: &AppHandle,
    engine: &Engine,
    server: mumcord_engine::ServerId,
    line: &str,
    png: Vec<u8>,
    reply: Reply<Result<PostOutcome, Error>>,
) {
    let Some(snapshot) = engine.snapshot_of(server) else {
        reply.send(Err(Error::NoRecipients));
        return;
    };
    let Some(channel) = snapshot
        .users
        .iter()
        .find(|u| u.session == snapshot.self_session)
        .map(|u| u.channel_id)
    else {
        reply.send(Err(Error::NoRecipients));
        return;
    };
    let html = image_html(line, &png);
    let limit = image_limit(snapshot.info.max_image_message_length);
    let outcome = outcome_for(html.encode_utf16().count(), limit, png.len());
    match outcome {
        None => {
            reply.send(Err(Error::TooLarge));
        }
        Some(PostOutcome::Image) => {
            reply.send(Ok(PostOutcome::Image));
            match engine.send_channel_message(server, channel, &html).await {
                Ok(message) => own_copy(app, server, message),
                Err(e) => tracing::info!(error = %e, "plugin image not posted"),
            }
        }
        Some(PostOutcome::FileOffer) => {
            if engine.files_availability(server).is_err() {
                reply.send(Err(Error::TooLarge));
                return;
            }
            let chip = {
                let engine = engine.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    engine.files_chip_add_owned(OFFER_NAME, &png)
                })
                .await
            };
            let chip = match chip {
                Ok(Ok(chip)) if chip.state == ChipState::Ready => chip,
                Ok(Ok(chip)) => {
                    engine.files_chip_remove(chip.id);
                    reply.send(Err(Error::TooLarge));
                    return;
                }
                Ok(Err(e)) => {
                    tracing::warn!(kind = ?e.kind(), "plugin image could not be prepared for sharing");
                    reply.send(Err(Error::TooLarge));
                    return;
                }
                Err(e) => {
                    tracing::warn!(error = %e, "plugin image could not be prepared for sharing");
                    reply.send(Err(Error::TooLarge));
                    return;
                }
            };
            reply.send(Ok(PostOutcome::FileOffer));
            // The line comes first so official clients see what the offer is about.
            match post_line(engine, server, line).await {
                Ok(message) => own_copy(app, server, message),
                Err(e) => tracing::info!(error = %e, "plugin image line not posted"),
            }
            match engine.files_send(server, channel, vec![chip.id]).await {
                Ok(posted) if !posted.is_empty() => {
                    for message in posted {
                        own_copy(app, server, message);
                    }
                }
                Ok(_) => {
                    tracing::info!("plugin image offer refused by the server");
                    engine.files_chip_remove(chip.id);
                }
                Err(e) => {
                    tracing::info!(error = %e, "plugin image offer not posted");
                    engine.files_chip_remove(chip.id);
                }
            }
        }
    }
}

/// Shows the user's own copy of a posted message (the server never echoes it).
fn own_copy(
    app: &AppHandle,
    server: mumcord_engine::ServerId,
    message: mumcord_engine::ChatMessageView,
) {
    let event = MessageReceived {
        server_id: to_api_server_id(server),
        message: message.into(),
    };
    if let Err(e) = event.emit(app) {
        tracing::warn!(error = %e, "could not show a posted plugin image");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_image_message_carries_the_line_as_text_and_description() {
        let html = image_html("[Board] <b>Sketch</b>", &[1, 2, 3]);
        assert_eq!(
            html,
            "<p>[Board] &lt;b&gt;Sketch&lt;/b&gt;</p>\
             <img src=\"data:image/png;base64,AQID\" alt=\"[Board] &lt;b&gt;Sketch&lt;/b&gt;\" />"
        );
    }

    #[test]
    fn too_large_images_become_file_offers() {
        assert_eq!(image_limit(0), IMAGE_SOFT_CAP as usize);
        assert_eq!(image_limit(5000), 5000);
        assert_eq!(outcome_for(5000, 5000, 10), Some(PostOutcome::Image));
        assert_eq!(outcome_for(5001, 5000, 10), Some(PostOutcome::FileOffer));
        let huge = usize::try_from(MAX_FILE_SIZE + 1).unwrap_or(usize::MAX);
        assert_eq!(outcome_for(huge, 5000, huge), None);
    }

    #[test]
    fn pictures_are_read_with_the_size_limit_and_without_their_folder() {
        let dir = tempfile::tempdir().expect("dir");
        let png = dir.path().join("sketch.png");
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::RgbImage::new(2, 2)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .expect("encode");
        std::fs::write(&png, bytes.get_ref()).expect("write");
        let picture = read_picture(&png).expect("picture");
        assert_eq!(picture.name, "sketch.png");
        assert_eq!(&picture.data, bytes.get_ref());

        let fake = dir.path().join("fake.png");
        std::fs::write(&fake, b"<svg/>").expect("write");
        assert_eq!(read_picture(&fake), Err(Error::Invalid));

        let big = dir.path().join("big.png");
        std::fs::File::create(&big)
            .and_then(|f| f.set_len(MAX_PICTURE_BYTES + 1))
            .expect("sparse");
        assert_eq!(read_picture(&big), Err(Error::TooLarge));
        assert_eq!(read_picture(dir.path()), Err(Error::Invalid));
    }

    #[test]
    fn a_save_after_the_plugin_gave_up_writes_nothing() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("board.png");
        let (reply, waiter) = host_calls::reply_channel::<bool>();
        assert_eq!(waiter.wait(std::time::Duration::from_millis(1)), None);
        assert!(!write_saved(&path, b"png", &reply));
        assert!(!path.exists());
        let (reply, _waiter) = host_calls::reply_channel::<bool>();
        assert!(write_saved(&path, b"png", &reply));
        assert_eq!(std::fs::read(&path).expect("read"), b"png");
    }
}
