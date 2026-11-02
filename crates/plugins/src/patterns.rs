//! Chat patterns (`chat.patterns`): the second narrow way a plugin hears chat text, next to
//! [`crate::triggers`]. A plugin declares up to [`MAX_CHAT_PATTERNS`] regular expressions in its
//! manifest (e.g. the first line of a pasted EVE Online fit, `[Ship, Fit name]`); a channel
//! message in the user's channel whose plain text matches one reaches that plugin as
//! `chat-match`.
//!
//! The patterns come from a stranger, so they are held to the `regex` crate's linear-time engine
//! with small size limits, and each must be anchored at the start of the message: a pattern
//! decides from the beginning of a message whether it is meant, it never scans chat for words.
//! The engine only forwards matching messages (never from blocked users); here the matching
//! plugins are found and each sender is rate-limited. Messages that match nothing leave no
//! trace: they are neither stored, logged nor counted.

use std::time::Duration;

use regex::{Regex, RegexBuilder, RegexSet, RegexSetBuilder};
use regex_syntax::hir::Look;

/// At most this many chat patterns per plugin.
pub const MAX_CHAT_PATTERNS: usize = 2;
/// Longest pattern source, in characters.
pub const MAX_PATTERN_CHARS: usize = 256;
/// Longest user-facing pattern description (manifest or translation), in characters.
pub const MAX_PATTERN_DESCRIPTION_CHARS: usize = 80;
/// Longest text a plugin gets, in bytes; longer messages are cut at a character boundary.
pub const MAX_MATCH_TEXT_BYTES: usize = 4 * 1024;
/// Most matches one sender may cause per [`RATE_WINDOW`]; more are dropped.
pub const RATE_LIMIT: usize = 3;
pub const RATE_WINDOW: Duration = Duration::from_secs(10);

/// Compiled program size per pattern: room for a few Unicode classes with bounded repetitions
/// (an EFT fit header does not fit in 64 KiB), far below the crate default of 10 MiB.
const SIZE_LIMIT: usize = 1024 * 1024;
/// Lazy DFA cache per pattern.
const DFA_SIZE_LIMIT: usize = 2 * 1024 * 1024;
/// Deepest nesting of groups and repetitions.
const NEST_LIMIT: u32 = 32;

/// Why a chat pattern is refused; shown to plugin developers.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PatternError {
    #[error("longer than {MAX_PATTERN_CHARS} characters")]
    TooLong,
    #[error("not a valid regular expression")]
    Invalid,
    #[error("must be anchored at the message start with ^ or \\A (not in multi-line mode)")]
    NotAnchored,
    #[error("too large once compiled")]
    TooLarge,
}

/// Checks and compiles one pattern from a manifest.
pub fn compile(pattern: &str) -> Result<Regex, PatternError> {
    if pattern.chars().count() > MAX_PATTERN_CHARS {
        return Err(PatternError::TooLong);
    }
    let hir = regex_syntax::ParserBuilder::new()
        .nest_limit(NEST_LIMIT)
        .build()
        .parse(pattern)
        .map_err(|_| PatternError::Invalid)?;
    // Every match must begin with `^`/`\A` at the start of the text; `(?m)^` (start of any line)
    // and alternations with an unanchored branch do not count.
    if !hir.properties().look_set_prefix().contains(Look::Start) {
        return Err(PatternError::NotAnchored);
    }
    RegexBuilder::new(pattern)
        .size_limit(SIZE_LIMIT)
        .dfa_size_limit(DFA_SIZE_LIMIT)
        .nest_limit(NEST_LIMIT)
        .build()
        .map_err(|e| match e {
            regex::Error::CompiledTooBig(_) => PatternError::TooLarge,
            _ => PatternError::Invalid,
        })
}

