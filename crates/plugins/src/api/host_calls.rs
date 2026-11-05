//! The host-owned calls of API 1.1 (FR-9.103 … FR-9.108): `files.save`, `files.import-picture`
//! and `chat.post-image`. The plugin hands over or receives bytes, never a path, a folder or the
//! chat itself. This module holds the input checks and the reply channel; the service gates each
//! call on a fresh user gesture, and the app opens the dialogs, writes or reads the file and posts
//! the image ([`crate::service::ServiceEvent::HostCall`]).

use std::io::Cursor;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use image::{ImageFormat, ImageReader};

use crate::runtime::wit::types::Error;

pub const SAVE_PERMISSION: &str = "files.save";
pub const IMPORT_PICTURE_PERMISSION: &str = "files.import-picture";
pub const POST_IMAGE_PERMISSION: &str = "chat.post-image";

/// Largest file `files.save` writes (FR-9.103).
pub const MAX_SAVE_BYTES: usize = 32 * 1024 * 1024;
/// Longest file name, in bytes, after it was reduced to a plain name (FR-9.103).
pub const MAX_NAME_BYTES: usize = 120;
/// Largest picture `files.import-picture` hands over (FR-9.105: 10 MB).
pub const MAX_PICTURE_BYTES: u64 = 10_000_000;
/// Longest `alt` text of `chat.post-image`, in bytes (FR-9.104).
pub const MAX_ALT_BYTES: usize = 200;
/// How long a plugin waits for the user in a save dialog or picker. Like a keychain prompt it
/// is waiting for the user, not the plugin's own work; after it the call counts as cancelled.
pub const DIALOG_WAIT_CAP: Duration = Duration::from_secs(120);
/// How long `chat.post-image` waits for the app to decide how the image goes out (chat image or
/// file offer). The app answers before it sends, so this is only a safety net.
pub const POST_WAIT_CAP: Duration = Duration::from_millis(400);
/// An extension longer than this is not kept when a long name is shortened.
const MAX_KEPT_EXTENSION: usize = 16;

/// The gesture-bound calls a gesture can allow, each once per gesture (FR-9.45, FR-9.106).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostCall {
    /// `webview.show` opening the web dialog (FR-9.40).
    OpenDialog,
    Save,
    ImportPicture,
    PostImage,
}

/// A picture the user picked: the file name only and its checked bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    pub name: String,
    pub data: Vec<u8>,
}

/// How `chat.post-image` delivered the picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostOutcome {
    /// A normal chat image.
    Image,
    /// Too large for the server's image limit: offered through file sharing (spec 07).
    FileOffer,
}

/// The app's side of a host call's answer. The plugin's thread waits on the matching
/// [`Waiter`]; once it gave up (timeout) [`Reply::is_waiting`] is false and the app must not
/// write anything any more.
#[derive(Debug, Clone)]
pub struct Reply<T> {
    tx: mpsc::Sender<T>,
    gone: Arc<AtomicBool>,
}

impl<T> Reply<T> {
    /// Whether the plugin still waits for the answer.
    pub fn is_waiting(&self) -> bool {
        !self.gone.load(Ordering::Acquire)
    }

    /// Answers the call. False when the plugin no longer waits.
    pub fn send(self, value: T) -> bool {
        self.is_waiting() && self.tx.send(value).is_ok()
    }
}

/// The plugin's side of a host call's answer.
#[derive(Debug)]
pub struct Waiter<T> {
    rx: mpsc::Receiver<T>,
    gone: Arc<AtomicBool>,
}

impl<T> Waiter<T> {
    /// Waits up to `cap`; `None` when no answer came (the app dropped the call or it took too
    /// long). Afterwards the app sees that nobody waits any more.
    pub fn wait(self, cap: Duration) -> Option<T> {
        let answer = self.rx.recv_timeout(cap).ok();
        self.gone.store(true, Ordering::Release);
        answer
    }
}

