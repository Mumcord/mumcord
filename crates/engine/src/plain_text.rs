//! Plain text of incoming Mumble chat HTML, and the chat trigger shape plugins may receive
//! (`chat.triggers`, e.g. `!poll 2`). [`plain_lines`] keeps line breaks for plugin chat
//! patterns (`chat.patterns`), which recognise multi-line pastes such as game item lists.

/// Longest trigger `args`, in characters.
pub const MAX_TRIGGER_ARGS_CHARS: usize = 200;

/// Mumble HTML to the plain line: tags dropped, entities decoded.
pub fn plain_text(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(start) = rest.find('<') {
        text.push_str(&rest[..start]);
        rest = match rest[start..].find('>') {
            Some(end) => &rest[start + end + 1..],
            None => "",
        };
    }
    text.push_str(rest);
    decode_entities(&text)
}

/// Mumble HTML to plain text with its line breaks: `<br>` and the end of a list item, table row,
/// heading or `<div>` become `\n`, the end of a paragraph `\n\n` (Mumcord sends blank lines as
/// paragraphs); `<head>`, `<style>` and `<script>` contents are dropped, entities decoded and the
/// result trimmed. Raw newlines only count in messages without such breaks (plain text from
/// bots); in HTML with breaks they are source formatting.
pub fn plain_lines(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let has_breaks = ["<br", "</p", "</div", "</li", "</tr", "</h"]
        .iter()
        .any(|tag| lower.contains(tag));
    let push_raw = |text: &mut String, raw: &str| {
        if has_breaks {
            text.extend(raw.chars().filter(|c| !matches!(c, '\r' | '\n')));
        } else {
            text.push_str(&raw.replace("\r\n", "\n"));
        }
    };
    let mut text = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(start) = rest.find('<') {
        push_raw(&mut text, &rest[..start]);
        let Some(end) = rest[start..].find('>') else {
            rest = "";
            break;
        };
        let tag = &rest[start + 1..start + end];
        rest = &rest[start + end + 1..];
        let name: String = tag
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '/')
            .collect::<String>()
            .to_ascii_lowercase();
        match name.as_str() {
            "br" | "br/" | "/li" | "/tr" | "/div" | "/h1" | "/h2" | "/h3" | "/h4" | "/h5"
            | "/h6" => text.push('\n'),
            "/p" => text.push_str("\n\n"),
            "head" | "style" | "script" => {
                let close = format!("</{name}");
                rest = match rest.to_ascii_lowercase().find(&close) {
                    Some(at) => &rest[at..],
                    None => "",
                };
            }
            _ => {}
        }
    }
    push_raw(&mut text, rest);
    decode_entities(&text).trim().to_owned()
}

fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let decoded = after.find(';').filter(|end| *end <= 10).and_then(|end| {
            let name = &after[..end];
            let c = match name {
                "lt" => Some('<'),
                "gt" => Some('>'),
                "amp" => Some('&'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some('\u{a0}'),
                _ => {
                    let number = name.strip_prefix('#')?;
                    let code = match number.strip_prefix(['x', 'X']) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                        None => number.parse().ok()?,
                    };
                    char::from_u32(code)
                }
            }?;
            Some((c, end))
        });
        match decoded {
            Some((c, end)) => {
                out.push(c);
                rest = &after[end + 1..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The trigger word (lowercased, with `!`) and its arguments if the message's first line has
/// the shape `!word` followed by a space or the end, with `word` 2 to 16 of a-z, 0-9 and `-`,
/// starting with a letter. Arguments are the rest of the line, trimmed, at most
/// [`MAX_TRIGGER_ARGS_CHARS`]. Every other message gives `None`, so its text goes no further.
pub fn chat_trigger(html: &str) -> Option<(String, String)> {
    let first = html.split("<br").next().unwrap_or_default();
    let text = plain_text(first);
    let line = text.trim_start().lines().next().unwrap_or_default();
    let rest = line.strip_prefix('!')?;
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let word = rest[..end].to_ascii_lowercase();
    let valid = (2..=16).contains(&word.len())
        && word.starts_with(|c: char| c.is_ascii_lowercase())
        && word
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if !valid {
        return None;
    }
    let args = rest[end..]
        .trim()
        .chars()
        .take(MAX_TRIGGER_ARGS_CHARS)
        .collect::<String>();
    Some((format!("!{word}"), args.trim_end().to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mumble_html_becomes_its_plain_line() {
        assert_eq!(
            plain_text("<p>[Dice] rolled <b>2d6</b>: 7 &#x2764; &#9733;</p>"),
            "[Dice] rolled 2d6: 7 \u{2764} \u{2605}"
        );
        assert_eq!(plain_text("a &unknown; & b"), "a &unknown; & b");
    }

    #[test]
    fn trigger_lines_are_recognised() {
        let t = |html: &str| chat_trigger(html);
        assert_eq!(t("!poll 2"), Some(("!poll".into(), "2".into())));
        assert_eq!(t("<p>!Poll   2  </p>"), Some(("!poll".into(), "2".into())));
        assert_eq!(t("!poll"), Some(("!poll".into(), String::new())));
        assert_eq!(
            t("!poll 2<br>secret second line"),
            Some(("!poll".into(), "2".into()))
        );
        assert_eq!(t("!poll 2\nsecret"), Some(("!poll".into(), "2".into())));
        assert_eq!(t("!d-2 a &amp; b"), Some(("!d-2".into(), "a & b".into())));
        for line in [
            "hello !poll 2",
            "!poll2x!",
            "!p",
            "!2poll",
            "! poll",
            "!pollpollpollpollp",
            "",
            "!pöll",
        ] {
            assert_eq!(t(line), None, "{line}");
        }
        let long = format!("!poll {}", "x".repeat(300));
        assert_eq!(
            t(&long).map(|(_, a)| a.chars().count()),
            Some(MAX_TRIGGER_ARGS_CHARS)
        );
    }

    #[test]
    fn plain_lines_keep_the_line_breaks() {
        // Plain Mumble: line breaks as <br />, sometimes followed by a raw newline.
        assert_eq!(
            plain_lines("[Rifter, PvP]<br />Damage Control II<br />\n<br />1MN Afterburner II"),
            "[Rifter, PvP]\nDamage Control II\n\n1MN Afterburner II"
        );
        // Mumcord: blank lines become paragraphs.
        assert_eq!(
            plain_lines("<p>[Rifter, PvP]<br>Damage Control II</p><p>Hammerhead II x5</p>"),
            "[Rifter, PvP]\nDamage Control II\n\nHammerhead II x5"
        );
        // A Qt rich text document: head and style dropped, source newlines ignored.
        assert_eq!(
            plain_lines(
                "<!DOCTYPE HTML><html><head><meta name=\"qrichtext\" /><style type=\"text/css\">\n\
                 p, li { white-space: pre-wrap; }\n</style></head><body>\n<p>a &amp; b</p>\n<p>c</p></body></html>"
            ),
            "a & b\n\nc"
        );
        // Plain text from a bot keeps its own newlines.
        assert_eq!(
            plain_lines("  [Rifter, x]\r\nModule\n"),
            "[Rifter, x]\nModule"
        );
        assert_eq!(plain_lines("<B>bold</B><BR>next"), "bold\nnext");
        assert_eq!(plain_lines("<script>alert(1)</script>ok"), "ok");
        assert_eq!(plain_lines("cut <b"), "cut");
    }

    /// The fit from the bug report, exactly as Mumcord's composer sent it.
    #[test]
    fn a_composed_eve_fit_matches_the_eve_fit_pattern() {
        const SENT: &str = "<p>[Mamba, *Mamba Class 2/3]<br>Drone Damage Amplifier II<br>Drone Damage Amplifier II</p><p>Pithum C-Type Medium Shield Booster<br>Pithum C-Type EM Shield Amplifier<br>Small Capacitor Booster II<br>Pith X-Type Explosive Shield Hardener</p><p>Rocket Launcher II<br>Rocket Launcher II<br>Rocket Launcher II<br>Rocket Launcher II<br>Rocket Launcher II</p><p>Small EM Shield Reinforcer II<br>Small Explosive Shield Reinforcer II<br>Small Thermal Shield Reinforcer II</p><p>Federation Navy Hobgoblin x2<br>'Augmented' Hobgoblin x2</p><p>Inferno Rage Rocket x5000<br>Navy Cap Booster 400 x16<br>Inferno Javelin Rocket x5000<br>Pith X-Type Explosive Shield Hardener x1<br>Mobile Depot x1</p>";
        let eft =
            regex::Regex::new(r"^\[[^,\]\n]{1,64},[^\]\n]{0,100}\][ \t\r]*\n").expect("pattern");
        let text = plain_lines(SENT);
        assert!(
            text.starts_with("[Mamba, *Mamba Class 2/3]\nDrone Damage Amplifier II\n"),
            "{text}"
        );
        assert!(text.contains("\n'Augmented' Hobgoblin x2\n\nInferno Rage Rocket x5000\n"));
        assert!(eft.is_match(&text));
    }
}
