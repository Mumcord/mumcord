//! Page metadata for link previews (FR-1.49): OpenGraph, Twitter cards, `<title>` and
//! `<meta name="description">`. Only the tokenizer runs, so nothing in the page is executed
//! or loaded; every value comes out as plain, length-limited text.

use std::cell::RefCell;

use html5ever::tendril::StrTendril;
use html5ever::tokenizer::states::RawKind;
use html5ever::tokenizer::{
    BufferQueue, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts,
};
use reqwest::Url;

const MAX_SITE: usize = 80;
const MAX_TITLE: usize = 200;
const MAX_DESCRIPTION: usize = 300;

/// What a page says about itself; all text is plain and trimmed.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PageMeta {
    pub site_name: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    /// Absolute http(s) URL of the preview image.
    pub image: Option<Url>,
}

#[derive(Default)]
struct Found {
    og_title: Option<String>,
    twitter_title: Option<String>,
    title_tag: Option<String>,
    og_description: Option<String>,
    twitter_description: Option<String>,
    description: Option<String>,
    site_name: Option<String>,
    og_image: Option<String>,
    twitter_image: Option<String>,
}

#[derive(Default)]
struct State {
    found: Found,
    /// Collecting the text of the first `<title>`.
    title: Option<String>,
    /// `<head>` is over; nothing more of interest follows.
    done: bool,
}

struct Sink(RefCell<State>);

impl TokenSink for Sink {
    type Handle = ();

    fn process_token(&self, token: Token, _line: u64) -> TokenSinkResult<()> {
        let mut state = self.0.borrow_mut();
        if state.done {
            return TokenSinkResult::Continue;
        }
        match token {
            Token::TagToken(tag) => {
                let name: &str = &tag.name;
                match (tag.kind, name) {
                    (TagKind::StartTag, "meta") => {
                        let attr = |key: &str| {
                            tag.attrs
                                .iter()
                                .find(|a| a.name.local.eq_str_ignore_ascii_case(key))
                                .map(|a| a.value.to_string())
                        };
                        if let (Some(key), Some(content)) =
                            (attr("property").or_else(|| attr("name")), attr("content"))
                        {
                            record(&mut state.found, &key.to_ascii_lowercase(), content);
                        }
                    }
                    (TagKind::StartTag, "title") if state.found.title_tag.is_none() => {
                        state.title = Some(String::new());
                        return TokenSinkResult::RawData(RawKind::Rcdata);
                    }
                    (TagKind::EndTag, "title") => {
                        if let Some(text) = state.title.take() {
                            state.found.title_tag = Some(text);
                        }
                    }
                    // Raw text: keep markup-looking strings inside them from being read as tags.
                    (TagKind::StartTag, "script") => {
                        return TokenSinkResult::RawData(RawKind::ScriptData);
                    }
                    (TagKind::StartTag, "style" | "noscript" | "template" | "textarea") => {
                        return TokenSinkResult::RawData(RawKind::Rawtext);
                    }
                    (TagKind::StartTag, "body") | (TagKind::EndTag, "head") => state.done = true,
                    _ => {}
                }
            }
            Token::CharacterTokens(text) => append_title(&mut state, &text),
            _ => {}
        }
        TokenSinkResult::Continue
    }
}

fn append_title(state: &mut State, text: &StrTendril) {
    if let Some(title) = state.title.as_mut()
        && title.len() < 4 * MAX_TITLE
    {
        title.push_str(text);
    }
}

fn record(found: &mut Found, key: &str, content: String) {
    let slot = match key {
        "og:title" => &mut found.og_title,
        "twitter:title" => &mut found.twitter_title,
        "og:description" => &mut found.og_description,
        "twitter:description" => &mut found.twitter_description,
        "description" => &mut found.description,
        "og:site_name" => &mut found.site_name,
        "og:image" | "og:image:url" | "og:image:secure_url" => &mut found.og_image,
        "twitter:image" | "twitter:image:src" => &mut found.twitter_image,
        _ => return,
    };
    if slot.is_none() {
        *slot = Some(content);
    }
}

