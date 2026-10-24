//! YouTube and Twitch links that get a player card (FR-1.50). The player URL points to the
//! privacy-friendly embed origins (`youtube-nocookie.com`, `player.twitch.tv`,
//! `clips.twitch.tv`); the UI loads it only after the user clicks play.

use reqwest::Url;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VideoProvider {
    Youtube,
    Twitch,
}

/// A recognised video link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoLink {
    pub provider: VideoProvider,
    /// Embedded player; Twitch players additionally need `parent=<embedding host>`.
    pub player_url: String,
    /// Twitch channel name taken from the link, if it has one.
    pub channel: Option<String>,
}

/// Paths on twitch.tv that are pages, not channels.
const TWITCH_RESERVED: &[&str] = &[
    "directory",
    "downloads",
    "jobs",
    "login",
    "p",
    "search",
    "settings",
    "signup",
    "subscriptions",
    "turbo",
    "inventory",
    "wallet",
    "friends",
    "messages",
    "payments",
    "prime",
];

pub fn recognise(url: &Url) -> Option<VideoLink> {
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
        "youtube.com" | "m.youtube.com" | "music.youtube.com" => {
            let id = match segments.as_slice() {
                ["watch"] => url
                    .query_pairs()
                    .find(|(k, _)| k == "v")
                    .map(|(_, v)| v.into_owned())?,
                ["shorts" | "live" | "embed", id] => (*id).to_owned(),
                _ => return None,
            };
            youtube(&id)
        }
        "youtu.be" => match segments.as_slice() {
            [id] => youtube(id),
            _ => None,
        },
        "twitch.tv" | "m.twitch.tv" => match segments.as_slice() {
            ["videos", id] if is_digits(id) => Some(twitch(format!("video=v{id}"), None)),
            [channel, "clip", slug] if is_twitch_name(channel) && is_slug(slug) => {
                Some(clip(slug, Some(channel)))
            }
            [channel] if is_twitch_name(channel) => {
                let channel = channel.to_ascii_lowercase();
                (!TWITCH_RESERVED.contains(&channel.as_str()))
                    .then(|| twitch(format!("channel={channel}"), Some(&channel)))
            }
            _ => None,
        },
        "clips.twitch.tv" => match segments.as_slice() {
            [slug] if is_slug(slug) && *slug != "embed" => Some(clip(slug, None)),
            _ => None,
        },
        _ => None,
    }
}

fn youtube(id: &str) -> Option<VideoLink> {
    let valid = id.len() == 11
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    valid.then(|| VideoLink {
        provider: VideoProvider::Youtube,
        player_url: format!("https://www.youtube-nocookie.com/embed/{id}?autoplay=1"),
        channel: None,
    })
}

fn twitch(query: String, channel: Option<&str>) -> VideoLink {
    VideoLink {
        provider: VideoProvider::Twitch,
        player_url: format!("https://player.twitch.tv/?{query}&autoplay=true"),
        channel: channel.map(str::to_owned),
    }
}

fn clip(slug: &str, channel: Option<&str>) -> VideoLink {
    VideoLink {
        provider: VideoProvider::Twitch,
        player_url: format!("https://clips.twitch.tv/embed?clip={slug}&autoplay=true"),
        channel: channel.map(|c| c.to_ascii_lowercase()),
    }
}

fn is_digits(s: &str) -> bool {
    !s.is_empty() && s.len() <= 20 && s.bytes().all(|b| b.is_ascii_digit())
}

fn is_twitch_name(s: &str) -> bool {
    (1..=25).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

fn is_slug(s: &str) -> bool {
    (1..=100).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(u: &str) -> Option<VideoLink> {
        recognise(&Url::parse(u).expect("url"))
    }

    fn player(u: &str) -> Option<String> {
        rec(u).map(|v| v.player_url)
    }

    #[test]
    fn recognises_youtube() {
        let embed = Some("https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ?autoplay=1".into());
        assert_eq!(
            player("https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=10"),
            embed
        );
        assert_eq!(player("https://youtu.be/dQw4w9WgXcQ?si=x"), embed);
        assert_eq!(player("https://youtube.com/shorts/dQw4w9WgXcQ"), embed);
        assert_eq!(player("https://m.youtube.com/watch?v=dQw4w9WgXcQ"), embed);
        assert_eq!(player("https://www.youtube.com/watch?v=short"), None);
        assert_eq!(
            player("https://www.youtube.com/watch?v=dQw4w9WgXc%22"),
            None
        );
        assert_eq!(player("https://www.youtube.com/@channel"), None);
        assert_eq!(player("https://notyoutube.com/watch?v=dQw4w9WgXcQ"), None);
    }

    #[test]
    fn recognises_twitch() {
        let live = rec("https://www.twitch.tv/SomeStreamer").expect("channel");
        assert_eq!(live.provider, VideoProvider::Twitch);
        assert_eq!(
            live.player_url,
            "https://player.twitch.tv/?channel=somestreamer&autoplay=true"
        );
        assert_eq!(live.channel.as_deref(), Some("somestreamer"));
        assert_eq!(
            player("https://twitch.tv/videos/123456789"),
            Some("https://player.twitch.tv/?video=v123456789&autoplay=true".into())
        );
        assert_eq!(
            player("https://www.twitch.tv/streamer/clip/Funny-Clip_1"),
            Some("https://clips.twitch.tv/embed?clip=Funny-Clip_1&autoplay=true".into())
        );
        assert_eq!(
            player("https://clips.twitch.tv/FunnyClip"),
            Some("https://clips.twitch.tv/embed?clip=FunnyClip&autoplay=true".into())
        );
        assert_eq!(player("https://www.twitch.tv/directory"), None);
        assert_eq!(player("https://www.twitch.tv/videos/abc"), None);
        assert_eq!(player("https://www.twitch.tv/a/b/c/d"), None);
    }
}
