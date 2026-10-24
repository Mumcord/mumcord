//! Own avatar, default avatar, comment and self-registration (FR-1.20, FR-1.34, FR-1.47).

use std::io::Cursor;
use std::path::{Path, PathBuf};

use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ImageFormat};
use mumcord_protocol::{DenyKind, PermissionDenied, permissions};

use crate::chat::{IMAGE_SOFT_CAP, check_text_size};
use crate::follow::{OnTimeout, confirm, subscribe_denials};
use crate::people::avatar_data_url;
use crate::session::convert::{ROOT_CHANNEL, command_error};
use crate::session::{Engine, EngineError, EngineEvent, EngineResult, ErrorKind, SessionCtx};

/// File name of the default avatar inside the app data directory (FR-1.47).
pub const DEFAULT_AVATAR_FILE: &str = "default_avatar.png";

/// Largest avatar edge in pixels (what Mumble clients display).
const AVATAR_SIZE: u32 = 128;
/// Smallest edge tried before giving up on a too-small server limit.
const MIN_AVATAR_SIZE: u32 = 32;
const JPEG_QUALITIES: [u8; 3] = [85, 70, 50];

/// Decodes an image and re-encodes it to fit `AVATAR_SIZE` and `limit` bytes: PNG first,
/// then JPEG with falling quality, then smaller sizes.
fn encode_avatar(bytes: &[u8], limit: usize) -> EngineResult<Vec<u8>> {
    let image = image::load_from_memory(bytes)
        .map_err(|_| EngineError::invalid("not a supported image (PNG, JPEG, GIF or WebP)"))?;
    let mut size = AVATAR_SIZE;
    loop {
        let scaled = if image.width() > size || image.height() > size {
            image.thumbnail(size, size)
        } else {
            image.clone()
        };
        if let Some(png) = encode_png(&scaled).filter(|b| b.len() <= limit) {
            return Ok(png);
        }
        let rgb = DynamicImage::ImageRgb8(scaled.to_rgb8());
        for quality in JPEG_QUALITIES {
            let mut out = Vec::new();
            if rgb
                .write_with_encoder(JpegEncoder::new_with_quality(&mut out, quality))
                .is_ok()
                && out.len() <= limit
            {
                return Ok(out);
            }
        }
        if size <= MIN_AVATAR_SIZE {
            return Err(EngineError::new(
                ErrorKind::MessageTooLarge,
                format!("the avatar does not fit the server limit of {limit} bytes"),
            ));
        }
        size /= 2;
    }
}