/// One set of every active plugin's patterns, for the engine's filter. `None` if there are none
/// (the engine then forwards nothing) or if they cannot be compiled together.
pub fn pattern_set<'a>(patterns: impl IntoIterator<Item = &'a str>) -> Option<RegexSet> {
    let patterns: Vec<&str> = patterns.into_iter().collect();
    if patterns.is_empty() {
        return None;
    }
    let count = patterns.len();
    RegexSetBuilder::new(patterns)
        .size_limit(SIZE_LIMIT * count)
        .dfa_size_limit(DFA_SIZE_LIMIT * count)
        .nest_limit(NEST_LIMIT)
        .build()
        .inspect_err(|e| tracing::warn!(error = %e, "combining plugin chat patterns failed"))
        .ok()
}

/// The index of the first of `patterns` that matches `text`. Patterns that do not compile (the
/// manifest check makes that impossible) never match.
pub fn first_match(patterns: &[String], text: &str) -> Option<u8> {
    patterns
        .iter()
        .position(|p| compile(p).is_ok_and(|re| re.is_match(text)))
        .and_then(|i| u8::try_from(i).ok())
}

/// `text` cut to at most [`MAX_MATCH_TEXT_BYTES`] bytes at a character boundary.
pub fn clip_text(text: &str) -> String {
    let mut end = text.len().min(MAX_MATCH_TEXT_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const EFT: &str = r"^\[[^,\]\n]{1,64},[^\]\n]{0,100}\]\n";

    #[test]
    fn anchored_patterns_compile() {
        let re = compile(EFT).expect("anchored");
        assert!(re.is_match("[Rifter, PvP]\nDamage Control II\n"));
        assert!(!re.is_match("look: [Rifter, PvP]\n"));
        compile(r"\Afit:").expect("\\A is anchored");
        compile(r"^(?:a|b)").expect("group after the anchor");
        compile(r"(?:^a|^b)").expect("every branch anchored");
    }

    #[test]
    fn unanchored_patterns_are_refused() {
        for pattern in [r"\[Rifter", r"(?m)^\[", r"^a|b", r"a^", r"(?:^)?x", r"$"] {
            assert_eq!(
                compile(pattern).err(),
                Some(PatternError::NotAnchored),
                "{pattern}"
            );
        }
    }

    #[test]
    fn invalid_long_and_large_patterns_are_refused() {
        assert_eq!(compile("^(").err(), Some(PatternError::Invalid));
        assert_eq!(compile(r"^(?<x>a)\k<x>").err(), Some(PatternError::Invalid));
        let long = format!("^{}", "a".repeat(MAX_PATTERN_CHARS));
        assert_eq!(compile(&long).err(), Some(PatternError::TooLong));
        assert_eq!(compile(r"^\w{1000}").err(), Some(PatternError::TooLarge));
        let deep = format!("^{}a{}", "(".repeat(40), ")".repeat(40));
        assert_eq!(compile(&deep).err(), Some(PatternError::Invalid));
    }

    #[test]
    fn the_set_matches_any_pattern() {
        assert!(pattern_set([]).is_none());
        let set = pattern_set([EFT, r"^!fit\b"]).expect("compiles");
        assert!(set.is_match("[Rifter, PvP]\nx"));
        assert!(set.is_match("!fit rifter"));
        assert!(!set.is_match("hello [Rifter, PvP]\n"));
    }

    #[test]
    fn the_first_matching_pattern_wins() {
        let patterns = vec![r"^\[".to_owned(), r"^\[Rifter".to_owned()];
        assert_eq!(first_match(&patterns, "[Rifter, x]"), Some(0));
        assert_eq!(first_match(&patterns[1..], "[Rifter, x]"), Some(0));
        assert_eq!(first_match(&patterns, "Rifter"), None);
    }

    #[test]
    fn text_is_clipped_at_a_character_boundary() {
        assert_eq!(clip_text("short"), "short");
        let long = "ä".repeat(3000);
        let clipped = clip_text(&long);
        assert!(clipped.len() <= MAX_MATCH_TEXT_BYTES);
        assert_eq!(clipped.len(), MAX_MATCH_TEXT_BYTES);
        let odd = format!("a{}", "ä".repeat(3000));
        assert_eq!(clip_text(&odd).len(), MAX_MATCH_TEXT_BYTES - 1);
    }
}
