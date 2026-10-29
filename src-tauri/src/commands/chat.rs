//! Sending and receiving text messages (FR-1.28–1.30).

use mumcord_engine::{ChatMessageView, Engine};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tauri_specta::Event;

use super::types::{
    ApiError, ApiResult, ChannelId, Conversation, ErrorCode, ServerId, SessionId,
    to_engine_server_id,
};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    /// Locally unique id; also the paging cursor for `history_load`.
    pub id: String,
    pub conversation: Conversation,
    pub sender_name: String,
    pub sender_cert_hash: Option<String>,
    /// Sender's session while still connected; `null` for history entries.
    pub sender_session: Option<SessionId>,
    /// True for messages the own user sent.
    pub own: bool,
    /// Local receive (or send) time, RFC 3339.
    pub sent_at: String,
    /// Sanitized HTML; the UI sanitizes again before rendering.
    pub body: String,
    /// Links whose preview card was dismissed by the user (`preview_dismiss`).
    pub dismissed_previews: Vec<String>,
    /// Name of the person whose history this message was synced from (FR-1.60); `null` for
    /// messages received or sent here. Show "from ‹name›'s history".
    pub synced_from: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct MessageReceived {
    pub server_id: ServerId,
    pub message: ChatMessage,
}

/// Sends Mumble-compatible HTML (the UI converts Markdown and scales images).
/// Waits briefly for the server's refusal (FR-2.47). Fails with `messageTooLarge` above the
/// server limit or `permissionDenied` (the message is then not stored).
#[tauri::command]
#[specta::specta]
pub async fn send_channel_message(
    engine: State<'_, Engine>,
    server_id: ServerId,
    channel_id: ChannelId,
    html: String,
) -> ApiResult<ChatMessage> {
    Ok(engine
        .send_channel_message(to_engine_server_id(server_id), channel_id, &html)
        .await?
        .into())
}

#[tauri::command]
#[specta::specta]
pub async fn send_private_message(
    engine: State<'_, Engine>,
    server_id: ServerId,
    session: SessionId,
    html: String,
) -> ApiResult<ChatMessage> {
    Ok(engine
        .send_private_message(to_engine_server_id(server_id), session, &html)
        .await?
        .into())
}

impl From<ChatMessageView> for ChatMessage {
    fn from(v: ChatMessageView) -> Self {
        Self {
            id: v.id,
            conversation: v.conversation.into(),
            sender_name: v.sender_name,
            sender_cert_hash: v.sender_cert_hash,
            sender_session: v.sender_session,
            own: v.own,
            sent_at: rfc3339(v.sent_at),
            body: v.body,
            dismissed_previews: v.dismissed_previews,
            synced_from: v.synced_from,
        }
    }
}

/// Unix milliseconds as RFC 3339 in UTC, e.g. `2026-10-24T08:15:30.250Z`.
pub(crate) fn rfc3339(millis: i64) -> String {
    let secs = millis.div_euclid(1000);
    let ms = millis.rem_euclid(1000);
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    // Civil date from days since 1970-01-01 (H. Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{ms:03}Z",
        tod / 3600,
        tod % 3600 / 60,
        tod % 60
    )
}

/// Writes the image of a `data:` URL to `path` (the viewer's "Save image", FR-1.56). The data is
/// already in the UI, so nothing is fetched again.
#[tauri::command]
#[specta::specta]
pub async fn save_image(path: String, data_url: String) -> ApiResult<()> {
    let bytes = limited_image_bytes(&data_url)?;
    std::fs::write(&path, bytes).map_err(|err| ApiError::new(ErrorCode::Io, err.to_string()))
}

/// Largest image "Copy image" and "Save image" take (issue #115): well above the largest
/// full-size image Mumcord users share, small enough that decoding stays quick.
pub(crate) const MAX_IMAGE_BYTES: usize = 50 * 1024 * 1024;

/// Width or height above which "Copy image" refuses to decode: the clipboard gets raw pixels.
const MAX_COPY_EDGE: u32 = 16_384;

/// Memory the decoder may use for one copied image.
const MAX_COPY_ALLOC: u64 = 512 * 1024 * 1024;

/// The bytes of an image `data:` URL, refused with `messageTooLarge` above `MAX_IMAGE_BYTES`.
fn limited_image_bytes(data_url: &str) -> Result<Vec<u8>, ApiError> {
    let too_large = || ApiError::new(ErrorCode::MessageTooLarge, "image too large");
    // Base64 needs 4 characters per 3 bytes; checked before decoding anything.
    if data_url.len() / 4 * 3 > MAX_IMAGE_BYTES + 3 {
        return Err(too_large());
    }
    let bytes = decode_image_data_url(data_url)?;
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(too_large());
    }
    Ok(bytes)
}

