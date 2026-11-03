//! The file offer: a channel text message with a visible line for official clients and a
//! `data-mumcord-file` element for Mumcord (section 6.4).

use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use super::sanitize::sanitize_file_name;
use super::{MAX_FILE_SIZE, is_lower_hex, is_share_id};

/// Attribute that carries the offer JSON.
pub const ATTRIBUTE: &str = "data-mumcord-file";

/// Current offer format version.
const VERSION: u8 = 1;
const MAX_NAME_BYTES: usize = 255;
const MAX_MIME_CHARS: usize = 100;
/// An offer lives at most this long after it arrives (FR-7.15).
const MAX_LIFETIME_SECS: u64 = 24 * 60 * 60;

/// A validated file offer. `name` is the raw name from the wire; show [`FileOffer::display_name`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileOffer {
    /// Share id, 32 lowercase hex characters.
    pub id: String,
    pub name: String,
    pub size: u64,
    pub mime: String,
    /// SHA-256 of the file, 64 lowercase hex characters.
    pub sha256: String,
    /// Unix seconds, already clamped to 24 hours after arrival.
    pub expires: u64,
}

impl FileOffer {
    /// The name as shown and saved (FR-7.23).
    pub fn display_name(&self) -> String {
        sanitize_file_name(&self.name)
    }
}

#[derive(Serialize, Deserialize)]
struct Wire {
    v: u8,
    id: String,
    n: String,
    s: u64,
    m: String,
    h: String,
    e: u64,
}

/// Why an offer could not be built for sending.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum OfferError {
    #[error("the file is larger than 2 GB")]
    TooLarge,
    #[error("the offer fields are invalid")]
    Invalid,
}

/// Builds the offer message HTML. `sender` and the file name are escaped; the visible line shows
/// the sanitized name.
pub fn encode_offer_html(sender: &str, offer: &FileOffer) -> Result<String, OfferError> {
    if offer.size > MAX_FILE_SIZE {
        return Err(OfferError::TooLarge);
    }
    let wire = Wire {
        v: VERSION,
        id: offer.id.clone(),
        n: offer.name.clone(),
        s: offer.size,
        m: offer.mime.clone(),
        h: offer.sha256.clone(),
        e: offer.expires,
    };
    if validate(&wire).is_none() {
        return Err(OfferError::Invalid);
    }
    let json = serde_json::to_string(&wire).map_err(|_| OfferError::Invalid)?;
    Ok(format!(
        "<p>\u{1F4CE} {} shared a file: <b>{}</b> ({}). Download it with Mumcord.</p>\
         <span {ATTRIBUTE}=\"{}\"></span>",
        escape(sender),
        escape(&offer.display_name()),
        format_size(offer.size),
        escape(&json),
    ))
}

/// Extracts and validates the offer in a received message. `arrival` is the receive time in Unix
/// seconds; the expiry is clamped to 24 hours after it. Anything invalid gives `None`, and the
/// message then stays the plain line.
pub fn parse_offer_html(html: &str, arrival: u64) -> Option<FileOffer> {
    static ATTR: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(&format!(r#"{ATTRIBUTE}\s*=\s*"([^"]*)""#)).expect("static pattern")
    });
    let raw = ATTR.captures(html)?.get(1)?.as_str();
    let wire: Wire = serde_json::from_str(&unescape(raw)).ok()?;
    validate(&wire)?;
    Some(FileOffer {
        id: wire.id,
        name: wire.n,
        size: wire.s,
        mime: wire.m,
        sha256: wire.h,
        expires: wire.e.min(arrival.saturating_add(MAX_LIFETIME_SECS)),
    })
}

fn validate(w: &Wire) -> Option<()> {
    let ok = w.v >= VERSION
        && is_share_id(&w.id)
        && !w.n.is_empty()
        && w.n.len() <= MAX_NAME_BYTES
        && w.s <= MAX_FILE_SIZE
        && w.m.chars().count() <= MAX_MIME_CHARS
        && w.h.len() == 64
        && is_lower_hex(&w.h);
    ok.then_some(())
}

