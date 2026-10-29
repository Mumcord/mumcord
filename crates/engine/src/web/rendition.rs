//! GIF hosts (QA.43): direct links to huge GIFs are swapped for smaller renditions of the same
//! animation, and page links of GIF sites are turned into the GIF they show. Only the address
//! is derived here; every request still goes through the guarded fetcher.
//!
//! Giphy's "downsized" GIFs are the original capped in bytes with the aspect ratio kept, about
//! the size of the 400×300 chat box. They are far cheaper to animate than the original
//! `giphy.webp`, which WebKitGTK decodes frame by frame at full size (issue #109: a 14.5 MB,
//! 640×480 WebP kept a core 16 % busy, its downsized GIF 4 %). The fixed-width `200w` files
//! are not used: they are blurry in the box and Giphy squashes some of them to 16:9.

use reqwest::Url;

/// A GIF page link and what it needs to be resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GifPage {
    /// The GIF's address can be derived from the id alone.
    Giphy(String),
    /// The page's `og:image` names the GIF.
    Tenor,
}

fn host_is(url: &Url, names: &[&str]) -> bool {
    url.host_str()
        .is_some_and(|h| names.iter().any(|n| h.eq_ignore_ascii_case(n)))
}

fn is_giphy_id(id: &str) -> bool {
    (8..=30).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// `giphy.com/gifs/<slug>-<id>` (the id is the last dash segment) and `tenor.com/view/<slug>-<id>`.
pub fn gif_page(url: &Url) -> Option<GifPage> {
    let segments: Vec<&str> = url.path_segments()?.filter(|s| !s.is_empty()).collect();
    if host_is(url, &["giphy.com", "www.giphy.com"]) {
        let [kind, slug] = segments[..] else {
            return None;
        };
        if kind != "gifs" {
            return None;
        }
        let id = slug.rsplit('-').next()?;
        return is_giphy_id(id).then(|| GifPage::Giphy(id.to_owned()));
    }
    if host_is(url, &["tenor.com", "www.tenor.com"]) {
        let [.., kind, slug] = segments[..] else {
            return None;
        };
        let id = slug.rsplit('-').next()?;
        return (kind == "view" && slug.contains('-') && id.bytes().all(|b| b.is_ascii_digit()))
            .then_some(GifPage::Tenor);
    }
    None
}

/// Giphy renditions for the chat box, best first.
const GIPHY_DOWNSIZED: [&str; 2] = ["giphy-downsized-large.gif", "giphy-downsized-medium.gif"];
/// The full-size files of a Giphy GIF; only these are swapped for a downsized one.
const GIPHY_ORIGINALS: [&str; 2] = ["giphy.gif", "giphy.webp"];

fn media(path: &str) -> Option<Url> {
    Url::parse(&format!("https://media.giphy.com/media/{path}")).ok()
}

/// Bumped when the renditions picked for a link change, so images cached under the old choice
/// are fetched again (the cache keeps them for days).
const RENDITIONS_VERSION: u32 = 2;

/// Cache key of the inline image for `url`: the address itself, versioned for links whose
/// image is a rendition picked here.
pub fn cache_key(url: &Url) -> String {
    if gif_page(url).is_some() || !smaller_renditions(url).is_empty() {
        format!("renditions-v{RENDITIONS_VERSION}:{url}")
    } else {
        url.to_string()
    }
}

/// Smaller renditions to try before the address itself, best first. Empty for other links.
pub fn smaller_renditions(url: &Url) -> Vec<Url> {
    if let Some(GifPage::Giphy(id)) = gif_page(url) {
        return GIPHY_DOWNSIZED
            .iter()
            .chain(&["giphy.gif"])
            .filter_map(|file| media(&format!("{id}/{file}")))
            .collect();
    }
    let giphy_media = url.host_str().is_some_and(|h| {
        let h = h.to_ascii_lowercase();
        h.starts_with("media") && h.ends_with(".giphy.com")
    });
    if !giphy_media {
        return Vec::new();
    }
    let Some(mut segments) = url.path_segments().map(|s| s.collect::<Vec<_>>()) else {
        return Vec::new();
    };
    let Some(file) = segments.pop() else {
        return Vec::new();
    };
    // `/media/<token…>/<id>/<file>`; the token segment of newer links stays as it is. A link
    // to a rendition that is already small is loaded as posted.
    if segments.first() != Some(&"media") || segments.len() < 2 || !GIPHY_ORIGINALS.contains(&file)
    {
        return Vec::new();
    }
    GIPHY_DOWNSIZED
        .iter()
        .map(|f| {
            let mut next = url.clone();
            next.set_query(None);
            next.set_fragment(None);
            next.set_path(&format!("/{}/{f}", segments.join("/")));
            next
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(s).expect("url")
    }

    #[test]
    fn giphy_page_id_is_the_last_dash_segment() {
        assert_eq!(
            gif_page(&url(
                "https://giphy.com/gifs/meme-67-kid-kids-1ojn1S7BTXUry8elNi"
            )),
            Some(GifPage::Giphy("1ojn1S7BTXUry8elNi".into()))
        );
        assert_eq!(
            gif_page(&url("https://giphy.com/gifs/1ojn1S7BTXUry8elNi")),
            Some(GifPage::Giphy("1ojn1S7BTXUry8elNi".into()))
        );
        assert_eq!(gif_page(&url("https://giphy.com/search/cats")), None);
        assert_eq!(gif_page(&url("https://giphy.com/gifs/short-id")), None);
        assert_eq!(
            gif_page(&url("https://example.com/gifs/a-1ojn1S7BTXUry8elNi")),
            None
        );
    }

    #[test]
    fn tenor_pages_need_a_numeric_id() {
        assert_eq!(
            gif_page(&url("https://tenor.com/view/cat-dance-gif-12345678")),
            Some(GifPage::Tenor)
        );
        assert_eq!(
            gif_page(&url("https://tenor.com/de/view/cat-dance-gif-12345678")),
            Some(GifPage::Tenor)
        );
        assert_eq!(gif_page(&url("https://tenor.com/view/nothing")), None);
        assert_eq!(gif_page(&url("https://tenor.com/search/cats")), None);
    }

    #[test]
    fn giphy_pages_resolve_to_media_renditions() {
        let list = smaller_renditions(&url("https://giphy.com/gifs/x-1ojn1S7BTXUry8elNi"));
        let got: Vec<&str> = list.iter().map(Url::as_str).collect();
        assert_eq!(
            got,
            [
                "https://media.giphy.com/media/1ojn1S7BTXUry8elNi/giphy-downsized-large.gif",
                "https://media.giphy.com/media/1ojn1S7BTXUry8elNi/giphy-downsized-medium.gif",
                "https://media.giphy.com/media/1ojn1S7BTXUry8elNi/giphy.gif",
            ]
        );
    }

    #[test]
    fn giphy_media_links_try_smaller_files_in_the_same_path() {
        let long =
            "https://media4.giphy.com/media/v1.Y2lkPTc5MGI3NjEx/1ojn1S7BTXUry8elNi/giphy.gif?cid=1";
        let list = smaller_renditions(&url(long));
        let got: Vec<&str> = list.iter().map(Url::as_str).collect();
        assert_eq!(
            got,
            [
                "https://media4.giphy.com/media/v1.Y2lkPTc5MGI3NjEx/1ojn1S7BTXUry8elNi/giphy-downsized-large.gif",
                "https://media4.giphy.com/media/v1.Y2lkPTc5MGI3NjEx/1ojn1S7BTXUry8elNi/giphy-downsized-medium.gif",
            ]
        );
        assert_eq!(
            smaller_renditions(&url("https://media.giphy.com/media/abc/giphy.webp")).len(),
            2
        );
        assert!(smaller_renditions(&url("https://media.giphy.com/media/abc/giphy.png")).is_empty());
        assert!(smaller_renditions(&url("https://example.com/media/x/giphy.gif")).is_empty());
    }

    #[test]
    fn cache_keys_of_rendition_links_are_versioned() {
        let page = url("https://giphy.com/gifs/x-1ojn1S7BTXUry8elNi");
        assert_eq!(cache_key(&page), format!("renditions-v2:{page}"));
        let media = url("https://media.giphy.com/media/1ojn1S7BTXUry8elNi/giphy.gif");
        assert_ne!(cache_key(&media), media.to_string());
        let tenor = url("https://tenor.com/view/cat-dance-gif-12345678");
        assert_ne!(cache_key(&tenor), tenor.to_string());
        let plain = url("https://example.com/cat.gif");
        assert_eq!(cache_key(&plain), plain.to_string());
    }

    #[test]
    fn small_giphy_renditions_load_as_posted() {
        for file in [
            "200w.gif",
            "giphy-downsized.gif",
            "giphy-downsized-large.gif",
        ] {
            let link = format!("https://media.giphy.com/media/1ojn1S7BTXUry8elNi/{file}");
            assert!(smaller_renditions(&url(&link)).is_empty(), "{file}");
        }
    }
}