/// What the app carries out for a host call that passed its permission and gesture checks.
/// Contents are untrusted plugin bytes; the app never logs them, the names or the paths.
#[derive(Debug, Clone)]
pub enum HostRequest {
    /// Open the save dialog with `name`, write `data` to the chosen file, answer true; false
    /// when the user cancels. Write nothing once [`Reply::is_waiting`] is false.
    Save {
        name: String,
        data: Vec<u8>,
        reply: Reply<bool>,
    },
    /// Open the picker for PNG, JPEG and WebP; check the file with [`check_picture`].
    ImportPicture {
        reply: Reply<Result<Option<Picture>, Error>>,
    },
    /// Post `png` into the user's current channel on `server` with the text line `line`
    /// ("[Name] alt"): as a chat image when it fits the server's image limit, else as a file
    /// offer. Answer before sending.
    PostImage {
        server: mumcord_store::ServerId,
        line: String,
        png: Vec<u8>,
        reply: Reply<Result<PostOutcome, Error>>,
    },
}

/// A new answer channel for one host call.
pub fn reply_channel<T>() -> (Reply<T>, Waiter<T>) {
    let (tx, rx) = mpsc::channel();
    let gone = Arc::new(AtomicBool::new(false));
    (
        Reply {
            tx,
            gone: Arc::clone(&gone),
        },
        Waiter { rx, gone },
    )
}

/// Reduces a name to a plain file name: the last path part, without control characters, the
/// characters Windows forbids, and leading or trailing dots and spaces (so never `..` or a
/// hidden file), shortened to [`MAX_NAME_BYTES`] while keeping a short extension. `None` when
/// nothing is left.
pub fn plain_file_name(name: &str) -> Option<String> {
    let last = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let cleaned: String = last
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
        .collect();
    let trimmed = cleaned.trim_matches(|c: char| c == '.' || c.is_whitespace());
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.len() <= MAX_NAME_BYTES {
        return Some(trimmed.to_owned());
    }
    let (stem, extension) = match trimmed.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && ext.len() <= MAX_KEPT_EXTENSION => {
            (stem, Some(ext))
        }
        _ => (trimmed, None),
    };
    let budget = MAX_NAME_BYTES - extension.map_or(0, |ext| ext.len() + 1);
    let stem = cut_to(stem, budget).trim_end_matches(|c: char| c == '.' || c.is_whitespace());
    if stem.is_empty() {
        return None;
    }
    Some(match extension {
        Some(ext) => format!("{stem}.{ext}"),
        None => stem.to_owned(),
    })
}