/// Human-readable size with binary units, e.g. `1.5 MB`.
pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn unescape(s: &str) -> String {
    s.replace("&quot;", "\"")
        .replace("&#34;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offer() -> FileOffer {
        FileOffer {
            id: "0123456789abcdef0123456789abcdef".into(),
            name: "My \"report\" <1>&.pdf".into(),
            size: 1_572_864,
            mime: "application/pdf".into(),
            sha256: "a".repeat(64),
            expires: 1_000 + 3_600,
        }
    }

    fn html_with(json: &str) -> String {
        format!("<p>x</p><span {ATTRIBUTE}=\"{}\"></span>", escape(json))
    }

    const GOOD: &str = r#"{"v":1,"id":"0123456789abcdef0123456789abcdef","n":"a.txt","s":5,"m":"text/plain","h":"0000000000000000000000000000000000000000000000000000000000000000","e":2000}"#;

    #[test]
    fn round_trip_with_special_characters() {
        let html = encode_offer_html("Al<ice>", &offer()).expect("valid");
        assert!(html.contains("Al&lt;ice&gt; shared a file"));
        assert!(html.contains("(1.5 MB)"));
        assert_eq!(parse_offer_html(&html, 1_000), Some(offer()));
    }

    #[test]
    fn visible_line_uses_the_sanitized_name() {
        let mut o = offer();
        o.name = "../x\u{202E}.txt".into();
        let html = encode_offer_html("A", &o).expect("valid");
        assert!(html.contains("<b>x.txt</b>"));
    }

    #[test]
    fn expiry_is_clamped_to_24_hours_after_arrival() {
        let html = html_with(&GOOD.replace("2000", "999999999999"));
        assert_eq!(
            parse_offer_html(&html, 100).expect("valid").expires,
            100 + 86_400
        );
    }

    #[test]
    fn higher_versions_and_unknown_fields_are_accepted() {
        let json = GOOD.replace("\"v\":1", "\"v\":7,\"extra\":true");
        assert!(parse_offer_html(&html_with(&json), 0).is_some());
    }

    #[test]
    fn invalid_offers_are_rejected() {
        let long_name = "n".repeat(256);
        let long_mime = "m".repeat(101);
        let bad = [
            GOOD.replace("\"v\":1", "\"v\":0"),
            GOOD.replace(
                "0123456789abcdef0123456789abcdef",
                "0123456789ABCDEF0123456789abcdef",
            ),
            GOOD.replace("0123456789abcdef0123456789abcdef", "0123"),
            GOOD.replace("\"a.txt\"", "\"\""),
            GOOD.replace("a.txt", &long_name),
            GOOD.replace("\"s\":5", "\"s\":2147483649"),
            GOOD.replace("\"s\":5", "\"s\":-1"),
            GOOD.replace("text/plain", &long_mime),
            GOOD.replace(
                "0000000000000000000000000000000000000000000000000000000000000000",
                "xyz",
            ),
            GOOD.replace("\"e\":2000", ""),
            "not json".into(),
        ];
        for json in bad {
            assert_eq!(parse_offer_html(&html_with(&json), 0), None, "{json}");
        }
        assert_eq!(parse_offer_html("<p>plain</p>", 0), None);
    }

    #[test]
    fn boundary_values_are_accepted() {
        let json = GOOD
            .replace("\"s\":5", "\"s\":2147483648")
            .replace("a.txt", &"n".repeat(255));
        assert!(parse_offer_html(&html_with(&json), 0).is_some());
        let empty = GOOD.replace("\"s\":5", "\"s\":0");
        assert!(parse_offer_html(&html_with(&empty), 0).is_some());
    }

    #[test]
    fn encoding_refuses_oversized_or_invalid_offers() {
        let mut o = offer();
        o.size = MAX_FILE_SIZE + 1;
        assert_eq!(encode_offer_html("A", &o), Err(OfferError::TooLarge));
        let mut o = offer();
        o.id = "short".into();
        assert_eq!(encode_offer_html("A", &o), Err(OfferError::Invalid));
    }

    #[test]
    fn sizes_are_formatted() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(1023), "1023 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(MAX_FILE_SIZE), "2.0 GB");
    }
}
