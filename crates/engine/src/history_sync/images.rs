//! Inline images in synced bodies as hash references (FR-1.60d … FR-1.60g, issue #402).
//!
//! A stored chat body carries its pictures as `<img src="data:image/…;base64,…">`. When the
//! requester announced [`FEATURE`], the holder swaps each such `src` attribute for
//! [`ATTRIBUTE`]`="<sha256 hex>:<size>:<type>:<width>x<height>"` and sends no image bytes in the
//! history stream; the requester fetches the bytes through the full-image path and puts the
//! `src` back. The swap touches nothing but that one attribute, so [`restore`] gives back the
//! holder's body byte for byte.
//!
//! Bodies are the sanitizer's output (double-quoted attributes, `<` escaped in text), which
//! keeps the small tag scanner below exact. Anything it does not recognise stays inline.

use std::borrow::Cow;
use std::fmt;
use std::ops::Range;

use crate::full_image::check::{self, MAX_BYTES};
use crate::full_image::wire::FullRef;
use crate::web::fetch::ImageKind;

/// Hello feature of clients that resolve image references in history bodies.
pub const FEATURE: &str = "histimg";

/// Attribute that replaces the `src` of a referenced image.
pub const ATTRIBUTE: &str = "data-mumcord-image";

/// Longest side a reference may announce; matches the full-image limit.
const MAX_SIDE: u32 = check::MAX_SIDE;

/// A valid 1×1 PNG (base64) for tests.
#[cfg(test)]
pub(crate) const TEST_PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

/// A referenced image: content hash, size, type and pixel size (for the placeholder).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageRef {
    pub full: FullRef,
    pub kind: ImageKind,
    pub width: u32,
    pub height: u32,
}

impl ImageRef {
    /// Parses `<64 hex>:<size>:<type>:<w>x<h>`; anything else, oversized images included, is
    /// `None`.
    pub fn parse(value: &str) -> Option<Self> {
        let mut parts = value.split(':');
        let (hash, size, kind, dims) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
        if parts.next().is_some() {
            return None;
        }
        let full = FullRef::parse(&format!("{hash}:{size}"), MAX_BYTES)?;
        let kind = kind_of(kind)?;
        let (width, height) = dims.split_once('x')?;
        let width = parse_side(width)?;
        let height = parse_side(height)?;
        Some(Self {
            full,
            kind,
            width,
            height,
        })
    }

    pub fn hash_hex(&self) -> String {
        self.full.hash_hex()
    }
}

impl fmt::Display for ImageRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{}:{}x{}",
            self.full.hash_hex(),
            self.full.size,
            type_name(self.kind),
            self.width,
            self.height
        )
    }
}

fn parse_side(text: &str) -> Option<u32> {
    if text.is_empty() || text.len() > 4 || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok().filter(|&n| n > 0 && n <= MAX_SIDE)
}

fn type_name(kind: ImageKind) -> &'static str {
    match kind {
        ImageKind::Png => "png",
        ImageKind::Jpeg => "jpeg",
        ImageKind::Gif => "gif",
        ImageKind::Webp => "webp",
    }
}

fn kind_of(name: &str) -> Option<ImageKind> {
    match name {
        "png" => Some(ImageKind::Png),
        "jpeg" => Some(ImageKind::Jpeg),
        "gif" => Some(ImageKind::Gif),
        "webp" => Some(ImageKind::Webp),
        _ => None,
    }
}

/// One attribute of an `<img>` tag: its name, value and the bytes from name to closing quote.
struct Attribute<'a> {
    name: &'a str,
    value: &'a str,
    range: Range<usize>,
}

