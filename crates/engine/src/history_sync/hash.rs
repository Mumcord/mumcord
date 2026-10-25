//! Content hashes for deduplicating synced messages (QA.48).
//!
//! A hash covers the channel, the sender (certificate hash, or name for users without one),
//! the send time rounded down to the second and the normalized body. Mumble has no server
//! timestamps, so every client stamps messages with its own receive time; two clients see the
//! same message a few milliseconds apart and may round to neighbouring seconds. Comparisons
//! therefore use [`variants`]: the hashes for the second before, of and after a message.
//! The body is reduced to its text (tags removed, common entities decoded, whitespace
//! collapsed), so different sanitizer versions on both sides still agree.

use sha1::{Digest, Sha1};

use super::wire::{HASH_LEN, Hash};

/// Prefix that versions the hash input.
const DOMAIN: &[u8] = b"mumcord-history-v1";
/// Prefix of [`text_key`], kept apart from the content hashes.
const TEXT_DOMAIN: &[u8] = b"mumcord-history-text-v1";

/// What identifies a message for deduplication.
pub struct Content<'a> {
    pub channel: &'a str,
    pub sender_name: &'a str,
    pub sender_cert_hash: Option<&'a str>,
    pub sent_at: i64,
    pub body: &'a str,
}

/// The hash of a message.
pub fn content_hash(content: &Content<'_>) -> Hash {
    hash_at(
        content,
        &normalize(content.body),
        content.sent_at.div_euclid(1000),
    )
}

/// The hashes of a message at the second before, of and after its send time.
pub fn variants(content: &Content<'_>) -> [Hash; 3] {
    let body = normalize(content.body);
    let second = content.sent_at.div_euclid(1000);
    [-1, 0, 1].map(|d| hash_at(content, &body, second + d))
}

/// A message without its time: channel, sender and normalized body. Clients whose clocks
/// are off by more than [`variants`] covers stamp the same message seconds apart; the sync
/// compares this key within a small time window to catch those duplicates.
pub fn text_key(content: &Content<'_>) -> Hash {
    let body = normalize(content.body);
    digest(&[
        TEXT_DOMAIN,
        content.channel.as_bytes(),
        sender(content).as_bytes(),
        body.as_bytes(),
    ])
}

fn sender(content: &Content<'_>) -> String {
    match content.sender_cert_hash {
        Some(cert) => format!("cert:{cert}"),
        None => format!("name:{}", content.sender_name),
    }
}

fn hash_at(content: &Content<'_>, body: &str, second: i64) -> Hash {
    digest(&[
        DOMAIN,
        content.channel.as_bytes(),
        sender(content).as_bytes(),
        second.to_string().as_bytes(),
        body.as_bytes(),
    ])
}

fn digest(parts: &[&[u8]]) -> Hash {
    let mut sha = Sha1::new();
    for part in parts {
        sha.update(part);
        sha.update([0]);
    }
    let digest = sha.finalize();
    let mut hash = [0; HASH_LEN];
    hash.copy_from_slice(&digest[..HASH_LEN]);
    hash
}

/// The text of a message body: tags removed, common entities decoded, whitespace
/// collapsed. Bodies without text (e.g. only an image) use the whole markup instead.
pub fn normalize(body: &str) -> String {
    let mut text = String::with_capacity(body.len());
    let mut in_tag = false;
    for c in body.chars() {
        match c {
            '<' => {
                in_tag = true;
                text.push(' ');
            }
            '>' if in_tag => in_tag = false,
            _ if !in_tag => text.push(c),
            _ => {}
        }
    }
    let text = text
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    let collapsed = collapse(&text);
    if collapsed.is_empty() {
        collapse(body)
    } else {
        collapsed
    }
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn content(sent_at: i64, body: &str) -> Content<'_> {
        Content {
            channel: "Lobby",
            sender_name: "Alice",
            sender_cert_hash: Some("aa"),
            sent_at,
            body,
        }
    }

    #[test]
    fn markup_and_whitespace_do_not_change_the_hash() {
        let a = content_hash(&content(1_000, "<p>Hello   <b>world</b> &amp; you</p>"));
        let b = content_hash(&content(1_999, "Hello world & you"));
        assert_eq!(a, b, "same second, same text");
        assert_ne!(a, content_hash(&content(2_000, "Hello world & you")));
        assert_ne!(a, content_hash(&content(1_000, "Hello world")));
    }

    #[test]
    fn sender_and_channel_are_part_of_the_hash() {
        let base = content(1_000, "hi");
        let other_channel = Content {
            channel: "Raid",
            ..content(1_000, "hi")
        };
        let other_sender = Content {
            sender_cert_hash: Some("bb"),
            ..content(1_000, "hi")
        };
        let guest = Content {
            sender_cert_hash: None,
            ..content(1_000, "hi")
        };
        let h = content_hash(&base);
        assert_ne!(h, content_hash(&other_channel));
        assert_ne!(h, content_hash(&other_sender));
        assert_ne!(h, content_hash(&guest));
    }

    #[test]
    fn variants_tolerate_one_second_of_difference() {
        let seen_here = content(10_999, "hi");
        let seen_there = content(11_001, "hi");
        assert!(variants(&seen_here).contains(&content_hash(&seen_there)));
        assert!(!variants(&content(8_999, "hi")).contains(&content_hash(&seen_there)));
    }

    #[test]
    fn text_key_ignores_time_but_not_sender_channel_or_text() {
        let key = text_key(&content(1_000, "<b>hi</b>"));
        assert_eq!(key, text_key(&content(9_000, "hi")));
        assert_ne!(key, text_key(&content(1_000, "hey")));
        let other_sender = Content {
            sender_cert_hash: Some("bb"),
            ..content(1_000, "hi")
        };
        assert_ne!(key, text_key(&other_sender));
        let other_channel = Content {
            channel: "Raid",
            ..content(1_000, "hi")
        };
        assert_ne!(key, text_key(&other_channel));
    }

    #[test]
    fn image_only_bodies_keep_their_markup() {
        assert_eq!(normalize("<img src=\"a.png\">"), "<img src=\"a.png\">");
        assert_ne!(
            content_hash(&content(1_000, "<img src=\"a.png\">")),
            content_hash(&content(1_000, "<img src=\"b.png\">"))
        );
    }
}
