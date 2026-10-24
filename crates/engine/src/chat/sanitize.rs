//! Strict HTML policy for everything a server or another person sends (FR-1.29, spec §9):
//! basic formatting, links to http(s)/mailto, and inline images only as embedded
//! `data:image/(png|jpeg|gif|webp);base64` URLs, so nothing ever loads from the network.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use ammonia::{Builder, UrlRelative};

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

const IMAGE_TYPES: &[&str] = &["png", "jpeg", "gif", "webp"];

static POLICY: LazyLock<Builder<'static>> = LazyLock::new(|| {
    let mut builder = Builder::empty();
    builder
        .tags(TAGS.iter().copied().collect())
        .tag_attributes(HashMap::from([
            ("a", HashSet::from(["href", "title"])),
            (
                "img",
                HashSet::from(["src", "alt", "title", "width", "height"]),
            ),
        ]))
        .url_schemes(HashSet::from(["http", "https", "mailto", "data"]))
        .url_relative(UrlRelative::Deny)
        .link_rel(Some("noopener noreferrer nofollow"))
        .clean_content_tags(HashSet::from(["script", "style"]))
        .attribute_filter(|element, attribute, value| match (element, attribute) {
            ("img", "src") => is_embedded_image(value).then_some(Cow::Borrowed(value)),
            ("a", "href") if value.trim_start().to_ascii_lowercase().starts_with("data:") => None,
            _ => Some(Cow::Borrowed(value)),
        });
    builder
});

/// Sanitizes untrusted HTML (messages, comments, welcome texts).
pub fn sanitize_html(html: &str) -> String {
    POLICY.clean(html).to_string()
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
