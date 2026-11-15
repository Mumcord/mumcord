//! What goes behind the person (FR-8.32): nothing, a blur, or an image (built-in or the user's
//! own, FR-8.14, FR-8.39).

use std::path::Path;
use std::sync::Arc;

use bgmatte::{Background, BackgroundImage};
use mumcord_store::{CameraBackground, CameraBackgrounds};

use crate::EffectsError;

/// File extensions tried for a built-in image id.
const BUILTIN_EXTENSIONS: [&str; 3] = ["jpg", "png", "webp"];

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
            image(&path)
        }
        CameraBackground::Own(id) => {
            let entry = own
                .get(id)
                .map_err(|err| EffectsError::Image(err.to_string()))?
                .ok_or(EffectsError::UnknownBackground)?;
            image(&entry.path)
        }
    }
}

fn image(path: &Path) -> Result<Background, EffectsError> {
    match BackgroundImage::open(path) {
        Ok(image) => Ok(Background::Image(Arc::new(image))),
        Err(bgmatte::Error::Image(reason)) => Err(EffectsError::Image(reason)),
        Err(err) => Err(EffectsError::Image(err.to_string())),
    }
}

fn is_plain_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use image::RgbImage;

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
    fn a_broken_image_file_is_an_image_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("broken.png"), b"not an image").expect("write");
        let own = CameraBackgrounds::new(dir.path());
        let setting = CameraBackground::Builtin("broken".to_owned());
        assert!(matches!(
            resolve(&setting, 50, dir.path(), &own),
            Err(EffectsError::Image(_))
        ));
    }
}
