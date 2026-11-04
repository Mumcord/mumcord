//! The user's own camera background images (FR-8.14, FR-8.15).
//!
//! Images are untrusted input. They are size-checked, decoded with allocation limits, scaled
//! and re-encoded, which drops every piece of metadata (EXIF, GPS, ICC, text chunks). Only
//! the re-encoded copy is kept; the original path is never stored. There is no database
//! table: the folder `{data}/backgrounds/` is the index, and the file name is the id.

use std::fs;
use std::io::{BufReader, Cursor};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};

use crate::db::{Store, UnixMillis};
use crate::error::{Error, Result};
use crate::settings::CameraBackground;

/// Largest accepted source file (FR-8.14).
pub const MAX_BACKGROUND_FILE_BYTES: u64 = 10 * 1024 * 1024;
/// Stored images are scaled down to at most this many pixels on each side (FR-8.14).
pub const MAX_BACKGROUND_PX: u32 = 1920;
/// Decoder limits against decompression bombs: a small file can claim a huge picture.
const MAX_DECODE_SIDE: u32 = 16_384;
const MAX_DECODE_ALLOC_BYTES: u64 = 400 * 1024 * 1024;
const JPEG_QUALITY: u8 = 90;
const FOLDER: &str = "backgrounds";

/// One stored own background image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnBackground {
    /// Random UUID; also the file stem. Used as `own:{id}` in the settings.
    pub id: String,
    pub path: PathBuf,
    /// When the file was added (file modification time), for the list order.
    pub added_at: UnixMillis,
}

/// Access to the own-background folder of one profile.
#[derive(Debug, Clone)]
pub struct CameraBackgrounds {
    dir: PathBuf,
}

impl CameraBackgrounds {
    /// `data_dir` is the profile's data folder (the one holding the database). The
    /// `backgrounds` subfolder is created on first use.
    pub fn new(data_dir: &Path) -> Self {
        Self {
            dir: data_dir.join(FOLDER),
        }
    }

    /// Adds an image from memory; returns the stored entry.
    pub fn add(&self, bytes: &[u8]) -> Result<OwnBackground> {
        if bytes.len() as u64 > MAX_BACKGROUND_FILE_BYTES {
            return Err(Error::InvalidBackgroundImage);
        }
        let (encoded, extension) = reencode(bytes)?;
        fs::create_dir_all(&self.dir)?;
        let id = new_id();
        let path = self.dir.join(format!("{id}.{extension}"));
        // Write next to the target and rename so a crash never leaves a half-written image
        // that the folder scan would list.
        let tmp = self.dir.join(format!(".{id}.tmp"));
        fs::write(&tmp, encoded)?;
        if let Err(err) = fs::rename(&tmp, &path) {
            let _ = fs::remove_file(&tmp);
            return Err(err.into());
        }
        Ok(OwnBackground {
            id,
            added_at: modified_millis(&path),
            path,
        })
    }

    /// Adds the image at `source`. The size is checked before the file is read.
    pub fn add_file(&self, source: &Path) -> Result<OwnBackground> {
        let meta = fs::metadata(source).map_err(|_| Error::InvalidBackgroundImage)?;
        if !meta.is_file() || meta.len() > MAX_BACKGROUND_FILE_BYTES {
            return Err(Error::InvalidBackgroundImage);
        }
        let bytes = fs::read(source).map_err(|_| Error::InvalidBackgroundImage)?;
        self.add(&bytes)
    }

