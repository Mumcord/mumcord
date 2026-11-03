//! File-name sanitizing (FR-7.23). Names come from other clients and end up in the UI and on
//! disk, so the result is safe on Windows, macOS and Linux alike.

const MAX_CHARS: usize = 200;

const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Bidirectional control characters; they let a name pretend to end in another extension.
fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// Returns a display- and save-safe version of `name`; never empty.
pub fn sanitize_file_name(name: &str) -> String {
    let last_part = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let cleaned: String = last_part
        .chars()
        .filter(|c| !is_bidi_control(*c))
        .map(|c| {
            if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = trim_edges(&cleaned);
    let guarded = guard_reserved(trimmed);
    let cut = cut_keeping_extension(&guarded, MAX_CHARS);
    let result = trim_edges(&cut);
    if result.is_empty() {
        "file".to_owned()
    } else {
        result.to_owned()
    }
}

/// Leading dots would hide the file, trailing dots and spaces are dropped by Windows.
fn trim_edges(s: &str) -> &str {
    s.trim_start_matches('.').trim_end_matches(['.', ' '])
}

/// Windows refuses `CON`, `NUL.txt` and friends, so they get a `_` prefix.
fn guard_reserved(name: &str) -> String {
    let stem = name.split('.').next().unwrap_or_default().trim_end();
    if RESERVED.iter().any(|r| stem.eq_ignore_ascii_case(r)) {
        format!("_{name}")
    } else {
        name.to_owned()
    }
}

fn cut_keeping_extension(name: &str, max: usize) -> String {
    if name.chars().count() <= max {
        return name.to_owned();
    }
    let split = name
        .rfind('.')
        .filter(|i| name[*i..].chars().count() < max)
        .unwrap_or(name.len());
    let (stem, extension) = name.split_at(split);
    let keep = max - extension.chars().count();
    let stem: String = stem.chars().take(keep).collect();
    format!("{stem}{extension}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table() {
        let cases: [(&str, &str); 22] = [
            ("report.pdf", "report.pdf"),
            ("../../evil\x07.txt", "evil_.txt"),
            ("C:\\Users\\x\\a.txt", "a.txt"),
            ("/etc/passwd", "passwd"),
            ("a/b\\c.txt", "c.txt"),
            ("photo\u{202E}gpj.exe", "photogpj.exe"),
            ("a\u{2066}b\u{200F}.txt", "ab.txt"),
            ("we?ird*na<m>e|\"x\":.txt", "we_ird_na_m_e__x__.txt"),
            ("CON", "_CON"),
            ("nul.txt", "_nul.txt"),
            ("Com1.tar.gz", "_Com1.tar.gz"),
            ("console.txt", "console.txt"),
            (".bashrc", "bashrc"),
            ("...hidden", "hidden"),
            ("name. . ", "name"),
            ("trailing.", "trailing"),
            ("", "file"),
            ("...", "file"),
            ("a/", "file"),
            ("\u{202E}", "file"),
            ("tab\there", "tab_here"),
            ("ünï cödé.txt", "ünï cödé.txt"),
        ];
        for (input, expected) in cases {
            assert_eq!(sanitize_file_name(input), expected, "input {input:?}");
        }
    }

    #[test]
    fn long_names_keep_their_extension() {
        let name = format!("{}.pdf", "a".repeat(300));
        let out = sanitize_file_name(&name);
        assert_eq!(out.chars().count(), 200);
        assert!(out.ends_with(".pdf"));
    }

    #[test]
    fn long_multibyte_names_are_cut_on_characters() {
        let name = format!("{}.txt", "ä".repeat(300));
        let out = sanitize_file_name(&name);
        assert_eq!(out.chars().count(), 200);
        assert!(out.ends_with(".txt"));
    }

    #[test]
    fn huge_extension_is_cut_like_a_name() {
        let out = sanitize_file_name(&format!("a.{}", "b".repeat(400)));
        assert_eq!(out.chars().count(), 200);
    }

    #[test]
    fn result_is_stable() {
        for input in ["../../evil\x07.txt", "CON", "x\u{202E}y. "] {
            let once = sanitize_file_name(input);
            assert_eq!(sanitize_file_name(&once), once);
        }
    }
}