/// The longest start of `text` with at most `max` bytes, cut at a character boundary.
fn cut_to(text: &str, max: usize) -> &str {
    let mut end = max.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// `files.save`: the plain file name to suggest, or `invalid` / `too-large`.
pub fn check_save(suggested_name: &str, data: &[u8]) -> Result<String, Error> {
    if data.len() > MAX_SAVE_BYTES {
        return Err(Error::TooLarge);
    }
    plain_file_name(suggested_name).ok_or(Error::Invalid)
}

/// Whether `bytes` decode as an image of `format` far enough to read its size (the header).
fn decodes_as(bytes: &[u8], format: ImageFormat) -> bool {
    ImageReader::with_format(Cursor::new(bytes), format)
        .into_dimensions()
        .is_ok_and(|(w, h)| w > 0 && h > 0)
}

/// `chat.post-image`: the bytes must be a PNG (by its header, not by a claim) and `alt` short.
pub fn check_post_image(png: &[u8], alt: &str) -> Result<(), Error> {
    if alt.len() > MAX_ALT_BYTES || !decodes_as(png, ImageFormat::Png) {
        return Err(Error::Invalid);
    }
    Ok(())
}

/// `files.import-picture`: a PNG, JPEG or WebP picture up to [`MAX_PICTURE_BYTES`], recognised
/// by its content. `name` is reduced to a plain file name.
pub fn check_picture(name: &str, data: Vec<u8>) -> Result<Picture, Error> {
    if data.len() as u64 > MAX_PICTURE_BYTES {
        return Err(Error::TooLarge);
    }
    let format = image::guess_format(&data).map_err(|_| Error::Invalid)?;
    if !matches!(
        format,
        ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP
    ) || !decodes_as(&data, format)
    {
        return Err(Error::Invalid);
    }
    let name = plain_file_name(name).unwrap_or_else(|| "picture".to_owned());
    Ok(Picture { name, data })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A tiny valid image in `format`.
    pub(crate) fn image_bytes(format: ImageFormat) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        image::RgbImage::new(2, 2)
            .write_to(&mut out, format)
            .expect("encode");
        out.into_inner()
    }

    #[test]
    fn names_become_plain_file_names() {
        assert_eq!(plain_file_name("board.png").as_deref(), Some("board.png"));
        assert_eq!(
            plain_file_name("../../etc/passwd").as_deref(),
            Some("passwd")
        );
        assert_eq!(
            plain_file_name("C:\\Users\\x\\a:b?.txt").as_deref(),
            Some("ab.txt")
        );
        assert_eq!(plain_file_name(".hidden").as_deref(), Some("hidden"));
        assert_eq!(plain_file_name(".."), None);
        assert_eq!(plain_file_name("folder/"), None);
        assert_eq!(plain_file_name("  "), None);
        assert_eq!(plain_file_name("a\nb\u{0}.png").as_deref(), Some("ab.png"));
    }

    #[test]
    fn long_names_are_shortened_and_keep_their_extension() {
        let long = format!("{}.png", "ä".repeat(100));
        let name = plain_file_name(&long).expect("name");
        assert!(name.len() <= MAX_NAME_BYTES);
        assert!(name.ends_with(".png"));
        let no_ext = "x".repeat(300);
        assert_eq!(
            plain_file_name(&no_ext).map(|n| n.len()),
            Some(MAX_NAME_BYTES)
        );
    }

    #[test]
    fn save_checks_name_and_size() {
        assert_eq!(check_save("a.png", b"x"), Ok("a.png".to_owned()));
        assert_eq!(check_save("", b"x"), Err(Error::Invalid));
        assert_eq!(
            check_save("a.png", &vec![0; MAX_SAVE_BYTES + 1]),
            Err(Error::TooLarge)
        );
        assert!(check_save("a.png", &vec![0; MAX_SAVE_BYTES]).is_ok());
    }

    #[test]
    fn post_image_takes_only_a_real_png() {
        let png = image_bytes(ImageFormat::Png);
        assert_eq!(check_post_image(&png, "A board"), Ok(()));
        assert_eq!(
            check_post_image(&image_bytes(ImageFormat::Jpeg), ""),
            Err(Error::Invalid)
        );
        assert_eq!(check_post_image(b"not a png", ""), Err(Error::Invalid));
        // The signature alone is not enough: the header must decode.
        assert_eq!(check_post_image(&png[..12], ""), Err(Error::Invalid));
        assert_eq!(
            check_post_image(&png, &"a".repeat(MAX_ALT_BYTES + 1)),
            Err(Error::Invalid)
        );
    }

    #[test]
    fn pictures_are_checked_by_content() {
        for format in [ImageFormat::Png, ImageFormat::Jpeg, ImageFormat::WebP] {
            let data = image_bytes(format);
            let picture = check_picture("/home/me/sketch.img", data.clone()).expect("picture");
            assert_eq!(
                picture,
                Picture {
                    name: "sketch.img".into(),
                    data
                }
            );
        }
        assert_eq!(
            check_picture("x.png", image_bytes(ImageFormat::Gif)),
            Err(Error::Invalid)
        );
        assert_eq!(
            check_picture("x.png", b"<svg></svg>".to_vec()),
            Err(Error::Invalid)
        );
        let mut big = image_bytes(ImageFormat::Png);
        big.resize(MAX_PICTURE_BYTES as usize + 1, 0);
        assert_eq!(check_picture("x.png", big), Err(Error::TooLarge));
    }

    #[test]
    fn a_late_answer_is_refused() {
        let (reply, waiter) = reply_channel::<bool>();
        assert_eq!(waiter.wait(Duration::from_millis(1)), None);
        assert!(!reply.is_waiting());
        assert!(!reply.send(true));
        let (reply, waiter) = reply_channel::<bool>();
        assert!(reply.send(true));
        assert_eq!(waiter.wait(Duration::from_secs(1)), Some(true));
    }
}
