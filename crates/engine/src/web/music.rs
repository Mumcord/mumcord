//! Music service links that get a music card (issue #262). Spotify, SoundCloud and YouTube Music
//! have an official, documented embed; the UI loads it only after the user clicks play. Apple
//! Music, Tidal, Deezer and Bandcamp get a card from their page metadata without a player,
//! because their embeds are not documented (or, for Bandcamp, need an id the link does not
//! carry). Every link is matched against fixed host and path patterns, and a player URL is only
//! ever built from the validated parts, never copied from the link.

use reqwest::Url;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MusicService {
    Spotify,
    Soundcloud,
    YoutubeMusic,
    AppleMusic,
    Tidal,
    Deezer,
    Bandcamp,
    /// Any other page that says it is music (`og:type` `music.*`).
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MusicKind {
    Track,
    Album,
    Playlist,
    Artist,
    Episode,
    Show,
}

impl MusicKind {
    /// The kind an OpenGraph `music.*` type stands for.
    pub fn from_og_type(og_type: &str) -> Option<Self> {
        match og_type {
            "music.song" => Some(Self::Track),
            "music.album" => Some(Self::Album),
            "music.playlist" => Some(Self::Playlist),
            _ => None,
        }
    }
}

/// A recognised music link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MusicLink {
    pub service: MusicService,
    pub kind: MusicKind,
    /// Official embed, loaded only after a click; `None` for services without one.
    pub player_url: Option<String>,
    /// Where the card's title and cover come from: the link itself, or for YouTube Music the
    /// same item on youtube.com (YouTube's oEmbed only knows that host).
    pub meta_url: Url,
}

/// The documented oEmbed endpoint of a service, if it has one.
pub fn oembed_endpoint(service: MusicService) -> Option<&'static str> {
    match service {
        MusicService::Spotify => Some("https://open.spotify.com/oembed"),
        MusicService::Soundcloud => Some("https://soundcloud.com/oembed"),
        MusicService::YoutubeMusic => Some("https://www.youtube.com/oembed"),
        _ => None,
    }
}

/// First path segments on soundcloud.com that are pages, not artists.
const SOUNDCLOUD_RESERVED: &[&str] = &[
    "charts",
    "discover",
    "feed",
    "jobs",
    "logout",
    "messages",
    "mobile",
    "notifications",
    "pages",
    "people",
    "search",
    "settings",
    "signin",
    "signup",
    "stations",
    "stream",
    "tags",
    "terms-of-use",
    "upload",
    "you",
];

/// Second path segments on soundcloud.com that are an artist's sub-pages, not tracks.
const SOUNDCLOUD_PAGES: &[&str] = &[
    "albums",
    "comments",
    "followers",
    "following",
    "likes",
    "popular-tracks",
    "reposts",
    "sets",
    "spotlight",
    "tracks",
];

pub fn recognise(url: &Url) -> Option<MusicLink> {
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let host = url.host_str()?.to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    let segments: Vec<&str> = url
        .path_segments()
        .map(|s| s.filter(|p| !p.is_empty()).collect())
        .unwrap_or_default();
    match host {
        "open.spotify.com" => spotify(&segments),
        "soundcloud.com" | "m.soundcloud.com" => soundcloud(&segments),
        "music.youtube.com" => youtube_music(url, &segments),
        "music.apple.com" => apple_music(url, &segments),
        "tidal.com" | "listen.tidal.com" => tidal(url, &segments),
        "deezer.com" => deezer(url, &segments),
        _ if host.ends_with(".bandcamp.com") => bandcamp(url, host, &segments),
        _ => None,
    }
}

fn spotify(segments: &[&str]) -> Option<MusicLink> {
    // Links may carry an `intl-xx` locale prefix.
    let rest = match segments {
        [first, rest @ ..] if first.starts_with("intl-") => rest,
        all => all,
    };
    let [kind, id] = rest else { return None };
    let kind_of = match *kind {
        "track" => MusicKind::Track,
        "album" => MusicKind::Album,
        "playlist" => MusicKind::Playlist,
        "artist" => MusicKind::Artist,
        "episode" => MusicKind::Episode,
        "show" => MusicKind::Show,
        _ => return None,
    };
    if !((10..=30).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric())) {
        return None;
    }
    Some(MusicLink {
        service: MusicService::Spotify,
        kind: kind_of,
        player_url: Some(format!("https://open.spotify.com/embed/{kind}/{id}")),
        meta_url: Url::parse(&format!("https://open.spotify.com/{kind}/{id}")).ok()?,
    })
}