/// Reads the metadata of an HTML page fetched from `base` (for relative image URLs).
pub fn parse(html: &[u8], base: &Url) -> PageMeta {
    let text = String::from_utf8_lossy(html);
    let input = BufferQueue::default();
    input.push_back(StrTendril::from_slice(&text));
    let tokenizer = Tokenizer::new(
        Sink(RefCell::new(State::default())),
        TokenizerOpts::default(),
    );
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    let found = tokenizer.sink.0.into_inner().found;
    let image = found
        .og_image
        .or(found.twitter_image)
        .and_then(|src| base.join(src.trim()).ok())
        .filter(|url| matches!(url.scheme(), "http" | "https"));
    PageMeta {
        site_name: found
            .site_name
            .and_then(|s| clean(&s, MAX_SITE))
            .or_else(|| {
                base.host_str()
                    .map(|h| h.trim_start_matches("www.").to_owned())
            }),
        title: [found.og_title, found.twitter_title, found.title_tag]
            .into_iter()
            .flatten()
            .find_map(|t| clean(&t, MAX_TITLE)),
        description: [
            found.og_description,
            found.twitter_description,
            found.description,
        ]
        .into_iter()
        .flatten()
        .find_map(|d| clean(&d, MAX_DESCRIPTION)),
        image,
    }
}

/// Plain one-line text: control characters dropped, whitespace collapsed, at most `max`
/// characters (with an ellipsis when cut). `None` when nothing is left.
pub fn clean(text: &str, max: usize) -> Option<String> {
    let mut out = String::new();
    let mut count = 0;
    for word in text
        .split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|w| !w.is_empty())
    {
        // Bidi overrides and other format characters could disguise the text.
        let word: String = word.chars().filter(|c| !is_format(*c)).collect();
        if word.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
            count += 1;
        }
        for c in word.chars() {
            if count >= max {
                let trimmed = out.trim_end().len();
                out.truncate(trimmed);
                out.push('…');
                return Some(out);
            }
            out.push(c);
            count += 1;
        }
    }
    (!out.is_empty()).then_some(out)
}

fn is_format(c: char) -> bool {
    matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Url {
        Url::parse("https://www.example.com/articles/42").expect("url")
    }

    #[test]
    fn reads_opengraph_first() {
        let html = br#"<!doctype html><html><head>
            <meta charset="utf-8">
            <title>Tag   title</title>
            <meta name="description" content="Plain description">
            <meta property="og:title" content="OG &amp; title">
            <meta property="og:description" content="OG
               description">
            <meta property="og:site_name" content="Example News">
            <meta property="og:image" content="/img/cover.png">
            <meta name="twitter:title" content="Twitter title">
            </head><body><meta property="og:title" content="ignored"></body></html>"#;
        let meta = parse(html, &base());
        assert_eq!(
            meta,
            PageMeta {
                site_name: Some("Example News".into()),
                title: Some("OG & title".into()),
                description: Some("OG description".into()),
                image: Some(Url::parse("https://www.example.com/img/cover.png").expect("url")),
            }
        );
    }

    #[test]
    fn falls_back_to_twitter_title_tag_and_host() {
        let html = br#"<html><head><title>Only &lt;b&gt;title&lt;/b&gt;</title>
            <meta name="twitter:description" content="tw desc">
            <meta name="twitter:image:src" content="https://cdn.example.org/t.jpg">
            <script>var s = '<meta property="og:title" content="from script">';</script>
            </head></html>"#;
        let meta = parse(html, &base());
        assert_eq!(meta.title.as_deref(), Some("Only <b>title</b>"));
        assert_eq!(meta.description.as_deref(), Some("tw desc"));
        assert_eq!(meta.site_name.as_deref(), Some("example.com"));
        assert_eq!(
            meta.image.map(String::from).as_deref(),
            Some("https://cdn.example.org/t.jpg")
        );
    }

    #[test]
    fn rejects_non_http_images_and_empty_values() {
        let html = br#"<head><meta property="og:image" content="javascript:alert(1)">
            <meta property="og:title" content="   ">
            <meta property="og:description" content="&#x202E;evil"></head>"#;
        let meta = parse(html, &base());
        assert_eq!(meta.image, None);
        assert_eq!(meta.title, None);
        assert_eq!(meta.description.as_deref(), Some("evil"));
    }

    #[test]
    fn handles_broken_markup() {
        let meta = parse(b"<title>unterminated <meta", &base());
        assert_eq!(meta.title, None);
        let meta = parse(b"\xff\xfe<title>x\xffy</title>", &base());
        assert_eq!(meta.title.as_deref(), Some("x\u{FFFD}y"));
    }

    #[test]
    fn cleans_and_limits_text() {
        assert_eq!(clean("  a \n\t b\u{0007}c ", 50).as_deref(), Some("a b c"));
        assert_eq!(clean("abcdef", 3).as_deref(), Some("abc…"));
        assert_eq!(clean("ab cd", 3).as_deref(), Some("ab…"));
        assert_eq!(clean("\u{200B}", 5), None);
    }
}
