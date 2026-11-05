//! What goes behind the person (FR-8.32): nothing, a blur, or an image (built-in or the user's
//! own, FR-8.14, FR-8.39).

use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;

use image::imageops::FilterType;
use image::{ImageReader, Limits, RgbImage};
use mumcord_store::{CameraBackground, CameraBackgrounds};

use crate::EffectsError;

/// Decoder limits: stored images are at most 1920 px wide, but treat files as untrusted anyway.
const MAX_DECODE_SIDE: u32 = 8192;
const MAX_DECODE_ALLOC_BYTES: u64 = 256 * 1024 * 1024;
/// File extensions tried for a built-in image id.
const BUILTIN_EXTENSIONS: [&str; 3] = ["jpg", "png", "webp"];

/// The background effect to apply.
#[derive(Debug, Clone)]
pub enum Background {
    /// No effect: the model is skipped and frames pass unchanged (FR-8.38).
    None,
    /// Blur with strength 0 (Light) … 100 (Strong).
    Blur {
        strength: u8,
    },
    Image(Arc<BackgroundImage>),
}

impl Background {
    pub fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }
}

/// A decoded background picture, kept at its own size and scaled to the camera frame when the
/// effect starts.
#[derive(Debug)]
pub struct BackgroundImage {
    rgb: RgbImage,
}

impl BackgroundImage {
    /// Decodes a PNG, JPEG or WebP image.
    pub fn decode(bytes: &[u8]) -> Result<Self, EffectsError> {
        let mut reader = ImageReader::new(Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|err| EffectsError::Image(err.to_string()))?;
        let mut limits = Limits::default();
        limits.max_image_width = Some(MAX_DECODE_SIDE);
        limits.max_image_height = Some(MAX_DECODE_SIDE);
        limits.max_alloc = Some(MAX_DECODE_ALLOC_BYTES);
        reader.limits(limits);
        let image = reader
            .decode()
            .map_err(|err| EffectsError::Image(err.to_string()))?;
        Ok(Self {
            rgb: image.into_rgb8(),
        })
    }

    /// Wraps an already decoded picture.
    pub fn from_rgb(rgb: RgbImage) -> Self {
        Self { rgb }
    }

    pub fn open(path: &Path) -> Result<Self, EffectsError> {
        let bytes = std::fs::read(path).map_err(|err| EffectsError::Image(err.to_string()))?;
        Self::decode(&bytes)
    }

    /// Scales and crops the image to cover `width × height` (no letterboxing).
    pub(crate) fn cover(&self, width: u32, height: u32) -> RgbImage {
        let (sw, sh) = self.rgb.dimensions();
        let scale = (width as f32 / sw as f32).max(height as f32 / sh as f32);
        let cw = ((width as f32 / scale).round() as u32).clamp(1, sw);
        let ch = ((height as f32 / scale).round() as u32).clamp(1, sh);
        let cropped =
            image::imageops::crop_imm(&self.rgb, (sw - cw) / 2, (sh - ch) / 2, cw, ch).to_image();
        image::imageops::resize(&cropped, width, height, FilterType::Triangle)
    }
}

/// Turns the stored setting into an effect. Built-in images are looked up as
/// `{builtin_dir}/{id}.jpg|png|webp`; own images through the store (FR-8.14). The id is checked
/// so a crafted settings value can't point outside the folder.
pub fn resolve(
    setting: &CameraBackground,
    blur_strength: u8,
    builtin_dir: &Path,
    own: &CameraBackgrounds,
) -> Result<Background, EffectsError> {
    match setting {
        CameraBackground::None => Ok(Background::None),
        CameraBackground::Blur => Ok(Background::Blur {
            strength: blur_strength.min(100),
        }),
        CameraBackground::Builtin(id) => {
            if !is_plain_id(id) {
                return Err(EffectsError::UnknownBackground);
            }
            let path = BUILTIN_EXTENSIONS
                .iter()
                .map(|ext| builtin_dir.join(format!("{id}.{ext}")))
                .find(|path| path.is_file())
                .ok_or(EffectsError::UnknownBackground)?;
            Ok(Background::Image(Arc::new(BackgroundImage::open(&path)?)))
        }
        CameraBackground::Own(id) => {
            let entry = own
                .get(id)
                .map_err(|err| EffectsError::Image(err.to_string()))?
                .ok_or(EffectsError::UnknownBackground)?;
            Ok(Background::Image(Arc::new(BackgroundImage::open(
                &entry.path,
            )?)))
        }
    }
}