fn soundcloud(segments: &[&str]) -> Option<MusicLink> {
    let (artist, kind, path) = match segments {
        [artist, "sets", set] if is_slug(set) => {
            (*artist, MusicKind::Playlist, format!("{artist}/sets/{set}"))
        }
        [artist, track] if is_slug(track) && !SOUNDCLOUD_PAGES.contains(track) => {
            (*artist, MusicKind::Track, format!("{artist}/{track}"))
        }
        _ => return None,
    };
    if !is_slug(artist) || SOUNDCLOUD_RESERVED.contains(&artist) {
        return None;
    }
    let link = Url::parse(&format!("https://soundcloud.com/{path}")).ok()?;
    let player = Url::parse_with_params(
        "https://w.soundcloud.com/player/",
        [
            ("url", link.as_str()),
            ("auto_play", "true"),
            ("visual", "false"),
        ],
    )
    .ok()?;
    Some(MusicLink {
        service: MusicService::Soundcloud,
        kind,
        player_url: Some(player.into()),
        meta_url: link,
    })
}

/// YouTube Music has no embed of its own; the standard (privacy-enhanced) YouTube player
/// plays the same video or playlist.
fn youtube_music(url: &Url, segments: &[&str]) -> Option<MusicLink> {
    let query = |key: &str| {
        url.query_pairs()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.into_owned())
    };
    let (kind, player, meta) = match segments {
        ["watch"] => {
            let id = query("v").filter(|id| is_youtube_id(id))?;
            (
                MusicKind::Track,
                format!("https://www.youtube-nocookie.com/embed/{id}?autoplay=1"),
                format!("https://www.youtube.com/watch?v={id}"),
            )
        }
        ["playlist"] => {
            let list = query("list").filter(|l| is_playlist_id(l))?;
            (
                MusicKind::Playlist,
                format!(
                    "https://www.youtube-nocookie.com/embed/videoseries?list={list}&autoplay=1"
                ),
                format!("https://www.youtube.com/playlist?list={list}"),
            )
        }
        _ => return None,
    };
    Some(MusicLink {
        service: MusicService::YoutubeMusic,
        kind,
        player_url: Some(player),
        meta_url: Url::parse(&meta).ok()?,
    })
}

/// `music.apple.com/{country}/{album|playlist|song}/{name}/{id}`; the name may be missing. An
/// album link with `?i=` points at one of its songs.
fn apple_music(url: &Url, segments: &[&str]) -> Option<MusicLink> {
    let (country, kind, id) = match segments {
        [country, kind, _, id] | [country, kind, id] => (*country, *kind, *id),
        _ => return None,
    };
    if country.len() != 2 || !country.bytes().all(|b| b.is_ascii_alphabetic()) {
        return None;
    }
    let kind = match kind {
        "album" if is_digits(id) => {
            if url.query_pairs().any(|(k, v)| k == "i" && is_digits(&v)) {
                MusicKind::Track
            } else {
                MusicKind::Album
            }
        }
        "song" if is_digits(id) => MusicKind::Track,
        "playlist" if id.starts_with("pl.") && is_slug(&id[3..]) => MusicKind::Playlist,
        _ => return None,
    };
    fallback(MusicService::AppleMusic, kind, url)
}

/// `tidal.com/{track|album|playlist}/{id}`, also under `/browse/` and on listen.tidal.com.
fn tidal(url: &Url, segments: &[&str]) -> Option<MusicLink> {
    let rest = match segments {
        ["browse", rest @ ..] => rest,
        all => all,
    };
    let kind = match rest {
        ["track", id] if is_digits(id) => MusicKind::Track,
        ["album", id] if is_digits(id) => MusicKind::Album,
        ["playlist", id] if is_uuid(id) => MusicKind::Playlist,
        _ => return None,
    };
    fallback(MusicService::Tidal, kind, url)
}

/// `deezer.com/{language}/{track|album|playlist}/{id}`; the language may be missing.
fn deezer(url: &Url, segments: &[&str]) -> Option<MusicLink> {
    let rest = match segments {
        [lang, kind, id]
            if (2..=5).contains(&lang.len())
                && lang.bytes().all(|b| b.is_ascii_lowercase() || b == b'-') =>
        {
            [*kind, *id]
        }
        [kind, id] => [*kind, *id],
        _ => return None,
    };
    let kind = match rest {
        ["track", id] if is_digits(id) => MusicKind::Track,
        ["album", id] if is_digits(id) => MusicKind::Album,
        ["playlist", id] if is_digits(id) => MusicKind::Playlist,
        _ => return None,
    };
    fallback(MusicService::Deezer, kind, url)
}