    /// All stored images, oldest first.
    pub fn list(&self) -> Result<Vec<OwnBackground>> {
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(err.into()),
        };
        let mut found = Vec::new();
        for entry in entries {
            let path = entry?.path();
            let (Some(stem), Some(ext)) = (
                path.file_stem().and_then(|s| s.to_str()),
                path.extension().and_then(|s| s.to_str()),
            ) else {
                continue;
            };
            if is_valid_id(stem) && is_stored_extension(ext) {
                found.push(OwnBackground {
                    id: stem.to_owned(),
                    added_at: modified_millis(&path),
                    path,
                });
            }
        }
        found.sort_by(|a, b| a.added_at.cmp(&b.added_at).then_with(|| a.id.cmp(&b.id)));
        Ok(found)
    }

    /// The stored image with this id, if any.
    pub fn get(&self, id: &str) -> Result<Option<OwnBackground>> {
        Ok(self.list()?.into_iter().find(|image| image.id == id))
    }

    /// Deletes an image. When it is the current default background, the default resets to
    /// blur (FR-8.15).
    pub fn delete(&self, store: &Store, id: &str) -> Result<()> {
        let image = self
            .get(id)?
            .ok_or_else(|| Error::UnknownBackgroundImage(id.to_owned()))?;
        fs::remove_file(&image.path)?;
        let mut settings = store.settings()?;
        if settings.camera_default_background == CameraBackground::Own(id.to_owned()) {
            settings.camera_default_background = CameraBackground::Blur;
            store.save_settings(&settings)?;
        }
        Ok(())
    }
}

/// Decodes, orients, scales and re-encodes. Returns the bytes and the file extension.
fn reencode(bytes: &[u8]) -> Result<(Vec<u8>, &'static str)> {
    let reader = ImageReader::new(BufReader::new(Cursor::new(bytes)))
        .with_guessed_format()
        .map_err(|_| Error::InvalidBackgroundImage)?;
    if !matches!(
        reader.format(),
        Some(ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP)
    ) {
        return Err(Error::InvalidBackgroundImage);
    }
    let mut decoder = reader
        .into_decoder()
        .map_err(|_| Error::InvalidBackgroundImage)?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DECODE_SIDE);
    limits.max_image_height = Some(MAX_DECODE_SIDE);
    limits.max_alloc = Some(MAX_DECODE_ALLOC_BYTES);
    decoder
        .set_limits(limits)
        .map_err(|_| Error::InvalidBackgroundImage)?;
    // The orientation tag is applied before the metadata goes away, or photos turn sideways.
    let orientation = decoder
        .orientation()
        .map_err(|_| Error::InvalidBackgroundImage)?;
    let mut picture =
        DynamicImage::from_decoder(decoder).map_err(|_| Error::InvalidBackgroundImage)?;
    picture.apply_orientation(orientation);

    if picture.width() > MAX_BACKGROUND_PX || picture.height() > MAX_BACKGROUND_PX {
        picture = picture.resize(MAX_BACKGROUND_PX, MAX_BACKGROUND_PX, FilterType::Lanczos3);
    }

    let mut out = Vec::new();
    // Pictures with transparency stay PNG; everything else becomes a smaller JPEG.
    if picture.color().has_alpha() {
        picture
            .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
            .map_err(|_| Error::InvalidBackgroundImage)?;
        Ok((out, "png"))
    } else {
        picture
            .to_rgb8()
            .write_with_encoder(JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY))
            .map_err(|_| Error::InvalidBackgroundImage)?;
        Ok((out, "jpg"))
    }
}

fn new_id() -> String {
    // UUID version 4 layout from 128 random bits.
    let mut bits: u128 = rand::random();
    bits = (bits & !(0xf << 76)) | (0x4 << 76);
    bits = (bits & !(0x3 << 62)) | (0x2 << 62);
    let hex = format!("{bits:032x}");
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// Ids come from the settings and the UI; only canonical lowercase UUIDs may touch the disk,
/// so an id can never name a path outside the folder.
fn is_valid_id(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_digit() || (b'a'..=b'f').contains(&b),
        })
}

fn is_stored_extension(ext: &str) -> bool {
    matches!(ext, "jpg" | "png")
}

