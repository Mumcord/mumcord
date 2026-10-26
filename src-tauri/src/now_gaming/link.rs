//! The join target from the Mumble Link context of the running game (FR-2.34). Games that
//! can be joined put the server address into the context, either bare (`host:port`, as Source
//! games do) or as a `steam://connect/host:port` link. The bytes come from another process, so
//! they are only used when they form exactly one plain address; everything else is ignored.

use std::time::Instant;

use mumcord_engine::now_gaming::payload::{STEAM_CONNECT, join_link};
use mumcord_engine::positional_sources::{LinkSource, PositionSource};
use mumcord_engine::{GameSource, LocalGame};

/// Prefix of the context on the wire (`"Link\0" + context`, see the positional crate).
const WIRE_PREFIX: &[u8] = b"Link\0";
/// Longest address worth looking at: a host name (253) plus `:65535`.
const ADDRESS_MAX: usize = 260;

/// The `steam://connect/…` link for the context bytes of the positional crate, if they hold a
/// joinable address.
fn join_from_context(context: &[u8]) -> Option<String> {
    let raw = context.strip_prefix(WIRE_PREFIX).unwrap_or(context);
    if raw.len() > ADDRESS_MAX + STEAM_CONNECT.len() {
        return None;
    }
    // Games pad the context with NULs; anything after the first one is not part of it.
    let end = raw.iter().position(|b| *b == 0).unwrap_or(raw.len());
    let text = std::str::from_utf8(&raw[..end]).ok()?.trim();
    let link = if text.starts_with(STEAM_CONNECT) {
        text.to_owned()
    } else {
        format!("{STEAM_CONNECT}{text}")
    };
    join_link(&link)
}

/// Adds the join target of the Link context to what `inner` finds; without any other game,
/// a game that names itself through Link is reported on its own.
pub struct LinkJoinSource<S> {
    inner: S,
    link: LinkSource,
}

impl<S> LinkJoinSource<S> {
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            link: LinkSource::new(),
        }
    }
}

impl<S: GameSource> GameSource for LinkJoinSource<S> {
    fn detect(&mut self) -> Option<LocalGame> {
        let found = self.inner.detect();
        let fetch = self.link.fetch(Instant::now());
        let join = fetch
            .as_ref()
            .and_then(|f| join_from_context(&f.data.context));
        match found {
            Some(game) => Some(LocalGame {
                join: game.join.or(join),
                ..game
            }),
            None => {
                let name = fetch?.game?;
                Some(LocalGame {
                    name,
                    steam_app_id: None,
                    join,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wire(context: &[u8]) -> Vec<u8> {
        [WIRE_PREFIX, context].concat()
    }

    #[test]
    fn bare_address() {
        assert_eq!(
            join_from_context(&wire(b"203.0.113.7:27015")).as_deref(),
            Some("steam://connect/203.0.113.7:27015")
        );
        assert_eq!(
            join_from_context(b"play.example.org:27016").as_deref(),
            Some("steam://connect/play.example.org:27016")
        );
    }

    #[test]
    fn steam_link_and_padding() {
        assert_eq!(
            join_from_context(&wire(b"steam://connect/10.0.0.1:27015\0\0junk")).as_deref(),
            Some("steam://connect/10.0.0.1:27015")
        );
    }

    #[test]
    fn garbage_is_ignored() {
        for bad in [
            &b""[..],
            b"Link\0",
            b"hello",
            b"1.2.3.4",
            b"1.2.3.4:0",
            b"1.2.3.4:70000",
            b"1.2.3.4:27015/../x",
            b"user:pw@host:27015",
            b"http://evil.example/x:80",
            b"steam://run/440",
            b"host name:27015",
            b"\xff\xfe\x00\x01",
            b"-host:27015",
        ] {
            assert_eq!(join_from_context(&wire(bad)), None, "{bad:?}");
        }
    }

    #[test]
    fn oversized_is_ignored() {
        let mut long = vec![b'a'; 300];
        long.extend_from_slice(b":27015");
        assert_eq!(join_from_context(&wire(&long)), None);
        assert_eq!(join_from_context(&vec![b'1'; 100_000]), None);
    }
}
