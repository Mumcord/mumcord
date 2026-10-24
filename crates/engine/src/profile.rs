//! Own avatar, comment and self-registration (FR-1.20, FR-1.34).

use std::io::Cursor;
use std::path::PathBuf;

use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ImageFormat};
use mumcord_protocol::{DenyKind, PermissionDenied, permissions};

use crate::chat::{IMAGE_SOFT_CAP, check_text_size};
use crate::follow::{OnTimeout, confirm, subscribe_denials};
use crate::session::convert::{ROOT_CHANNEL, command_error};
use crate::session::{Engine, EngineError, EngineResult, ErrorKind};

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

impl Engine {
    /// Sets the own avatar from an image file, scaled to the server limit; `None` removes it.
    pub async fn set_own_avatar(&self, path: Option<PathBuf>) -> EngineResult<()> {
        let ctx = self.session_ctx()?;
        let texture = match path {
            None => Vec::new(),
            Some(path) => {
                let limit = match ctx.handle.with_state(|s| s.server.image_message_length) {
                    Some(0) | None => IMAGE_SOFT_CAP,
                    Some(n) => n,
                } as usize;
                tokio::task::spawn_blocking(move || {
                    let bytes = std::fs::read(&path).map_err(|e| {
                        EngineError::new(ErrorKind::Io, format!("reading the image failed: {e}"))
                    })?;
                    encode_avatar(&bytes, limit)
                })
                .await
                .map_err(|e| EngineError::internal(e.to_string()))??
            }
        };
        let (own, before) = ctx
            .handle
            .with_state(|s| (s.session, s.own_user().and_then(|u| u.texture_hash.clone())));
        let denials = subscribe_denials();
        ctx.handle
            .set_texture(texture.clone())
            .map_err(command_error)?;
        confirm(
            &ctx,
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

    /// Sets the own profile comment (Mumble HTML); empty removes it.
    pub async fn set_own_comment(&self, html: String) -> EngineResult<()> {
        let ctx = self.session_ctx()?;
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