fn modified_millis(path: &Path) -> UnixMillis {
    fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .and_then(|elapsed| UnixMillis::try_from(elapsed.as_millis()).ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgb, Rgba};

    fn png(width: u32, height: u32) -> Vec<u8> {
        let img = ImageBuffer::from_fn(width, height, |x, y| {
            Rgb([(x % 256) as u8, (y % 256) as u8, 7])
        });
        let mut out = Vec::new();
        DynamicImage::ImageRgb8(img)
            .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
            .expect("encode");
        out
    }

    fn setup() -> (tempfile::TempDir, Store, CameraBackgrounds) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Store::open(dir.path()).expect("store");
        let backgrounds = CameraBackgrounds::new(dir.path());
        (dir, store, backgrounds)
    }

    #[test]
    fn large_image_is_scaled_to_1920_and_reencoded() {
        let (_dir, _store, backgrounds) = setup();
        let added = backgrounds.add(&png(3000, 1500)).expect("add");
        let stored = image::open(&added.path).expect("open");
        assert_eq!((stored.width(), stored.height()), (1920, 960));
        assert_eq!(added.path.extension().and_then(|e| e.to_str()), Some("jpg"));
        assert_eq!(backgrounds.list().expect("list"), vec![added]);
    }

    #[test]
    fn small_image_keeps_its_size_and_alpha_stays_png() {
        let (_dir, _store, backgrounds) = setup();
        let img = ImageBuffer::from_fn(40, 30, |_, _| Rgba([1, 2, 3, 128]));
        let mut bytes = Vec::new();
        DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .expect("encode");
        let added = backgrounds.add(&bytes).expect("add");
        let stored = image::open(&added.path).expect("open");
        assert_eq!((stored.width(), stored.height()), (40, 30));
        assert_eq!(added.path.extension().and_then(|e| e.to_str()), Some("png"));
    }

    #[test]
    fn metadata_is_stripped() {
        let (_dir, _store, backgrounds) = setup();
        // A JPEG with a COM segment and an APP1 "Exif" segment injected after the SOI marker.
        let mut jpeg = Vec::new();
        DynamicImage::ImageRgb8(ImageBuffer::from_pixel(16, 16, Rgb([9, 9, 9])))
            .write_to(&mut Cursor::new(&mut jpeg), ImageFormat::Jpeg)
            .expect("encode");
        let comment = b"SECRET-GPS-48.1N";
        let mut tagged = jpeg[..2].to_vec();
        tagged.extend([0xFF, 0xFE]);
        tagged.extend(((comment.len() + 2) as u16).to_be_bytes());
        tagged.extend(comment);
        let exif = b"Exif\0\0SECRET-EXIF";
        tagged.extend([0xFF, 0xE1]);
        tagged.extend(((exif.len() + 2) as u16).to_be_bytes());
        tagged.extend(exif);
        tagged.extend(&jpeg[2..]);
        assert!(tagged.windows(6).any(|w| w == b"SECRET"));

        let added = backgrounds.add(&tagged).expect("add");
        let stored = fs::read(&added.path).expect("read");
        assert!(!stored.windows(6).any(|w| w == b"SECRET"));
        assert!(!stored.windows(4).any(|w| w == b"Exif"));
    }

    #[test]
    fn invalid_and_oversized_images_are_refused() {
        let (dir, _store, backgrounds) = setup();
        assert!(matches!(
            backgrounds.add(b"not an image at all"),
            Err(Error::InvalidBackgroundImage)
        ));
        assert!(matches!(
            backgrounds.add(&[]),
            Err(Error::InvalidBackgroundImage)
        ));
        // GIF is decodable but not an accepted format.
        let mut gif = Vec::new();
        DynamicImage::ImageRgba8(ImageBuffer::from_pixel(4, 4, Rgba([1, 2, 3, 255])))
            .write_to(&mut Cursor::new(&mut gif), ImageFormat::Gif)
            .expect("encode");
        assert!(matches!(
            backgrounds.add(&gif),
            Err(Error::InvalidBackgroundImage)
        ));
        // Over the 10 MB file limit, checked before decoding.
        let big = vec![0u8; MAX_BACKGROUND_FILE_BYTES as usize + 1];
        assert!(matches!(
            backgrounds.add(&big),
            Err(Error::InvalidBackgroundImage)
        ));
        let big_file = dir.path().join("big.png");
        fs::write(&big_file, &big).expect("write");
        assert!(matches!(
            backgrounds.add_file(&big_file),
            Err(Error::InvalidBackgroundImage)
        ));
        assert!(matches!(
            backgrounds.add_file(&dir.path().join("missing.png")),
            Err(Error::InvalidBackgroundImage)
        ));
        assert!(backgrounds.list().expect("list").is_empty());
    }

    #[test]
    fn truncated_image_is_refused() {
        let (_dir, _store, backgrounds) = setup();
        let bytes = png(64, 64);
        assert!(matches!(
            backgrounds.add(&bytes[..bytes.len() / 2]),
            Err(Error::InvalidBackgroundImage)
        ));
        assert!(backgrounds.list().expect("list").is_empty());
    }

    #[test]
    fn image_claiming_a_huge_size_is_refused() {
        let (_dir, _store, backgrounds) = setup();
        // Tiny file whose header says 60000 x 60000: must fail on the limits, not allocate.
        let mut bytes = png(8, 8);
        bytes[16..20].copy_from_slice(&60_000u32.to_be_bytes());
        bytes[20..24].copy_from_slice(&60_000u32.to_be_bytes());
        assert!(matches!(
            backgrounds.add(&bytes),
            Err(Error::InvalidBackgroundImage)
        ));
    }

    #[test]
    fn delete_removes_the_file_and_resets_a_default_that_pointed_at_it() {
        let (_dir, store, backgrounds) = setup();
        let first = backgrounds.add(&png(10, 10)).expect("add");
        let second = backgrounds.add(&png(12, 12)).expect("add");
        let mut settings = store.settings().expect("settings");
        settings.camera_default_background = CameraBackground::Own(first.id.clone());
        store.save_settings(&settings).expect("save");

        // Deleting another image leaves the default alone.
        backgrounds.delete(&store, &second.id).expect("delete");
        assert_eq!(
            store
                .settings()
                .expect("settings")
                .camera_default_background,
            CameraBackground::Own(first.id.clone())
        );
        backgrounds.delete(&store, &first.id).expect("delete");
        assert_eq!(
            store
                .settings()
                .expect("settings")
                .camera_default_background,
            CameraBackground::Blur
        );
        assert!(backgrounds.list().expect("list").is_empty());
        assert!(matches!(
            backgrounds.delete(&store, &first.id),
            Err(Error::UnknownBackgroundImage(_))
        ));
    }

    #[test]
    fn delete_refuses_ids_that_could_leave_the_folder() {
        let (dir, store, backgrounds) = setup();
        fs::write(dir.path().join("victim.jpg"), b"x").expect("write");
        backgrounds.add(&png(4, 4)).expect("add");
        assert!(backgrounds.delete(&store, "../victim").is_err());
        assert!(dir.path().join("victim.jpg").exists());
    }

    #[test]
    fn list_ignores_foreign_and_temporary_files() {
        let (_dir, _store, backgrounds) = setup();
        let added = backgrounds.add(&png(4, 4)).expect("add");
        fs::write(backgrounds.dir.join("notes.txt"), b"x").expect("write");
        fs::write(backgrounds.dir.join(".tmp"), b"x").expect("write");
        assert_eq!(backgrounds.list().expect("list").len(), 1);
        assert_eq!(backgrounds.get(&added.id).expect("get"), Some(added));
    }

    #[test]
    fn generated_ids_are_valid_uuids() {
        let id = new_id();
        assert!(is_valid_id(&id), "{id}");
        assert_eq!(&id[14..15], "4");
        assert_ne!(id, new_id());
    }
}
