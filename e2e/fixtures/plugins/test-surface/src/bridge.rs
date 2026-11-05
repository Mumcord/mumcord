//! The string protocol between the plugin and its pages (`web/probe.js`).
//!
//! Plugin → page: `run:<place>:<probe>` (`all` runs every probe that does not navigate),
//! `busy:<place>`, `hog:<place>`, `tone:<place>`; `<place>` is a page's place or `*`.
//! Page → plugin: `ready:<place>` and `result:<place>:<probe>:<status>`.

/// What a page reported. The page is untrusted, so anything else is dropped.
#[derive(Debug, PartialEq, Eq)]
pub enum PageMessage {
    Ready(String),
    Result {
        place: String,
        probe: String,
        status: String,
    },
}

const MAX_WORD: usize = 32;

fn word(text: &str) -> Option<String> {
    let valid = !text.is_empty()
        && text.len() <= MAX_WORD
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    valid.then(|| text.to_owned())
}

pub fn parse(message: &str) -> Option<PageMessage> {
    let mut parts = message.split(':');
    let message = match (parts.next()?, parts.next(), parts.next(), parts.next()) {
        ("ready", Some(place), None, None) => PageMessage::Ready(word(place)?),
        ("result", Some(place), Some(probe), Some(status)) => PageMessage::Result {
            place: word(place)?,
            probe: word(probe)?,
            status: word(status)?,
        },
        _ => return None,
    };
    parts.next().is_none().then_some(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ready_and_results() {
        assert_eq!(
            parse("ready:stage"),
            Some(PageMessage::Ready("stage".into()))
        );
        assert_eq!(
            parse("result:side:fetch-undeclared:blocked"),
            Some(PageMessage::Result {
                place: "side".into(),
                probe: "fetch-undeclared".into(),
                status: "blocked".into(),
            })
        );
    }

    #[test]
    fn drops_anything_unexpected() {
        for bad in [
            "",
            "ready",
            "ready:",
            "ready:Stage",
            "result:stage:x",
            "result:stage:x:ok:extra",
            "result:stage:<b>:ok",
            "hello:stage",
        ] {
            assert_eq!(parse(bad), None, "{bad}");
        }
    }
}
