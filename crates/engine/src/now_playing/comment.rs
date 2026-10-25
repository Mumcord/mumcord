//! Status lines at the end of the own comment, for official Mumble clients that do not
//! understand the plugin data: "🎵 Artist – Title" (FR-1.58) and "🎮 Game" (FR-1.59). Each
//! feature owns one line, recognised by its marker; the lines always come in [`MARKERS`] order
//! and the rest of the comment stays untouched.

/// Start of the now-playing line; also how an existing line is recognised.
pub const MARKER: &str = "🎵 ";
/// Start of the now-gaming line.
pub const GAME_MARKER: &str = "🎮 ";
/// Every status line marker, in the order the lines are written.
pub const MARKERS: [&str; 2] = [MARKER, GAME_MARKER];

/// The HTML line for a track (artist and title escaped).
pub fn line(artist: &str, title: &str) -> String {
    if artist.is_empty() {
        format!("{MARKER}{}", escape(title))
    } else {
        format!("{MARKER}{} – {}", escape(artist), escape(title))
    }
}

/// The HTML line for a game (name escaped).
pub fn game_line(name: &str) -> String {
    format!("{GAME_MARKER}{}", escape(name))
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

/// The user's part of the comment and the status lines after it.
fn split(comment: &str) -> (&str, Vec<&str>) {
    let mut base = comment;
    let mut lines = Vec::new();
    loop {
        let br = ["<br>", "<br/>", "<br />"]
            .iter()
            .filter_map(|br| base.rfind(br).map(|at| (at, at + br.len())))
            .max();
        let (rest, segment) = match br {
            Some((start, end)) => (&base[..start], &base[end..]),
            None => ("", base),
        };
        let segment = segment.trim_start();
        // A line is plain escaped text; anything with markup is the user's.
        if !MARKERS.iter().any(|m| segment.starts_with(m)) || segment.contains('<') {
            break;
        }
        lines.push(segment);
        base = rest;
        if br.is_none() {
            break;
        }
    }
    lines.reverse();
    (base, lines)
}

/// The comment without its trailing status lines (as written by [`with_line`]).
pub fn strip(comment: &str) -> &str {
    split(comment).0
}

/// `comment` with its status line for `marker` replaced by `line` (or removed for `None`);
/// the other status lines stay.
pub fn with_line(comment: &str, marker: &str, line: Option<&str>) -> String {
    let (base, lines) = split(comment);
    let mut lines: Vec<&str> = lines
        .into_iter()
        .filter(|l| !l.starts_with(marker))
        .collect();
    lines.extend(line);
    lines.sort_by_key(|l| MARKERS.iter().position(|m| l.starts_with(m)));
    let mut out = if lines.is_empty() || !base.trim().is_empty() {
        base.to_owned()
    } else {
        String::new()
    };
    for line in lines {
        if !out.is_empty() {
            out.push_str("<br>");
        }
        out.push_str(line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_escapes_markup() {
        assert_eq!(line("A&B", "<x>"), "🎵 A&amp;B – &lt;x&gt;");
        assert_eq!(line("", "Solo"), "🎵 Solo");
    }

    #[test]
    fn adds_replaces_and_removes_the_line() {
        let first = with_line("Hello <b>there</b>", MARKER, Some(&line("Band", "One")));
        assert_eq!(first, "Hello <b>there</b><br>🎵 Band – One");
        let second = with_line(&first, MARKER, Some(&line("Band", "Two")));
        assert_eq!(second, "Hello <b>there</b><br>🎵 Band – Two");
        assert_eq!(with_line(&second, MARKER, None), "Hello <b>there</b>");
    }

    #[test]
    fn empty_comment_holds_only_the_line() {
        let only = with_line("", MARKER, Some(&line("Band", "One")));
        assert_eq!(only, "🎵 Band – One");
        assert_eq!(with_line(&only, MARKER, None), "");
    }

    #[test]
    fn user_text_with_the_note_is_kept() {
        assert_eq!(strip("I like 🎵 <i>music</i>"), "I like 🎵 <i>music</i>");
        assert_eq!(strip("Notes 🎵 here"), "Notes 🎵 here");
        assert_eq!(strip("Hi<br />🎵 A – B"), "Hi");
    }

    #[test]
    fn music_and_game_lines_live_side_by_side() {
        let game = game_line("Factorio & <Friends>");
        assert_eq!(game, "🎮 Factorio &amp; &lt;Friends&gt;");
        let both = with_line("Hi", GAME_MARKER, Some(&game));
        let both = with_line(&both, MARKER, Some(&line("Band", "One")));
        assert_eq!(both, format!("Hi<br>🎵 Band – One<br>{game}"));
        assert_eq!(strip(&both), "Hi");
        let music_gone = with_line(&both, MARKER, None);
        assert_eq!(music_gone, format!("Hi<br>{game}"));
        assert_eq!(with_line(&music_gone, GAME_MARKER, None), "Hi");
        let only_game = with_line("", GAME_MARKER, Some(&game));
        assert_eq!(only_game, game);
        assert_eq!(with_line(&only_game, GAME_MARKER, None), "");
        assert_eq!(strip("Play 🎮 <b>now</b>"), "Play 🎮 <b>now</b>");
        let odd = with_line("Hi", GAME_MARKER, Some(&game_line("🎵 Rhythm")));
        assert_eq!(with_line(&odd, GAME_MARKER, None), "Hi");
    }
}