fn encode_png(image: &DynamicImage) -> Option<Vec<u8>> {
    let mut out = Cursor::new(Vec::new());
    image.write_to(&mut out, ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

/// Denials that answer a change of the own user state.
fn is_profile_denial(denied: &PermissionDenied, own: Option<u32>) -> bool {
    match denied.kind {
        DenyKind::TextTooLong | DenyKind::Text | DenyKind::MissingCertificate => true,
        DenyKind::Permission | DenyKind::UserName => {
            denied.session.is_none() || denied.session == own
        }
        _ => false,
    }
}

/// The server's image size limit in bytes.
fn image_limit(ctx: &SessionCtx) -> usize {
    let limit = match ctx.handle.with_state(|s| s.server.image_message_length) {
        Some(0) | None => IMAGE_SOFT_CAP,
        Some(n) => n,
    };
    limit as usize
}

/// Reads an image file off the async runtime and encodes it twice with `encode_avatar`:
/// for the server's `limit`, and as an unlimited PNG to remember for later connects.
async fn encode_avatar_file(path: PathBuf, limit: usize) -> EngineResult<(Vec<u8>, Vec<u8>)> {
    tokio::task::spawn_blocking(move || {
        let bytes = std::fs::read(&path).map_err(|e| {
            EngineError::new(ErrorKind::Io, format!("reading the image failed: {e}"))
        })?;
        Ok((
            encode_avatar(&bytes, limit)?,
            encode_avatar(&bytes, usize::MAX)?,
        ))
    })
    .await
    .map_err(|e| EngineError::internal(e.to_string()))?
}

/// Sends `texture` as the own avatar (empty removes it) and waits for the server's answer.
async fn apply_texture(ctx: &SessionCtx, texture: Vec<u8>) -> EngineResult<()> {
    let (own, before) = ctx
        .handle
        .with_state(|s| (s.session, s.own_user().and_then(|u| u.texture_hash.clone())));
    let denials = subscribe_denials();
    ctx.handle
        .set_texture(texture.clone())
        .map_err(command_error)?;
    confirm(
        ctx,
        denials,
        |s| {
            s.own_user().is_some_and(|u| {
                if texture.is_empty() {
                    u.texture.is_none() && u.texture_hash.is_none()
                } else {
                    u.texture.as_ref() == Some(&texture)
                        || (u.texture_hash.is_some() && u.texture_hash != before)
                }
            })
        },
        |d| is_profile_denial(d, own),
        OnTimeout::Accept,
    )
    .await
}

/// What to do with the own avatar after a sync (FR-1.20, FR-1.47, QA.10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AvatarStep {
    /// Nothing to do: the server already shows an avatar, or nothing is to be sent.
    Nothing,
    /// The default avatar is not needed (setting off, no default avatar, or the user has an
    /// avatar there); the server still counts as seen so turning the setting on later only
    /// affects new servers.
    MarkOnly,
    /// First connect to this server: send the default avatar once.
    SendDefault,
    /// The server keeps no avatar for the user (unregistered): send the remembered one again.
    Resend,
}

fn avatar_step(
    default_handled: bool,
    enabled: bool,
    has_default: bool,
    remembered: bool,
    has_avatar: bool,
) -> AvatarStep {
    if has_avatar {
        if default_handled {
            AvatarStep::Nothing
        } else {
            AvatarStep::MarkOnly
        }
    } else if remembered {
        AvatarStep::Resend
    } else if default_handled {
        AvatarStep::Nothing
    } else if enabled && has_default {
        AvatarStep::SendDefault
    } else {
        AvatarStep::MarkOnly
    }
}

fn mark_default_avatar_sent(ctx: &SessionCtx) {
    if let Err(e) = ctx.store().set_default_avatar_sent(ctx.server_id) {
        tracing::warn!(error = %e, "remembering the default avatar failed");
    }
}

fn remember_avatar(ctx: &SessionCtx, avatar: Option<&[u8]>) {
    if let Err(e) = ctx.store().set_server_avatar(ctx.server_id, avatar) {
        tracing::warn!(error = %e, "remembering the avatar for this server failed");
    }
}

/// Encodes stored image bytes for the server's limit and sends them as the own avatar.
async fn send_avatar_bytes(ctx: &SessionCtx, bytes: Vec<u8>) -> EngineResult<()> {
    let limit = image_limit(ctx);
    let texture = tokio::task::spawn_blocking(move || encode_avatar(&bytes, limit))
        .await
        .map_err(|e| EngineError::internal(e.to_string()))??;
    apply_texture(ctx, texture).await
}

/// Makes sure the server shows the avatar Mumcord remembers for it. Called in the
/// background after every sync (fresh connect and reconnect): the default avatar is sent
/// once per saved server (FR-1.47); afterwards the remembered avatar is sent again whenever
/// the server shows none, because servers keep avatars only for registered users (QA.10).
pub(crate) async fn restore_avatar(ctx: SessionCtx, default_file: PathBuf) {
    let state = {
        let store = ctx.store();
        store.default_avatar_sent(ctx.server_id).and_then(|sent| {
            Ok((
                sent,
                store.settings()?.send_default_avatar,
                store.server_avatar(ctx.server_id)?,
            ))
        })
    };
    let (default_handled, enabled, remembered) = match state {
        Ok(state) => state,
        Err(e) => {
            tracing::warn!(error = %e, "reading the avatar state failed");
            return;
        }
    };
    let has_avatar = ctx.handle.with_state(|s| {
        s.own_user().is_some_and(|u| {
            u.texture_hash.is_some() || u.texture.as_ref().is_some_and(|t| !t.is_empty())
        })
    });
    let default = if default_handled || has_avatar {
        None
    } else {
        match std::fs::read(&default_file) {
            Ok(bytes) => Some(bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                tracing::warn!(error = %e, "reading the default avatar failed");
                return;
            }
        }
    };
    let step = avatar_step(
        default_handled,
        enabled,
        default.is_some(),
        remembered.is_some(),
        has_avatar,
    );
    match (step, default, remembered) {
        (AvatarStep::MarkOnly, ..) => mark_default_avatar_sent(&ctx),
        (AvatarStep::SendDefault, Some(bytes), _) => {
            let result = send_avatar_bytes(&ctx, bytes.clone()).await;
            match &result {
                Ok(()) => {
                    tracing::info!(server_id = ctx.server_id, "default avatar sent");
                    remember_avatar(&ctx, Some(&bytes));
                }
                Err(e) if e.kind == ErrorKind::MessageTooLarge => {
                    tracing::info!(server_id = ctx.server_id, "default avatar too large");
                    ctx.events.emit(EngineEvent::DefaultAvatarSkipped {
                        server_id: ctx.server_id,
                    });
                    mark_default_avatar_sent(&ctx);
                }
                // Connection lost, denied, …: tried again on the next connect.
                Err(e) => tracing::warn!(error = %e, "sending the default avatar failed"),
            }
        }
        (AvatarStep::Resend, _, Some(bytes)) => match send_avatar_bytes(&ctx, bytes).await {
            Ok(()) => tracing::info!(server_id = ctx.server_id, "remembered avatar sent again"),
            Err(e) => tracing::warn!(error = %e, "sending the remembered avatar failed"),
        },
        _ => {}
    }
}

/// Lets servers that got the default avatar before avatars were remembered per server
/// remember it now, so it is re-sent there too (QA.10). Failing only costs that re-send.
pub(crate) fn backfill_server_avatars(store: &mumcord_store::Store, data_dir: &Path) {
    let default = match std::fs::read(data_dir.join(DEFAULT_AVATAR_FILE)) {
        Ok(bytes) => Some(bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            tracing::warn!(error = %e, "reading the default avatar for the backfill failed");
            return;
        }
    };
    match store.backfill_server_avatars(default.as_deref()) {
        Ok(0) => {}
        Ok(n) => tracing::info!(
            servers = n,
            "remembered the default avatar for older servers"
        ),
        Err(e) => tracing::warn!(error = %e, "remembering the default avatar failed"),
    }
}

/// Reads the stored default avatar as a `data:` URL; `None` if none is set.
fn read_default_avatar(file: &Path) -> EngineResult<Option<String>> {
    match std::fs::read(file) {
        Ok(bytes) => Ok(avatar_data_url(&bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(EngineError::new(
            ErrorKind::Io,
            format!("reading the default avatar failed: {e}"),
        )),
    }
}

/// Stores `source` scaled to a PNG of at most `AVATAR_SIZE` pixels as `file`; `None` removes it.
fn write_default_avatar(file: &Path, source: Option<&Path>) -> EngineResult<Option<String>> {
    let io = |what: &str, e: std::io::Error| {
        EngineError::new(
            ErrorKind::Io,
            format!("{what} the default avatar failed: {e}"),
        )
    };
    let Some(source) = source else {
        return match std::fs::remove_file(file) {
            Ok(()) => Ok(None),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(io("removing", e)),
        };
    };
    let bytes = std::fs::read(source)
        .map_err(|e| EngineError::new(ErrorKind::Io, format!("reading the image failed: {e}")))?;
    // Without a size limit `encode_avatar` always settles on a PNG.
    let png = encode_avatar(&bytes, usize::MAX)?;
    // Write next to the target and rename, so a crash never leaves half an image behind.
    let partial = file.with_extension("png.partial");
    std::fs::write(&partial, &png).map_err(|e| io("saving", e))?;
    std::fs::rename(&partial, file).map_err(|e| io("saving", e))?;
    Ok(avatar_data_url(&png))
}

impl Engine {
    fn default_avatar_file(&self) -> PathBuf {
        self.data_dir().join(DEFAULT_AVATAR_FILE)
    }

    /// The default avatar as a `data:` URL, or `None` if none is set (FR-1.47).
    pub fn default_avatar_get(&self) -> EngineResult<Option<String>> {
        read_default_avatar(&self.default_avatar_file())
    }

    /// Sets the default avatar from an image file (scaled to a 128×128 PNG) or removes it
    /// with `None`; returns the stored image as a `data:` URL. Avatars already on servers
    /// stay untouched.
    pub async fn default_avatar_set(&self, path: Option<PathBuf>) -> EngineResult<Option<String>> {
        let file = self.default_avatar_file();
        tokio::task::spawn_blocking(move || write_default_avatar(&file, path.as_deref()))
            .await
            .map_err(|e| EngineError::internal(e.to_string()))?
    }

    /// Sets the own avatar from an image file, scaled to the server limit; `None` removes it.
    /// The choice is remembered for the saved server and restored on later connects when the
    /// server does not keep it (FR-1.20, QA.10).
    pub async fn set_own_avatar(&self, path: Option<PathBuf>) -> EngineResult<()> {
        let ctx = self.session_ctx()?;
        let (texture, keep) = match path {
            None => (Vec::new(), None),
            Some(path) => {
                let (texture, keep) = encode_avatar_file(path, image_limit(&ctx)).await?;
                (texture, Some(keep))
            }
        };
        apply_texture(&ctx, texture).await?;
        remember_avatar(&ctx, keep.as_deref());
        Ok(())
    }

    /// Sets the own profile comment (Mumble HTML); empty removes it.
    pub async fn set_own_comment(&self, html: String) -> EngineResult<()> {
        let ctx = self.session_ctx()?;
        // The now-playing line stays at the end while it is shown (FR-1.58).
        let html = self.with_now_playing_line(&html);
        if !html.trim().is_empty() {
            check_text_size(&ctx, &html)?;
        }
        let (own, before) = ctx
            .handle
            .with_state(|s| (s.session, s.own_user().and_then(|u| u.comment_hash.clone())));
        let denials = subscribe_denials();
        ctx.handle
            .set_comment(html.clone())
            .map_err(command_error)?;
        confirm(
            &ctx,
            denials,
            |s| {
                s.own_user().is_some_and(|u| {
                    if html.is_empty() {
                        u.comment.is_none()
                    } else {
                        u.comment.as_deref() == Some(html.as_str())
                            || (u.comment_hash.is_some() && u.comment_hash != before)
                    }
                })
            },
            |d| is_profile_denial(d, own),
            OnTimeout::Accept,
        )
        .await
    }

    /// Registers the current name with the current identity on the connected server.
    pub async fn register_self(&self) -> EngineResult<()> {
        let ctx = self.session_ctx()?;
        let (own, registered, has_cert, root_bits) = ctx.handle.with_state(|s| {
            let user = s.own_user();
            (
                s.session,
                user.is_some_and(|u| u.user_id.is_some()),
                user.is_some_and(|u| u.cert_hash.is_some()),
                s.permissions.get(&ROOT_CHANNEL).copied(),
            )
        });
        if registered {
            return Ok(());
        }
        let allowed =
            root_bits.is_none_or(|b| b & (permissions::SELF_REGISTER | permissions::REGISTER) != 0);
        if !has_cert || !allowed {
            return Err(EngineError::new(
                ErrorKind::PermissionDenied,
                "registering is not allowed on this server",
            ));
        }
        let denials = subscribe_denials();
        ctx.handle.register_self().map_err(command_error)?;
        confirm(
            &ctx,
            denials,
            |s| s.own_user().is_some_and(|u| u.user_id.is_some()),
            |d| is_profile_denial(d, own),
            OnTimeout::Fail("the registration"),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use image::{Rgba, RgbaImage};

    use super::*;

    fn noisy_png(w: u32, h: u32) -> Vec<u8> {
        let mut seed = 12345u32;
        let img = RgbaImage::from_fn(w, h, |_, _| {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            let [a, b, c, _] = seed.to_le_bytes();
            Rgba([a, b, c, 255])
        });
        encode_png(&DynamicImage::ImageRgba8(img)).expect("png")
    }

    #[test]
    fn avatar_is_scaled_to_128_and_kept_as_png_when_small() {
        let out = encode_avatar(&noisy_png(400, 200), 1 << 20).expect("encode");
        assert!(out.starts_with(b"\x89PNG"));
        let decoded = image::load_from_memory(&out).expect("decode");
        assert_eq!((decoded.width(), decoded.height()), (128, 64));
    }

    #[test]
    fn avatar_falls_back_to_jpeg_and_smaller_sizes_under_a_tight_limit() {
        let out = encode_avatar(&noisy_png(300, 300), 6_000).expect("encode");
        assert!(out.len() <= 6_000);
        assert!(out.starts_with(&[0xFF, 0xD8, 0xFF]), "jpeg");
    }

    #[test]
    fn default_avatar_is_handled_once_per_server() {
        use AvatarStep::{MarkOnly, Nothing, SendDefault};
        // First connect, setting on, default set, nothing remembered, no avatar there yet.
        assert_eq!(avatar_step(false, true, true, false, false), SendDefault);
        // Already has an avatar there: never send, but remember the server.
        assert_eq!(avatar_step(false, true, true, false, true), MarkOnly);
        // Setting off or no default avatar: the server still counts as seen.
        assert_eq!(avatar_step(false, false, true, false, false), MarkOnly);
        assert_eq!(avatar_step(false, true, false, false, false), MarkOnly);
        // Once handled and "no avatar" remembered (removed by hand): never again.
        for (enabled, has_default, has_avatar) in [
            (true, true, false),
            (true, true, true),
            (false, false, false),
        ] {
            assert_eq!(
                avatar_step(true, enabled, has_default, false, has_avatar),
                Nothing
            );
        }
    }

    #[test]
    fn remembered_avatar_is_resent_only_when_the_server_shows_none() {
        use AvatarStep::{Nothing, Resend};
        // Unregistered: the server forgot the avatar on disconnect.
        for (enabled, has_default) in [(true, true), (false, false), (true, false)] {
            assert_eq!(avatar_step(true, enabled, has_default, true, false), Resend);
        }
        // Registered (or already restored): the server's copy is kept, nothing is sent.
        for (enabled, has_default) in [(true, true), (false, false)] {
            assert_eq!(avatar_step(true, enabled, has_default, true, true), Nothing);
        }
    }

    #[test]
    fn default_avatar_is_stored_as_scaled_png_and_cleared() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join(DEFAULT_AVATAR_FILE);
        let source = dir.path().join("source.png");
        std::fs::write(&source, noisy_png(300, 600)).expect("write source");

        assert_eq!(read_default_avatar(&file).expect("empty"), None);
        let url = write_default_avatar(&file, Some(&source)).expect("set");
        assert!(url.is_some_and(|u| u.starts_with("data:image/png;base64,")));
        let stored = image::open(&file).expect("stored image");
        assert_eq!((stored.width(), stored.height()), (64, 128));
        assert!(read_default_avatar(&file).expect("read").is_some());

        assert_eq!(write_default_avatar(&file, None).expect("clear"), None);
        assert!(!file.exists());
        assert_eq!(
            write_default_avatar(&file, None).expect("clear twice"),
            None
        );

        std::fs::write(&source, b"not an image").expect("write source");
        assert_eq!(
            write_default_avatar(&file, Some(&source))
                .expect_err("invalid")
                .kind,
            ErrorKind::InvalidInput
        );
        assert!(!file.exists(), "a failed set keeps the previous state");
    }

    #[test]
    fn avatar_errors_are_plain() {
        assert_eq!(
            encode_avatar(b"not an image", 1000)
                .expect_err("invalid")
                .kind,
            ErrorKind::InvalidInput
        );
        assert_eq!(
            encode_avatar(&noisy_png(300, 300), 100)
                .expect_err("too small")
                .kind,
            ErrorKind::MessageTooLarge
        );
    }
}
