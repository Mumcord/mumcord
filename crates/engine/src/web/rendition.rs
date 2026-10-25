//! GIF hosts (QA.43): direct links to huge GIFs are swapped for smaller renditions of the same
//! animation, and page links of GIF sites are turned into the GIF they show. Only the address
//! is derived here; every request still goes through the guarded fetcher.

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

fn media(path: &str) -> Option<Url> {
    Url::parse(&format!("https://media.giphy.com/media/{path}")).ok()
}

/// Smaller renditions to try before the address itself, best first. Empty for other links.
pub fn smaller_renditions(url: &Url) -> Vec<Url> {
    if let Some(GifPage::Giphy(id)) = gif_page(url) {
        return ["giphy.webp", "200w.webp", "200w.gif", "giphy.gif"]
            .iter()
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
    // `/media/<token…>/<id>/<file>.gif`; the token segment of newer links stays as it is.
    if segments.first() != Some(&"media") || segments.len() < 2 || !file.ends_with(".gif") {
        return Vec::new();
    }
    ["giphy.webp", "200w.webp", "200w.gif"]
        .iter()
        .filter(|f| **f != file)
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
        assert_eq!(
            list[0].as_str(),
            "https://media.giphy.com/media/1ojn1S7BTXUry8elNi/giphy.webp"
        );
        assert_eq!(list.len(), 4);
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
                "https://media4.giphy.com/media/v1.Y2lkPTc5MGI3NjEx/1ojn1S7BTXUry8elNi/giphy.webp",
                "https://media4.giphy.com/media/v1.Y2lkPTc5MGI3NjEx/1ojn1S7BTXUry8elNi/200w.webp",
                "https://media4.giphy.com/media/v1.Y2lkPTc5MGI3NjEx/1ojn1S7BTXUry8elNi/200w.gif",
            ]
        );
        assert!(smaller_renditions(&url("https://media.giphy.com/media/abc/giphy.png")).is_empty());
        assert!(smaller_renditions(&url("https://example.com/media/x/giphy.gif")).is_empty());
    }
}