/// `{artist}.bandcamp.com/{track|album}/{name}`.
fn bandcamp(url: &Url, host: &str, segments: &[&str]) -> Option<MusicLink> {
    let artist = host.strip_suffix(".bandcamp.com")?;
    if !is_slug(artist) {
        return None;
    }
    let kind = match segments {
        ["track", name] if is_slug(name) => MusicKind::Track,
        ["album", name] if is_slug(name) => MusicKind::Album,
        _ => return None,
    };
    fallback(MusicService::Bandcamp, kind, url)
}

fn fallback(service: MusicService, kind: MusicKind, url: &Url) -> Option<MusicLink> {
    Some(MusicLink {
        service,
        kind,
        player_url: None,
        meta_url: url.clone(),
    })
}

fn is_youtube_id(s: &str) -> bool {
    s.len() == 11
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn is_playlist_id(s: &str) -> bool {
    (10..=64).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn is_digits(s: &str) -> bool {
    !s.is_empty() && s.len() <= 20 && s.bytes().all(|b| b.is_ascii_digit())
}

fn is_uuid(s: &str) -> bool {
    s.len() == 36
        && s.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_hexdigit(),
        })
}

fn is_slug(s: &str) -> bool {
    (1..=100).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(u: &str) -> Option<MusicLink> {
        recognise(&Url::parse(u).expect("url"))
    }

    fn player(u: &str) -> Option<String> {
        rec(u).and_then(|m| m.player_url)
    }

    fn service(u: &str) -> Option<(MusicService, MusicKind)> {
        rec(u).map(|m| (m.service, m.kind))
    }

    #[test]
    fn recognises_spotify() {
        let track =
            rec("https://open.spotify.com/track/4uLU6hMCjMI75M1A2tKUQC?si=abc123").expect("track");
        assert_eq!(track.service, MusicService::Spotify);
        assert_eq!(track.kind, MusicKind::Track);
        assert_eq!(
            track.player_url.as_deref(),
            Some("https://open.spotify.com/embed/track/4uLU6hMCjMI75M1A2tKUQC")
        );
        // The tracking parameter is not sent to the service.
        assert_eq!(
            track.meta_url.as_str(),
            "https://open.spotify.com/track/4uLU6hMCjMI75M1A2tKUQC"
        );
        for (kind, expected) in [
            ("album", MusicKind::Album),
            ("playlist", MusicKind::Playlist),
            ("artist", MusicKind::Artist),
            ("episode", MusicKind::Episode),
            ("show", MusicKind::Show),
        ] {
            let link = rec(&format!(
                "https://open.spotify.com/{kind}/37i9dQZF1DXcBWIGoYBM5M"
            ));
            assert_eq!(link.expect(kind).kind, expected);
        }
        assert_eq!(
            player("https://open.spotify.com/intl-de/track/4uLU6hMCjMI75M1A2tKUQC"),
            Some("https://open.spotify.com/embed/track/4uLU6hMCjMI75M1A2tKUQC".into())
        );
        assert_eq!(rec("https://open.spotify.com/track/short"), None);
        assert_eq!(
            rec("https://open.spotify.com/track/4uLU6hMCjMI75M1A2tKUQ%22"),
            None
        );
        assert_eq!(rec("https://open.spotify.com/user/someone"), None);
        assert_eq!(rec("https://open.spotify.com/"), None);
        assert_eq!(
            rec("https://spotify.com.evil.example/track/4uLU6hMCjMI75M1A2tKUQC"),
            None
        );
    }

    #[test]
    fn recognises_soundcloud() {
        let track = rec("https://soundcloud.com/lumen-park/midnight-signals?si=x&utm_source=y")
            .expect("track");
        assert_eq!(track.service, MusicService::Soundcloud);
        assert_eq!(track.kind, MusicKind::Track);
        assert_eq!(
            track.player_url.as_deref(),
            Some(
                "https://w.soundcloud.com/player/?url=https%3A%2F%2Fsoundcloud.com%2Flumen-park%2Fmidnight-signals&auto_play=true&visual=false"
            )
        );
        assert_eq!(
            track.meta_url.as_str(),
            "https://soundcloud.com/lumen-park/midnight-signals"
        );
        assert_eq!(
            service("https://m.soundcloud.com/lumen-park/sets/night-drive"),
            Some((MusicService::Soundcloud, MusicKind::Playlist))
        );
        assert_eq!(rec("https://soundcloud.com/lumen-park"), None);
        assert_eq!(rec("https://soundcloud.com/lumen-park/likes"), None);
        assert_eq!(rec("https://soundcloud.com/discover/sets/x"), None);
        assert_eq!(rec("https://soundcloud.com/a/b/c"), None);
        assert_eq!(rec("https://soundcloud.com/a%22/b"), None);
        assert_eq!(rec("https://soundcloud.com.evil.example/a/b"), None);
        assert_eq!(rec("https://notsoundcloud.com/a/b"), None);
    }

    #[test]
    fn recognises_youtube_music_with_the_standard_player() {
        let song = rec("https://music.youtube.com/watch?v=dQw4w9WgXcQ&si=x").expect("song");
        assert_eq!(song.service, MusicService::YoutubeMusic);
        assert_eq!(
            song.player_url.as_deref(),
            Some("https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ?autoplay=1")
        );
        assert_eq!(
            song.meta_url.as_str(),
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
        );
        let list =
            rec("https://music.youtube.com/playlist?list=PLx0sYbCqOb8TBPRdmBHs5Iftvv9TPboYG")
                .expect("playlist");
        assert_eq!(list.kind, MusicKind::Playlist);
        assert_eq!(
            list.player_url.as_deref(),
            Some(
                "https://www.youtube-nocookie.com/embed/videoseries?list=PLx0sYbCqOb8TBPRdmBHs5Iftvv9TPboYG&autoplay=1"
            )
        );
        assert_eq!(rec("https://music.youtube.com/watch?v=short"), None);
        assert_eq!(rec("https://music.youtube.com/playlist?list=a%22b"), None);
        assert_eq!(rec("https://music.youtube.com/channel/UCabc"), None);
        assert_eq!(rec("https://www.youtube.com/watch?v=dQw4w9WgXcQ"), None);
    }

    #[test]
    fn falls_back_for_services_without_a_documented_embed() {
        let album = rec("https://music.apple.com/us/album/glass-harbour/1700000000").expect("a");
        assert_eq!(album.service, MusicService::AppleMusic);
        assert_eq!(album.kind, MusicKind::Album);
        assert_eq!(album.player_url, None);
        assert_eq!(
            service("https://music.apple.com/de/album/glass-harbour/1700000000?i=1700000001"),
            Some((MusicService::AppleMusic, MusicKind::Track))
        );
        assert_eq!(
            service("https://music.apple.com/us/playlist/night/pl.u-abc123"),
            Some((MusicService::AppleMusic, MusicKind::Playlist))
        );
        assert_eq!(rec("https://music.apple.com/us/browse"), None);
        assert_eq!(rec("https://music.apple.com/usa/album/x/1"), None);
        assert_eq!(
            service("https://tidal.com/browse/track/12345678"),
            Some((MusicService::Tidal, MusicKind::Track))
        );
        assert_eq!(
            service("https://listen.tidal.com/playlist/6b1e0d0a-0000-4000-8000-000000000000"),
            Some((MusicService::Tidal, MusicKind::Playlist))
        );
        assert_eq!(rec("https://tidal.com/playlist/not-a-uuid"), None);
        assert_eq!(
            service("https://www.deezer.com/en/album/302127"),
            Some((MusicService::Deezer, MusicKind::Album))
        );
        assert_eq!(
            service("https://deezer.com/track/3135556"),
            Some((MusicService::Deezer, MusicKind::Track))
        );
        assert_eq!(rec("https://www.deezer.com/en/profile/1"), None);
        let bandcamp = rec("https://lumenpark.bandcamp.com/album/glass-harbour").expect("b");
        assert_eq!(
            (bandcamp.service, bandcamp.kind, bandcamp.player_url),
            (MusicService::Bandcamp, MusicKind::Album, None)
        );
        assert_eq!(rec("https://bandcamp.com/album/x"), None);
        assert_eq!(rec("https://lumenpark.bandcamp.com/merch"), None);
        assert_eq!(
            rec("https://lumenpark.bandcamp.com.evil.example/album/x"),
            None
        );
        assert_eq!(rec("https://evilbandcamp.com/album/x"), None);
    }

    #[test]
    fn ignores_other_schemes_and_hosts() {
        assert_eq!(
            rec("ftp://open.spotify.com/track/4uLU6hMCjMI75M1A2tKUQC"),
            None
        );
        assert_eq!(
            rec("https://example.com/track/4uLU6hMCjMI75M1A2tKUQC"),
            None
        );
    }

    #[test]
    fn maps_opengraph_music_types() {
        assert_eq!(
            MusicKind::from_og_type("music.song"),
            Some(MusicKind::Track)
        );
        assert_eq!(
            MusicKind::from_og_type("music.album"),
            Some(MusicKind::Album)
        );
        assert_eq!(
            MusicKind::from_og_type("music.playlist"),
            Some(MusicKind::Playlist)
        );
        assert_eq!(MusicKind::from_og_type("website"), None);
    }
}