fn is_plain_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// RGB → BT.601 limited-range YUV, the colour space webcams and the encoders use.
pub(crate) fn rgb_to_yuv(r: u8, g: u8, b: u8) -> (u8, u8, u8) {
    let (r, g, b) = (f32::from(r), f32::from(g), f32::from(b));
    let y = 16.0 + (65.481 * r + 128.553 * g + 24.966 * b) / 255.0;
    let u = 128.0 + (-37.797 * r - 74.203 * g + 112.0 * b) / 255.0;
    let v = 128.0 + (112.0 * r - 93.786 * g - 18.214 * b) / 255.0;
    (
        y.round().clamp(0.0, 255.0) as u8,
        u.round().clamp(0.0, 255.0) as u8,
        v.round().clamp(0.0, 255.0) as u8,
    )
}

/// Converts an RGB picture of the frame size into NV12 (`y` then interleaved `uv`).
pub(crate) fn rgb_to_nv12(rgb: &RgbImage, nv12: &mut [u8]) {
    let (w, h) = (rgb.width() as usize, rgb.height() as usize);
    let (y_plane, uv_plane) = nv12.split_at_mut(w * h);
    for (i, pixel) in rgb.pixels().enumerate() {
        y_plane[i] = rgb_to_yuv(pixel[0], pixel[1], pixel[2]).0;
    }
    for cy in 0..h / 2 {
        for cx in 0..w / 2 {
            let (mut su, mut sv) = (0u32, 0u32);
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let p = rgb.get_pixel((cx * 2 + dx) as u32, (cy * 2 + dy) as u32);
                let (_, u, v) = rgb_to_yuv(p[0], p[1], p[2]);
                su += u32::from(u);
                sv += u32::from(v);
            }
            uv_plane[cy * w + cx * 2] = ((su + 2) / 4) as u8;
            uv_plane[cy * w + cx * 2 + 1] = ((sv + 2) / 4) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32, colour: [u8; 3]) -> Vec<u8> {
        let image = RgbImage::from_pixel(width, height, image::Rgb(colour));
        let mut bytes = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
            .expect("encode png");
        bytes
    }

    #[test]
    fn cover_fills_the_frame_without_bars() {
        let image = BackgroundImage::decode(&png(400, 400, [10, 200, 30])).expect("decode");
        let covered = image.cover(64, 36);
        assert_eq!(covered.dimensions(), (64, 36));
        assert!(covered.pixels().all(|p| p.0 == [10, 200, 30]));
    }

    #[test]
    fn rejects_garbage() {
        assert!(BackgroundImage::decode(b"not an image").is_err());
    }

    #[test]
    fn resolves_settings() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("forest.png"), png(8, 8, [0, 0, 0])).expect("write");
        let own = CameraBackgrounds::new(dir.path());
        let builtin = |id: &str| CameraBackground::Builtin(id.to_owned());
        assert!(matches!(
            resolve(&CameraBackground::None, 50, dir.path(), &own),
            Ok(Background::None)
        ));
        assert!(matches!(
            resolve(&CameraBackground::Blur, 200, dir.path(), &own),
            Ok(Background::Blur { strength: 100 })
        ));
        assert!(matches!(
            resolve(&builtin("forest"), 50, dir.path(), &own),
            Ok(Background::Image(_))
        ));
        assert!(matches!(
            resolve(&builtin("../forest"), 50, dir.path(), &own),
            Err(EffectsError::UnknownBackground)
        ));
        assert!(matches!(
            resolve(&builtin("missing"), 50, dir.path(), &own),
            Err(EffectsError::UnknownBackground)
        ));
        let added = own.add(&png(64, 64, [255, 0, 0])).expect("add own image");
        assert!(matches!(
            resolve(&CameraBackground::Own(added.id), 50, dir.path(), &own),
            Ok(Background::Image(_))
        ));
    }

    #[test]
    fn converts_white_and_black_to_limited_range() {
        assert_eq!(rgb_to_yuv(255, 255, 255), (235, 128, 128));
        assert_eq!(rgb_to_yuv(0, 0, 0), (16, 128, 128));
    }
}