/// The pixels of an image `data:` URL for the clipboard. Clipboards rarely take animations, so
/// a GIF gives its first frame.
fn clipboard_pixels(data_url: &str) -> Result<tauri::image::Image<'static>, ApiError> {
    let bytes = limited_image_bytes(data_url)?;
    let invalid = || ApiError::new(ErrorCode::InvalidInput, "not a readable image");
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| invalid())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_COPY_EDGE);
    limits.max_image_height = Some(MAX_COPY_EDGE);
    limits.max_alloc = Some(MAX_COPY_ALLOC);
    reader.limits(limits);
    let decoded = reader.decode().map_err(|err| match err {
        image::ImageError::Limits(_) => {
            ApiError::new(ErrorCode::MessageTooLarge, "image too large")
        }
        _ => invalid(),
    })?;
    let rgba = decoded.into_rgba8();
    let (width, height) = rgba.dimensions();
    Ok(tauri::image::Image::new_owned(
        rgba.into_raw(),
        width,
        height,
    ))
}

/// Puts the image of a `data:` URL on the system clipboard as image data ("Copy image", issue
/// #115). Fails with `messageTooLarge` above the size limits, `invalidInput` when the data is not
/// a readable image and `io` when the clipboard does not take it.
#[tauri::command]
#[specta::specta]
pub async fn copy_image(app: tauri::AppHandle, data_url: String) -> ApiResult<()> {
    use tauri_plugin_clipboard_manager::ClipboardExt as _;
    let pixels = clipboard_pixels(&data_url).inspect_err(|e| {
        tracing::warn!(code = ?e.code, "copying an image failed");
    })?;
    app.clipboard().write_image(&pixels).map_err(|_| {
        tracing::warn!("the clipboard did not take the copied image");
        ApiError::new(ErrorCode::Io, "the clipboard did not take the image")
    })
}

/// Largest text `save_text` writes; matches the UI's limit for downloading a code block.
const MAX_TEXT_BYTES: usize = 1024 * 1024;

/// Writes text the UI already has to `path` (a chat code block's "Download", issue #111).
#[tauri::command]
#[specta::specta]
pub fn save_text(path: String, contents: String) -> ApiResult<()> {
    if contents.len() > MAX_TEXT_BYTES {
        return Err(ApiError::new(
            ErrorCode::InvalidInput,
            "text too large to save",
        ));
    }
    std::fs::write(&path, contents).map_err(|err| ApiError::new(ErrorCode::Io, err.to_string()))
}

/// Largest text `copy_text` puts on the clipboard: far more than a long chat selection.
const MAX_COPY_TEXT_BYTES: usize = 10 * 1024 * 1024;

/// Checks text for `copy_text`; split out so the limit is testable without a clipboard.
fn copyable_text(text: &str) -> Result<(), ApiError> {
    if text.len() > MAX_COPY_TEXT_BYTES {
        return Err(ApiError::new(
            ErrorCode::MessageTooLarge,
            "text too large to copy",
        ));
    }
    Ok(())
}

/// Puts plain text on the system clipboard (copying chat text, issue #114). The webview's own
/// clipboard access does not reach the desktop everywhere (WebKitGTK on Linux), so the copy
/// goes through the Tauri clipboard plugin instead. Fails with `messageTooLarge` above 10 MB and
/// `io` when the clipboard does not take the text.
#[tauri::command]
#[specta::specta]
pub async fn copy_text(app: tauri::AppHandle, text: String) -> ApiResult<()> {
    use tauri_plugin_clipboard_manager::ClipboardExt as _;
    copyable_text(&text)?;
    app.clipboard().write_text(text).map_err(|_| {
        tracing::warn!("the clipboard did not take the copied text");
        ApiError::new(ErrorCode::Io, "the clipboard did not take the text")
    })
}