/// The attributes of every `<img>` tag in `body`, in order.
fn image_attributes(body: &str) -> Vec<Attribute<'_>> {
    let bytes = body.as_bytes();
    let mut found = Vec::new();
    let mut at = 0;
    while let Some(offset) = body[at..].find('<') {
        let start = at + offset + 1;
        let name_end = start
            + bytes[start..]
                .iter()
                .position(|b| b.is_ascii_whitespace() || *b == b'>' || *b == b'/')
                .unwrap_or(bytes.len() - start);
        let is_img = body[start..name_end].eq_ignore_ascii_case("img");
        at = name_end;
        // Attributes until the end of the tag; quoted values may contain `>`.
        loop {
            while at < bytes.len() && (bytes[at].is_ascii_whitespace() || bytes[at] == b'/') {
                at += 1;
            }
            if at >= bytes.len() || bytes[at] == b'>' {
                at = (at + 1).min(bytes.len());
                break;
            }
            let name_start = at;
            while at < bytes.len()
                && !bytes[at].is_ascii_whitespace()
                && !matches!(bytes[at], b'=' | b'>' | b'/')
            {
                at += 1;
            }
            let name = &body[name_start..at];
            if at < bytes.len() && bytes[at] == b'=' && bytes.get(at + 1) == Some(&b'"') {
                let value_start = at + 2;
                let Some(len) = body[value_start..].find('"') else {
                    return found;
                };
                at = value_start + len + 1;
                if is_img {
                    found.push(Attribute {
                        name,
                        value: &body[value_start..value_start + len],
                        range: name_start..at,
                    });
                }
            } else if at < bytes.len() && bytes[at] == b'=' {
                // Not the sanitizer's shape: skip the unquoted value, record nothing.
                while at < bytes.len() && !bytes[at].is_ascii_whitespace() && bytes[at] != b'>' {
                    at += 1;
                }
            } else if at == name_start {
                at += 1;
            }
        }
    }
    found
}

/// The image of an inline `src` value, when it can travel as a reference: a supported type
/// whose bytes match it, canonical base64 (so [`restore`] gives back the same text), within
/// the full-image size limit and with readable dimensions.
fn inline_image(src: &str) -> Option<(ImageRef, Vec<u8>)> {
    let rest = src.strip_prefix("data:image/")?;
    let (name, payload) = rest.split_once(";base64,")?;
    let kind = kind_of(name)?;
    let bytes = crate::people::base64_decode(payload)?;
    if bytes.is_empty() || bytes.len() > MAX_BYTES || crate::people::base64(&bytes) != payload {
        return None;
    }
    if ImageKind::sniff(&bytes) != Some(kind) {
        return None;
    }
    let (width, height) = check::dimensions(&bytes, kind)?;
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return None;
    }
    Some((
        ImageRef {
            full: FullRef::of(&bytes),
            kind,
            width,
            height,
        },
        bytes,
    ))
}

/// The inline images of `body` that can travel as references, with their bytes.
pub fn inline_images(body: &str) -> Vec<(ImageRef, Vec<u8>)> {
    image_attributes(body)
        .into_iter()
        .filter(|a| a.name == "src")
        .filter_map(|a| inline_image(a.value))
        .collect()
}

/// `body` with every eligible inline image replaced by its reference, and those images.
/// `allow` decides per image (e.g. the holder's memory budget); refused ones stay inline.
pub fn to_references(
    body: &str,
    mut allow: impl FnMut(&ImageRef, &[u8]) -> bool,
) -> (String, Vec<(ImageRef, Vec<u8>)>) {
    let mut out = String::with_capacity(body.len());
    let mut images = Vec::new();
    let mut last = 0;
    for attribute in image_attributes(body) {
        if attribute.name != "src" {
            continue;
        }
        let Some((image, bytes)) = inline_image(attribute.value) else {
            continue;
        };
        if !allow(&image, &bytes) {
            continue;
        }
        out.push_str(&body[last..attribute.range.start]);
        out.push_str(&format!("{ATTRIBUTE}=\"{image}\""));
        last = attribute.range.end;
        images.push((image, bytes));
    }
    if images.is_empty() {
        return (body.to_owned(), images);
    }
    out.push_str(&body[last..]);
    (out, images)
}

/// The image references in `body`, in order (duplicates included).
pub fn references(body: &str) -> Vec<ImageRef> {
    image_attributes(body)
        .into_iter()
        .filter(|a| a.name == ATTRIBUTE)
        .filter_map(|a| ImageRef::parse(a.value))
        .collect()
}

