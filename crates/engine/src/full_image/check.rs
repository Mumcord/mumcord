//! Size limits and image checks for full images, on both ends of a transfer.
//!
//! The sender keeps the original when it is within [`MAX_BYTES`] and [`MAX_SIDE`]; otherwise it
//! scales it down and re-encodes it until it fits. The receiver accepts only bytes that match
//! the announced hash, are a PNG/JPEG/GIF/WebP by their content (never by a claimed type),
//! stay within the same limits and decode completely, before the webview ever sees them.

use std::io::Cursor;

use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ImageFormat, ImageReader};

use crate::web::fetch::ImageKind;
use crate::web::{decode_limits, image_format};

/// Largest full image in bytes. Plugin data moves about 2.7 KB/s, so this already takes about
/// three minutes; larger pictures are re-encoded to fit.
pub const MAX_BYTES: usize = 512 * 1024;
/// Largest side of a full image in pixels.
pub const MAX_SIDE: u32 = 3840;
/// Larger files are refused before decoding.
pub const SOURCE_MAX_BYTES: usize = 25 * 1024 * 1024;
const JPEG_QUALITY: u8 = 85;
/// Each re-encoding attempt shrinks the longest side to this fraction.
const SHRINK: f32 = 0.8;
const MIN_SIDE: u32 = 256;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CheckError {
    #[error("not a supported image")]
    NotImage,
    #[error("image too large")]
    TooLarge,
}

/// A checked image: its format by content and its size in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Checked {
    pub kind: ImageKind,
    pub width: u32,
    pub height: u32,
}

impl Checked {
    pub fn mime(&self) -> &'static str {
        match self.kind {
            ImageKind::Png => "image/png",
            ImageKind::Jpeg => "image/jpeg",
            ImageKind::Gif => "image/gif",
            ImageKind::Webp => "image/webp",
        }
    }
}

fn decode(bytes: &[u8], kind: ImageKind) -> Result<DynamicImage, CheckError> {
    let mut reader = ImageReader::with_format(Cursor::new(bytes), image_format(kind));
    reader.limits(decode_limits());
    reader.decode().map_err(|_| CheckError::NotImage)
}

/// Checks bytes received from another client (or about to be shared): format by content,
/// [`MAX_BYTES`], [`MAX_SIDE`] and a complete decode.
pub fn check(bytes: &[u8]) -> Result<Checked, CheckError> {
    if bytes.len() > MAX_BYTES {
        return Err(CheckError::TooLarge);
    }
    let kind = ImageKind::sniff(bytes).ok_or(CheckError::NotImage)?;
    let decoded = decode(bytes, kind)?;
    if decoded.width() > MAX_SIDE || decoded.height() > MAX_SIDE {
        return Err(CheckError::TooLarge);
    }
    Ok(Checked {
        kind,
        width: decoded.width(),
        height: decoded.height(),
    })
}

/// The full image to keep for a picture the user sends: the original when it passes
/// [`check`], else a scaled-down re-encoding (PNG when it has transparency, else JPEG) that
/// fits. Animated GIFs that are too large keep their first frame. CPU-heavy: call it off the
/// UI and session threads.
pub fn prepare(source: &[u8]) -> Result<Vec<u8>, CheckError> {
    if source.len() > SOURCE_MAX_BYTES {
        return Err(CheckError::TooLarge);
    }
    let kind = ImageKind::sniff(source).ok_or(CheckError::NotImage)?;
    if check(source).is_ok() {
        return Ok(source.to_vec());
    }
    let decoded = decode(source, kind)?;
    let mut side = MAX_SIDE.min(decoded.width().max(decoded.height()));
    loop {
        let scaled = if decoded.width() > side || decoded.height() > side {
            decoded.thumbnail(side, side)
        } else {
            decoded.clone()
        };
        let encoded = encode(&scaled)?;
        if encoded.len() <= MAX_BYTES {
            return Ok(encoded);
        }
        if side <= MIN_SIDE {
            return Err(CheckError::TooLarge);
        }
        let next = (side as f32 * SHRINK) as u32;
        side = next.max(MIN_SIDE);
    }
}

