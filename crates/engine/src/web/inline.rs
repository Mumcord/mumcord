//! External images in messages and direct image links, shown inline (FR-1.55, QA.35).
//!
//! The engine fetches the image through the guarded [`Fetcher`] (public addresses only, also
//! after redirects; size limit) and hands it to the UI as a `data:` URL, so the webview never
//! contacts a remote origin. The format is sniffed from the bytes. Still images are decoded
//! and re-encoded at a display size; animated GIF and WebP pass through unchanged (after
//! their header has been checked) so they keep moving.

use std::io::Cursor;

use image::codecs::gif::GifDecoder;
use image::codecs::webp::WebPDecoder;
use image::{AnimationDecoder, ImageDecoder, ImageReader};
use reqwest::Url;
use serde::{Deserialize, Serialize};

use super::fetch::{FetchError, FetchedImage, Fetcher, ImageKind};

/// Larger images are refused; animated GIFs are often several megabytes.
pub const INLINE_IMAGE_LIMIT: usize = 8 * 1024 * 1024;
/// Animations reach the webview as they are, so it decodes every frame: keep them modest.
pub const ANIMATED_MAX_SIDE: u32 = 2048;
/// Still images are re-encoded to at most this many pixels per side: twice the 400×300 box
/// the chat shows them in, so they stay sharp on high-density screens.
pub const STILL_MAX_SIDE: u32 = 800;

/// An image ready to show inline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InlineImage {
    /// The image link as posted (without fragment).
    pub url: String,
    /// The image as a `data:` URL.
    pub data_url: String,
    /// Size of the delivered image in pixels, so the UI can reserve its space.
    pub width: u32,
    pub height: u32,
    /// Animated GIF or WebP, delivered unchanged.
    pub animated: bool,
}

/// Fetches and prepares the image at `url`.
pub(super) async fn load(fetcher: &Fetcher, url: &Url) -> Result<InlineImage, FetchError> {
    let image = fetcher
        .image_up_to(url.as_str(), INLINE_IMAGE_LIMIT)
        .await?;
    prepare(url.as_str(), &image)
}

/// Checks the image and turns it into what the chat shows: animations unchanged, still images
/// re-encoded at display size.
pub fn prepare(url: &str, image: &FetchedImage) -> Result<InlineImage, FetchError> {
    // Only the header is read here, so no limits are needed yet.
    let (width, height) =
        ImageReader::with_format(Cursor::new(&image.bytes), super::image_format(image.kind))
            .into_dimensions()
            .map_err(|_| FetchError::UnsupportedType)?;
    if width == 0 || height == 0 {
        return Err(FetchError::UnsupportedType);
    }
    if width > super::DECODE_MAX_SIDE || height > super::DECODE_MAX_SIDE {
        return Err(FetchError::TooLarge);
    }
    if is_animated(image)? {
        if width > ANIMATED_MAX_SIDE || height > ANIMATED_MAX_SIDE {
            return Err(FetchError::TooLarge);
        }
        let mime = match image.kind {
            ImageKind::Gif => "image/gif",
            _ => "image/webp",
        };
        return Ok(InlineImage {
            url: url.to_owned(),
            data_url: format!("data:{mime};base64,{}", crate::people::base64(&image.bytes)),
            width,
            height,
            animated: true,
        });
    }
    let still = super::reencode(image, STILL_MAX_SIDE).ok_or(FetchError::UnsupportedType)?;
    Ok(InlineImage {
        url: url.to_owned(),
        data_url: still.data_url,
        width: still.width,
        height: still.height,
        animated: false,
    })
}

/// Whether the image has more than one frame. A GIF counts as animated once a second frame
/// decodes; WebP declares animation in its header.
fn is_animated(image: &FetchedImage) -> Result<bool, FetchError> {
    match image.kind {
        ImageKind::Gif => {
            let mut decoder = GifDecoder::new(Cursor::new(&image.bytes))
                .map_err(|_| FetchError::UnsupportedType)?;
            decoder
                .set_limits(super::decode_limits())
                .map_err(|_| FetchError::TooLarge)?;
            let frames = decoder
                .into_frames()
                .take(2)
                .take_while(Result::is_ok)
                .count();
            if frames == 0 {
                return Err(FetchError::UnsupportedType);
            }
            Ok(frames > 1)
        }
        ImageKind::Webp => WebPDecoder::new(Cursor::new(&image.bytes))
            .map(|d| d.has_animation())
            .map_err(|_| FetchError::UnsupportedType),
        ImageKind::Png | ImageKind::Jpeg => Ok(false),
    }
}