/// `body` with every reference that `bytes_of` can resolve turned back into its inline image.
pub fn restore<'a>(
    body: &'a str,
    mut bytes_of: impl FnMut(&ImageRef) -> Option<Vec<u8>>,
) -> Cow<'a, str> {
    let mut out = String::new();
    let mut last = 0;
    for attribute in image_attributes(body) {
        if attribute.name != ATTRIBUTE {
            continue;
        }
        let Some(image) = ImageRef::parse(attribute.value) else {
            continue;
        };
        let Some(bytes) = bytes_of(&image) else {
            continue;
        };
        out.push_str(&body[last..attribute.range.start]);
        out.push_str(&format!(
            "src=\"data:image/{};base64,{}\"",
            type_name(image.kind),
            crate::people::base64(&bytes)
        ));
        last = attribute.range.end;
    }
    if last == 0 {
        return Cow::Borrowed(body);
    }
    out.push_str(&body[last..]);
    Cow::Owned(out)
}

/// `body` with inline images in their reference form, so a body compares equal whether its
/// images are inline or referenced (deduplication of synced messages, FR-1.60e).
pub fn canonical(body: &str) -> Cow<'_, str> {
    if !body.contains("data:image/") {
        return Cow::Borrowed(body);
    }
    match to_references(body, |_, _| true) {
        (canonical, images) if !images.is_empty() => Cow::Owned(canonical),
        _ => Cow::Borrowed(body),
    }
}

#[cfg(test)]
mod tests {
    use super::TEST_PNG as PNG;
    use super::*;

    fn png_bytes() -> Vec<u8> {
        crate::people::base64_decode(PNG).expect("png")
    }

    #[test]
    fn references_replace_only_the_src_and_restore_exactly() {
        let body = format!(
            "<p>look</p><img src=\"data:image/png;base64,{PNG}\" data-mumcord-full=\"{}:9\" alt=\"a > b\"><img src=\"data:image/png;base64,{PNG}\">",
            "0".repeat(64)
        );
        let (referenced, images) = to_references(&body, |_, _| true);
        assert_eq!(images.len(), 2);
        assert!(!referenced.contains("base64"), "{referenced}");
        let image = images[0].0;
        assert_eq!(
            (image.width, image.height, image.kind),
            (1, 1, ImageKind::Png)
        );
        assert!(image.full.matches(&png_bytes()));
        assert!(referenced.contains(&format!("<img {ATTRIBUTE}=\"{image}\" data-mumcord-full=")));
        assert_eq!(references(&referenced), vec![image, image]);
        assert_eq!(restore(&referenced, |_| Some(png_bytes())), body);
        assert_eq!(
            restore(&referenced, |_| None),
            referenced,
            "unresolved stays a reference"
        );
    }

    #[test]
    fn only_canonical_supported_images_become_references() {
        let jpeg_label = format!("<img src=\"data:image/jpeg;base64,{PNG}\">");
        let not_canonical = format!(
            "<img src=\"data:image/png;base64,{}\">",
            PNG.trim_end_matches('=')
        );
        let text = format!("<p>src=\"data:image/png;base64,{PNG}\"</p>");
        let external = "<img src=\"https://e.org/a.png\">".to_owned();
        for body in [jpeg_label, not_canonical, text, external] {
            let (out, images) = to_references(&body, |_, _| true);
            assert!(images.is_empty(), "{body}");
            assert_eq!(out, body);
        }
    }

    #[test]
    fn refused_images_stay_inline() {
        let body = format!("<img src=\"data:image/png;base64,{PNG}\">");
        let (out, images) = to_references(&body, |_, _| false);
        assert_eq!((out, images.len()), (body, 0));
    }

    #[test]
    fn inline_and_referenced_bodies_have_the_same_canonical_form() {
        let body = format!("<img src=\"data:image/png;base64,{PNG}\">");
        let (referenced, _) = to_references(&body, |_, _| true);
        assert_eq!(canonical(&body), canonical(&referenced));
        assert_eq!(canonical("<p>hi</p>"), "<p>hi</p>");
    }

    #[test]
    fn malformed_references_are_ignored() {
        let hex = "a".repeat(64);
        for bad in [
            format!("{hex}:10:png"),
            format!("{hex}:10:svg:1x1"),
            format!("{hex}:10:png:0x1"),
            format!("{hex}:10:png:5000x1"),
            format!("{hex}:999999999:png:1x1"),
            format!("{hex}:10:png:1x1:x"),
            "x:10:png:1x1".to_owned(),
        ] {
            assert_eq!(ImageRef::parse(&bad), None, "{bad}");
        }
        let good = format!("{hex}:10:webp:640x480");
        assert_eq!(ImageRef::parse(&good).map(|r| r.to_string()), Some(good));
    }
}