/// The bytes of a base64 `data:image/...` URL; anything else is rejected.
pub(crate) fn decode_image_data_url(data_url: &str) -> Result<Vec<u8>, ApiError> {
    use base64::Engine as _;
    let invalid = || ApiError::new(ErrorCode::InvalidInput, "not an image data URL");
    let rest = data_url.strip_prefix("data:image/").ok_or_else(invalid)?;
    let (_, payload) = rest.split_once(";base64,").ok_or_else(invalid)?;
    base64::engine::general_purpose::STANDARD
        .decode(payload)
        .map_err(|_| invalid())
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_COPY_TEXT_BYTES, MAX_IMAGE_BYTES, MAX_TEXT_BYTES, clipboard_pixels, copyable_text,
        decode_image_data_url, limited_image_bytes, rfc3339, save_text,
    };
    use crate::commands::types::ErrorCode;
    use base64::Engine as _;

    #[test]
    fn formats_rfc3339() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(rfc3339(951_782_400_123), "2000-02-29T00:00:00.123Z");
        assert_eq!(rfc3339(1_792_829_730_250), "2026-10-24T08:15:30.250Z");
    }

    #[test]
    fn decodes_image_data_urls_only() {
        assert_eq!(
            decode_image_data_url("data:image/png;base64,AQID").expect("valid data URL"),
            vec![1, 2, 3]
        );
        assert!(decode_image_data_url("data:text/html;base64,AQID").is_err());
        assert!(decode_image_data_url("https://example.com/a.png").is_err());
        assert!(decode_image_data_url("data:image/png;base64,***").is_err());
    }

    #[test]
    fn saves_text_up_to_the_limit() {
        let dir = std::env::temp_dir().join(format!("mumcord-save-text-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("snippet.sql");
        let path_text = path.to_string_lossy().into_owned();
        save_text(path_text.clone(), "SELECT 1;\n".into()).expect("small text is saved");
        assert_eq!(
            std::fs::read_to_string(&path).expect("saved file"),
            "SELECT 1;\n"
        );
        assert!(save_text(path_text, "x".repeat(MAX_TEXT_BYTES + 1)).is_err());
        std::fs::remove_dir_all(&dir).expect("remove temp dir");
    }

    fn data_url(mime: &str, bytes: &[u8]) -> String {
        format!(
            "data:{mime};base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        )
    }

    fn encoded(image: &image::RgbaImage, format: image::ImageFormat) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        image.write_to(&mut out, format).expect("encode test image");
        out.into_inner()
    }

    #[test]
    fn copies_the_pixels_of_png_and_jpeg() {
        let png = image::RgbaImage::from_pixel(3, 2, image::Rgba([10, 20, 30, 255]));
        let pixels = clipboard_pixels(&data_url(
            "image/png",
            &encoded(&png, image::ImageFormat::Png),
        ))
        .expect("png decodes");
        assert_eq!((pixels.width(), pixels.height()), (3, 2));
        assert_eq!(&pixels.rgba()[..4], &[10, 20, 30, 255]);
        let rgb = image::DynamicImage::ImageRgba8(png).into_rgb8();
        let mut jpeg = std::io::Cursor::new(Vec::new());
        rgb.write_to(&mut jpeg, image::ImageFormat::Jpeg)
            .expect("encode jpeg");
        let pixels =
            clipboard_pixels(&data_url("image/jpeg", &jpeg.into_inner())).expect("jpeg decodes");
        assert_eq!((pixels.width(), pixels.height()), (3, 2));
    }

    #[test]
    fn copies_the_first_frame_of_a_gif() {
        use image::codecs::gif::GifEncoder;
        let red = image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]));
        let blue = image::RgbaImage::from_pixel(2, 2, image::Rgba([0, 0, 255, 255]));
        let mut gif = Vec::new();
        {
            let mut encoder = GifEncoder::new(&mut gif);
            encoder
                .encode_frames([image::Frame::new(red), image::Frame::new(blue)])
                .expect("encode gif");
        }
        let pixels = clipboard_pixels(&data_url("image/gif", &gif)).expect("gif decodes");
        assert_eq!(&pixels.rgba()[..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn refuses_unreadable_and_oversized_images() {
        let err = clipboard_pixels(&data_url("image/png", b"not a png")).expect_err("garbage");
        assert_eq!(err.code, ErrorCode::InvalidInput);
        let big = "A".repeat((MAX_IMAGE_BYTES + 6) / 3 * 4);
        let err =
            limited_image_bytes(&format!("data:image/png;base64,{big}")).expect_err("too large");
        assert_eq!(err.code, ErrorCode::MessageTooLarge);
        // A huge canvas in a tiny file must not make the decoder allocate gigabytes.
        let wide = image::RgbaImage::new(20_000, 1);
        let err = clipboard_pixels(&data_url(
            "image/png",
            &encoded(&wide, image::ImageFormat::Png),
        ))
        .expect_err("too wide");
        assert_eq!(err.code, ErrorCode::MessageTooLarge);
    }

    #[test]
    fn copies_text_up_to_the_limit() {
        assert!(copyable_text("hello\nworld").is_ok());
        assert!(copyable_text(&"x".repeat(MAX_COPY_TEXT_BYTES)).is_ok());
        assert!(copyable_text(&"x".repeat(MAX_COPY_TEXT_BYTES + 1)).is_err());
    }
}