fn encode(image: &DynamicImage) -> Result<Vec<u8>, CheckError> {
    let mut out = Vec::new();
    let written = if image.color().has_alpha() {
        image.write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
    } else {
        image
            .to_rgb8()
            .write_with_encoder(JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY))
    };
    written.map_err(|_| CheckError::NotImage)?;
    Ok(out)
}

/// The image as a `data:` URL for the webview.
pub fn data_url(bytes: &[u8], checked: &Checked) -> String {
    format!(
        "data:{};base64,{}",
        checked.mime(),
        crate::people::base64(bytes)
    )
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use image::{Rgb, RgbImage, Rgba, RgbaImage};

    /// A noisy image (hard to compress) of the given size as PNG or JPEG.
    pub(crate) fn noisy(width: u32, height: u32, alpha: bool, format: ImageFormat) -> Vec<u8> {
        let mut seed = 0x1234_5678u32;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed.to_le_bytes()
        };
        let image = if alpha {
            DynamicImage::ImageRgba8(RgbaImage::from_fn(width, height, |_, _| {
                let [r, g, b, a] = next();
                Rgba([r, g, b, a])
            }))
        } else {
            DynamicImage::ImageRgb8(RgbImage::from_fn(width, height, |_, _| {
                let [r, g, b, _] = next();
                Rgb([r, g, b])
            }))
        };
        let mut out = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut out), format)
            .expect("encode");
        out
    }

    #[test]
    fn small_images_pass_and_keep_their_bytes() {
        let png = noisy(40, 30, true, ImageFormat::Png);
        let checked = check(&png).expect("valid");
        assert_eq!(
            checked,
            Checked {
                kind: ImageKind::Png,
                width: 40,
                height: 30
            }
        );
        assert_eq!(prepare(&png).expect("kept"), png);
        assert!(data_url(&png, &checked).starts_with("data:image/png;base64,iVBOR"));
    }

    #[test]
    fn broken_or_foreign_data_is_rejected() {
        let png = noisy(40, 30, false, ImageFormat::Png);
        assert_eq!(check(b"<svg onload=x()>"), Err(CheckError::NotImage));
        assert_eq!(check(&png[..png.len() / 2]), Err(CheckError::NotImage));
        assert_eq!(prepare(b"GIF89a broken"), Err(CheckError::NotImage));
        assert_eq!(check(&vec![0; MAX_BYTES + 1]), Err(CheckError::TooLarge));
    }

    #[test]
    fn oversized_sides_are_rejected_by_the_check() {
        let wide = noisy(MAX_SIDE + 1, 1, false, ImageFormat::Png);
        assert_eq!(check(&wide), Err(CheckError::TooLarge));
    }

    #[test]
    fn large_images_are_reencoded_to_fit() {
        let photo = noisy(1200, 900, false, ImageFormat::Png);
        assert!(photo.len() > MAX_BYTES, "the test image must be too large");
        let full = prepare(&photo).expect("fits after re-encoding");
        let checked = check(&full).expect("valid");
        assert_eq!(checked.kind, ImageKind::Jpeg);
        assert!(full.len() <= MAX_BYTES);
        let ratio = f64::from(checked.width) / f64::from(checked.height);
        assert!((ratio - 4.0 / 3.0).abs() < 0.01, "aspect kept: {ratio}");

        let transparent = noisy(800, 600, true, ImageFormat::Png);
        let full = prepare(&transparent).expect("fits after re-encoding");
        assert_eq!(check(&full).expect("valid").kind, ImageKind::Png);
    }

    #[test]
    fn huge_sources_are_refused_unread() {
        assert_eq!(
            prepare(&vec![0x89; SOURCE_MAX_BYTES + 1]),
            Err(CheckError::TooLarge)
        );
    }
}
