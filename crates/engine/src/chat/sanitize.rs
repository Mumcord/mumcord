//! Strict HTML policy for everything a server or another person sends (FR-1.29, spec §9):
//! basic formatting, links to http(s)/mailto and to `mumble://` servers (whose card the UI
//! builds, issue #175), and inline images only as embedded
//! `data:image/(png|jpeg|gif|webp);base64` URLs, so nothing ever loads from the network.
//!
//! Chat messages additionally keep absolute http(s) image sources ([`sanitize_message_html`]):
//! the UI never puts them into the page but asks the engine to fetch them (FR-1.55, QA.35)
//! when link previews are on. Comments and welcome texts never keep them. Message images also
//! keep a valid full-image reference ([`crate::full_image::wire::ATTRIBUTE`], FR-1.60c) and the
//! message card marker ([`CARD_ATTRIBUTE`] on `<div>`, issue #72), which the UI parses strictly,
//! and the code block language ([`CODE_LANGUAGE_ATTRIBUTE`] on `<pre>`, issue #112).

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use ammonia::{Builder, UrlRelative};

use crate::full_image::check::MAX_BYTES;
use crate::full_image::wire::{ATTRIBUTE as FULL_ATTRIBUTE, FullRef};

const TAGS: &[&str] = &[
    "a",
    "b",
    "blockquote",
    "br",
    "code",
    "del",
    "div",
    "em",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "hr",
    "i",
    "img",
    "li",
    "ol",
    "p",
    "pre",
    "s",
    "span",
    "strike",
    "strong",
    "sub",
    "sup",
    "table",
    "tbody",
    "td",
    "th",
    "thead",
    "tr",
    "u",
    "ul",
];

/// Marks a message card and its text (docs/message-cards.md); plain data for the UI.
pub const CARD_ATTRIBUTE: &str = "data-mumcord-card";

/// The language of a ``` code block (`sql`, `c++`); kept only in that shape.
pub const CODE_LANGUAGE_ATTRIBUTE: &str = "data-mumcord-lang";

const IMAGE_TYPES: &[&str] = &["png", "jpeg", "gif", "webp"];

static POLICY: LazyLock<Builder<'static>> = LazyLock::new(|| policy(false));
static MESSAGE_POLICY: LazyLock<Builder<'static>> = LazyLock::new(|| policy(true));

fn policy(message: bool) -> Builder<'static> {
    let mut image_attributes = HashSet::from(["src", "alt", "title", "width", "height"]);
    if message {
        image_attributes.insert(FULL_ATTRIBUTE);
    }
    let mut tag_attributes = HashMap::from([
        ("a", HashSet::from(["href", "title"])),
        ("img", image_attributes),
    ]);
    if message {
        tag_attributes.insert("div", HashSet::from([CARD_ATTRIBUTE]));
        tag_attributes.insert("pre", HashSet::from([CODE_LANGUAGE_ATTRIBUTE]));
    }
    let mut builder = Builder::empty();
    builder
        .tags(TAGS.iter().copied().collect())
        .tag_attributes(tag_attributes)
        // `mumble` stays a link so the chat can show it as the server card (issue #175).
        .url_schemes(HashSet::from(["http", "https", "mailto", "data", "mumble"]))
        .url_relative(UrlRelative::Deny)
        .link_rel(Some("noopener noreferrer nofollow"))
        .clean_content_tags(HashSet::from(["script", "style"]))
        .attribute_filter(
            move |element, attribute, value| match (element, attribute) {
                ("img", "src") => (is_embedded_image(value)
                    || (message && is_external_image(value)))
                .then_some(Cow::Borrowed(value)),
                ("img", FULL_ATTRIBUTE) => FullRef::parse(value, MAX_BYTES)
                    .is_some()
                    .then_some(Cow::Borrowed(value)),
                ("pre", CODE_LANGUAGE_ATTRIBUTE) => {
                    is_code_language(value).then_some(Cow::Borrowed(value))
                }
                ("a", "href") if value.trim_start().to_ascii_lowercase().starts_with("data:") => {
                    None
                }
                ("a", "href") if is_mumble_link(value) => Some(Cow::Borrowed(value)),
                // A mumble link without a server address is not a card, so it is not a link.
                ("a", "href")
                    if value
                        .trim_start()
                        .to_ascii_lowercase()
                        .starts_with("mumble:") =>
                {
                    None
                }
                _ => Some(Cow::Borrowed(value)),
            },
        );
    builder
}

