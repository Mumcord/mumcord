//! The "🎵 Artist – Title" line at the end of the own comment, for official Mumble clients
//! that do not understand the plugin data (FR-1.58). The rest of the comment stays untouched.

/// Start of the line; also how an existing line is recognised.
pub const MARKER: &str = "🎵 ";

/// The HTML line for a track (artist and title escaped).
pub fn line(artist: &str, title: &str) -> String {
    if artist.is_empty() {
        format!("{MARKER}{}", escape(title))
    } else {
        format!("{MARKER}{} – {}", escape(artist), escape(title))
    }
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

/// The comment without a trailing now-playing line (as written by [`with_line`]).
pub fn strip(comment: &str) -> &str {
    let Some(at) = comment.rfind(MARKER) else {
        return comment;
    };
    // The line is plain escaped text; anything with markup after the marker is the user's.
    if comment[at..].contains('<') {
        return comment;
    }
    let before = &comment[..at];
    if before.trim().is_empty() {
        return "";
    }
    for br in ["<br>", "<br/>", "<br />"] {
        if let Some(rest) = before.strip_suffix(br) {
            return rest;
        }
    }
    comment
}

/// `comment` with its now-playing line replaced by `line` (or removed for `None`).
pub fn with_line(comment: &str, line: Option<&str>) -> String {
    let base = strip(comment);
    match line {
        None => base.to_owned(),
        Some(line) if base.trim().is_empty() => line.to_owned(),
        Some(line) => format!("{base}<br>{line}"),
    }
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
        let first = with_line("Hello <b>there</b>", Some(&line("Band", "One")));
        assert_eq!(first, "Hello <b>there</b><br>🎵 Band – One");
        let second = with_line(&first, Some(&line("Band", "Two")));
        assert_eq!(second, "Hello <b>there</b><br>🎵 Band – Two");
        assert_eq!(with_line(&second, None), "Hello <b>there</b>");
    }

    #[test]
    fn empty_comment_holds_only_the_line() {
        let only = with_line("", Some(&line("Band", "One")));
        assert_eq!(only, "🎵 Band – One");
        assert_eq!(with_line(&only, None), "");
    }

    #[test]
    fn user_text_with_the_note_is_kept() {
        assert_eq!(strip("I like 🎵 <i>music</i>"), "I like 🎵 <i>music</i>");
        assert_eq!(strip("Notes 🎵 here"), "Notes 🎵 here");
        assert_eq!(strip("Hi<br />🎵 A – B"), "Hi");
    }
}