/// Sanitizes untrusted HTML (comments, welcome texts): embedded images only.
pub fn sanitize_html(html: &str) -> String {
    POLICY.clean(html).to_string()
}

/// Sanitizes a chat message: like [`sanitize_html`], but absolute http(s) image sources stay
/// so the UI can show them through the engine's guarded fetch.
pub fn sanitize_message_html(html: &str) -> String {
    MESSAGE_POLICY.clean(html).to_string()
}

/// Same rule as the UI's `CODE_LANGUAGE`: a lowercase name of at most 32 characters.
fn is_code_language(value: &str) -> bool {
    let mut chars = value.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && value.len() <= 32
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "+#._-".contains(c))
}

/// A `mumble://` link needs a server address before the chat can show its card (issue #175).
fn is_mumble_link(value: &str) -> bool {
    reqwest::Url::parse(value.trim()).is_ok_and(|u| u.scheme() == "mumble" && u.host().is_some())
}

fn is_external_image(src: &str) -> bool {
    reqwest::Url::parse(src.trim())
        .is_ok_and(|u| matches!(u.scheme(), "http" | "https") && u.host().is_some())
}

fn is_embedded_image(src: &str) -> bool {
    let src = src.trim();
    let Some(rest) = src
        .get(..11)
        .filter(|p| p.eq_ignore_ascii_case("data:image/"))
        .map(|_| &src[11..])
    else {
        return false;
    };
    IMAGE_TYPES.iter().any(|t| {
        rest.get(..t.len() + 8)
            .is_some_and(|p| p.eq_ignore_ascii_case(&format!("{t};base64,")))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &str = "data:image/png;base64,iVBORw0KGgo=";

    #[test]
    fn keeps_mumble_links_only_with_a_server_address() {
        let clean =
            sanitize_html("<a href=\"mumble://voice.example.org:64739/Raid/Officers\">join</a>");
        assert!(clean.contains("href=\"mumble://voice.example.org:64739/Raid/Officers\""));
        // Without a server there is nothing the card could show.
        assert!(!sanitize_html("<a href=\"mumble:///Channel\">join</a>").contains("mumble://"));
    }

    #[test]
    fn keeps_basic_formatting_links_and_embedded_images() {
        let html = format!(
            "<b>bold</b> <i>it</i> <a href=\"https://example.org/x\">link</a> \
             <a href=\"mailto:a@b.c\">mail</a> <img src=\"{PNG}\" alt=\"pic\">"
        );
        let clean = sanitize_html(&html);
        assert!(clean.contains("<b>bold</b>"));
        assert!(clean.contains("<i>it</i>"));
        assert!(clean.contains("href=\"https://example.org/x\""));
        assert!(clean.contains("rel=\"noopener noreferrer nofollow\""));
        assert!(clean.contains("href=\"mailto:a@b.c\""));
        assert!(clean.contains(&format!("src=\"{PNG}\"")));
    }

    #[test]
    fn strips_scripts_handlers_and_styles() {
        let clean = sanitize_html(
            "<script>alert(1)</script><p onclick=\"x()\" style=\"color:red\">hi</p>\
             <iframe src=\"https://e.org\"></iframe><style>p{}</style>",
        );
        assert_eq!(clean, "<p>hi</p>");
    }

    #[test]
    fn drops_external_and_non_image_sources() {
        for src in [
            "https://tracker.example/pixel.png",
            "http://e.org/a.gif",
            "//e.org/a.gif",
            "data:image/svg+xml;base64,PHN2Zz4=",
            "data:text/html;base64,PGI+",
            "data:image/png,raw",
            "javascript:alert(1)",
            "file:///etc/passwd",
        ] {
            let clean = sanitize_html(&format!("<img src=\"{src}\">"));
            assert_eq!(clean, "<img>", "{src}");
        }
    }

    #[test]
    fn messages_keep_absolute_http_image_sources_only() {
        for src in ["https://e.org/a.gif", "http://e.org/b.png?x=1"] {
            let clean = sanitize_message_html(&format!("<img src=\"{src}\" onerror=\"x()\">"));
            assert_eq!(
                clean,
                format!("<img src=\"{}\">", src.replace('&', "&amp;"))
            );
            assert_eq!(sanitize_html(&format!("<img src=\"{src}\">")), "<img>");
        }
        for src in [
            "//e.org/a.gif",
            "/local.png",
            "javascript:alert(1)",
            "file:///etc/passwd",
            "ftp://e.org/a.png",
        ] {
            let clean = sanitize_message_html(&format!("<img src=\"{src}\">"));
            assert_eq!(clean, "<img>", "{src}");
        }
        assert_eq!(
            sanitize_message_html("<script>x</script><b>hi</b>"),
            "<b>hi</b>"
        );
    }

    #[test]
    fn messages_keep_valid_full_image_references_only() {
        let reference = format!("{}:2048", "0f".repeat(32));
        let html = format!("<img src=\"{PNG}\" data-mumcord-full=\"{reference}\">");
        assert_eq!(
            sanitize_message_html(&html),
            format!("<img src=\"{PNG}\" data-mumcord-full=\"{reference}\">")
        );
        assert_eq!(sanitize_html(&html), format!("<img src=\"{PNG}\">"));
        for bad in ["x", "0f:1", "javascript:alert(1)"] {
            let html = format!("<img src=\"{PNG}\" data-mumcord-full=\"{bad}\">");
            assert_eq!(sanitize_message_html(&html), format!("<img src=\"{PNG}\">"));
        }
    }

    #[test]
    fn messages_keep_the_card_marker_on_divs_only() {
        let html = "<div data-mumcord-card=\"v=1&amp;title=CPU\"><b>CPU</b>\
                    <div data-mumcord-card=\"body\">x</div></div>";
        assert_eq!(sanitize_message_html(html), html);
        assert_eq!(sanitize_html(html), "<div><b>CPU</b><div>x</div></div>");
        assert_eq!(
            sanitize_message_html("<span data-mumcord-card=\"body\">x</span>"),
            "<span>x</span>"
        );
    }

    #[test]
    fn messages_keep_a_valid_code_language_on_pre_only() {
        let html = "<pre data-mumcord-lang=\"c++\"><code>int x;</code></pre>";
        assert_eq!(sanitize_message_html(html), html);
        assert_eq!(sanitize_html(html), "<pre><code>int x;</code></pre>");
        for bad in ["SQL", "\"><img src=x>", "a b", "", &"a".repeat(33)] {
            let html = format!(
                "<pre data-mumcord-lang=\"{}\">x</pre>",
                bad.replace('"', "&quot;")
            );
            assert_eq!(sanitize_message_html(&html), "<pre>x</pre>", "{bad}");
        }
        assert_eq!(
            sanitize_message_html("<code data-mumcord-lang=\"sql\">x</code>"),
            "<code>x</code>"
        );
    }

    #[test]
    fn drops_dangerous_link_targets() {
        for href in [
            "javascript:alert(1)",
            " JaVaScRiPt:alert(1)",
            "data:text/html;base64,PGI+",
            "vbscript:x",
            "/relative",
        ] {
            let clean = sanitize_html(&format!("<a href=\"{href}\">x</a>"));
            assert!(!clean.contains("href"), "{href}: {clean}");
        }
    }

    #[test]
    fn accepts_all_allowed_image_types_case_insensitively() {
        for t in ["png", "jpeg", "gif", "webp", "PNG"] {
            assert!(is_embedded_image(&format!("data:image/{t};base64,AAAA")));
        }
        assert!(!is_embedded_image("data:image/bmp;base64,AAAA"));
    }
}
