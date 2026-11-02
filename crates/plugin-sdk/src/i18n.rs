//! Translations for plugins. The plugin's `i18n/<tag>.json` files are embedded at compile time
//! (the host offers no file access at runtime) and looked up in the language the host reports
//! through the `language-changed` event.
//!
//! Files are flat JSON objects of strings. Placeholders are written `{{name}}`. Plural forms use
//! the same convention as the Mumcord app (i18next): one key per CLDR plural category of the
//! language, `key_zero`, `key_one`, `key_two`, `key_few`, `key_many` and `key_other`, with
//! `key_other` as the fallback and `{{count}}` filled in automatically. See [`plural_category`].
//!
//! Embed the files with [`i18n_embed!`] (an explicit list of tags) or, for official plugins,
//! [`i18n_all!`] (all 24 languages):
//!
//! ```ignore
//! let strings = Strings::new(mumcord_plugin_sdk::i18n_embed!["en", "de", "fr"]);
//! ```
//!
//! Using the strings, here with inline JSON so the example runs:
//!
//! ```
//! use mumcord_plugin_sdk::{events::Event, i18n::Strings};
//!
//! let mut strings = Strings::new(&[
//!     ("en", r#"{"hi":"Hello {{name}}!","votes_one":"{{count}} vote","votes_other":"{{count}} votes"}"#),
//!     ("de", r#"{"hi":"Hallo {{name}}!"}"#),
//! ]);
//!
//! // In `Plugin::on_event`: returns true when the language changed, so render the UI again.
//! assert!(strings.on_event(&Event::LanguageChanged("de-AT".into())));
//!
//! assert_eq!(strings.t_with("hi", &[("name", "Ada")]), "Hallo Ada!");
//! // Not translated into German: falls back to English.
//! assert_eq!(strings.tn("votes", 3, &[]), "3 votes");
//! // Unknown key: the key itself (and a log warning), never a panic.
//! assert_eq!(strings.t("missing"), "missing");
//! ```

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use crate::events::Event;

/// The 24 languages of Mumcord. Official plugins ship a file for each.
pub const LANGUAGES: [&str; 24] = [
    "bg", "cs", "da", "de", "el", "en", "es", "et", "fi", "fr", "ga", "hr", "hu", "it", "lt", "lv",
    "mt", "nl", "pl", "pt", "ro", "sk", "sl", "sv",
];

/// The language every lookup finally falls back to.
const FALLBACK: &str = "en";

/// Embeds `i18n/<tag>.json` for each listed tag, relative to the plugin's crate root. Returns a
/// `&'static [(&'static str, &'static str)]` for [`Strings::new`]. A missing file is a compile
/// error.
#[macro_export]
macro_rules! i18n_embed {
    ($($tag:literal),+ $(,)?) => {
        &[$(
            ($tag, include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/i18n/", $tag, ".json"))),
        )+] as &'static [(&'static str, &'static str)]
    };
}

/// Embeds the files of all 24 Mumcord languages (see [`LANGUAGES`]).
#[macro_export]
macro_rules! i18n_all {
    () => {
        $crate::i18n_embed![
            "bg", "cs", "da", "de", "el", "en", "es", "et", "fi", "fr", "ga", "hr", "hu", "it",
            "lt", "lv", "mt", "nl", "pl", "pt", "ro", "sk", "sl", "sv"
        ]
    };
}

/// The embedded translations plus the current language.
pub struct Strings {
    bundles: HashMap<String, HashMap<String, String>>,
    /// Languages to try for the current language, most specific first, always ending in `en`.
    chain: Vec<String>,
    warned: RefCell<HashSet<String>>,
}

impl Strings {
    /// Builds the strings from `(tag, json)` pairs (see [`i18n_embed!`]). A file that is not a
    /// flat JSON object of strings is skipped and logged; it never panics. The language starts
    /// as English until the host's `language-changed` event arrives.
    pub fn new(files: &[(&str, &str)]) -> Self {
        let mut bundles = HashMap::new();
        for (tag, json) in files {
            match serde_json::from_str::<HashMap<String, String>>(json) {
                Ok(map) => {
                    bundles.insert(normalise(tag), map);
                }
                Err(e) => warn(&format!("i18n/{tag}.json is not valid: {e}")),
            }
        }
        let mut strings = Self {
            bundles,
            chain: Vec::new(),
            warned: RefCell::new(HashSet::new()),
        };
        strings.set_language(FALLBACK);
        strings
    }

    /// Switches the language. `de-AT` tries `de-at`, then `de`, then `en`.
    pub fn set_language(&mut self, tag: &str) {
        let mut chain = Vec::new();
        let mut current = normalise(tag);
        loop {
            if !current.is_empty() && !chain.contains(&current) {
                chain.push(current.clone());
            }
            match current.rfind('-') {
                Some(i) => current.truncate(i),
                None => break,
            }
        }
        if !chain.iter().any(|t| t == FALLBACK) {
            chain.push(FALLBACK.to_owned());
        }
        self.chain = chain;
    }

    /// Feeds a host event in. Returns true when it was `language-changed`, so the plugin can
    /// render its UI again.
    pub fn on_event(&mut self, event: &Event) -> bool {
        match event {
            Event::LanguageChanged(tag) => {
                self.set_language(tag);
                true
            }
            _ => false,
        }
    }

    /// The text for `key` in the current language, falling back per key along the chain. An
    /// unknown key returns the key itself and logs a warning once.
    pub fn t(&self, key: &str) -> String {
        self.t_with(key, &[])
    }

    /// Like [`Strings::t`], replacing each `{{name}}` with its value from `args`.
    pub fn t_with(&self, key: &str, args: &[(&str, &str)]) -> String {
        match self.lookup(key) {
            Some(text) => fill(text, args),
            None => self.missing(key),
        }
    }

    /// Plural text: for each language of the fallback chain, uses `<key>_<category>` where the
    /// category is that language's CLDR cardinal plural category of `count` (see
    /// [`plural_category`]), then `<key>_other`; finally the bare `key`. `{{count}}` is filled in;
    /// `args` adds more placeholders.
    pub fn tn(&self, key: &str, count: u64, args: &[(&str, &str)]) -> String {
        let other_key = format!("{key}_other");
        let text = self
            .chain
            .iter()
            .find_map(|tag| {
                let bundle = self.bundles.get(tag)?;
                let category = plural_category(tag, count);
                bundle
                    .get(&format!("{key}_{category}"))
                    .or_else(|| bundle.get(&other_key))
            })
            .map(String::as_str)
            .or_else(|| self.lookup(key));
        let Some(text) = text else {
            return self.missing(&other_key);
        };
        let count = count.to_string();
        let mut all = vec![("count", count.as_str())];
        all.extend_from_slice(args);
        fill(text, &all)
    }

    fn lookup(&self, key: &str) -> Option<&str> {
        self.chain
            .iter()
            .find_map(|tag| self.bundles.get(tag)?.get(key))
            .map(String::as_str)
    }

    fn missing(&self, key: &str) -> String {
        if self.warned.borrow_mut().insert(key.to_owned()) {
            warn(&format!("missing translation key {key}"));
        }
        key.to_owned()
    }
}

/// The CLDR cardinal plural category (`zero`, `one`, `two`, `few`, `many` or `other`) of an
/// integer `count` in the language `tag` (a base or region tag such as `pl` or `pt-BR`). These are
/// the categories `Intl.PluralRules` gives the Mumcord app, so translators reuse the same keys.
/// A language outside the 24 Mumcord ones gets the English rule.
pub fn plural_category(tag: &str, count: u64) -> &'static str {
    let n = count;
    let (m10, m100) = (n % 10, n % 100);
    // Romance "many": a non-zero whole number of millions (CLDR 42 and later).
    let millions = n != 0 && n.is_multiple_of(1_000_000);
    let base = tag.split(['-', '_']).next().unwrap_or(tag);
    match base {
        "fr" | "pt" if n <= 1 => "one",
        "es" | "it" if n == 1 => "one",
        "fr" | "pt" | "es" | "it" => {
            if millions {
                "many"
            } else {
                "other"
            }
        }
        "cs" | "sk" => match n {
            1 => "one",
            2..=4 => "few",
            _ => "other",
        },
        "ga" => match n {
            1 => "one",
            2 => "two",
            3..=6 => "few",
            7..=10 => "many",
            _ => "other",
        },
        "hr" => {
            if m10 == 1 && m100 != 11 {
                "one"
            } else if (2..=4).contains(&m10) && !(12..=14).contains(&m100) {
                "few"
            } else {
                "other"
            }
        }
        "lt" => {
            if m10 == 1 && !(11..=19).contains(&m100) {
                "one"
            } else if (2..=9).contains(&m10) && !(11..=19).contains(&m100) {
                "few"
            } else {
                "other"
            }
        }
        "lv" => {
            if m10 == 0 || (11..=19).contains(&m100) {
                "zero"
            } else if m10 == 1 && m100 != 11 {
                "one"
            } else {
                "other"
            }
        }
        "mt" => {
            if n == 1 {
                "one"
            } else if n == 2 {
                "two"
            } else if n == 0 || (2..=10).contains(&m100) {
                "few"
            } else if (11..=19).contains(&m100) {
                "many"
            } else {
                "other"
            }
        }
        "pl" => {
            if n == 1 {
                "one"
            } else if (2..=4).contains(&m10) && !(12..=14).contains(&m100) {
                "few"
            } else {
                "many"
            }
        }
        "ro" => {
            if n == 1 {
                "one"
            } else if n == 0 || (1..=19).contains(&m100) {
                "few"
            } else {
                "other"
            }
        }
        "sl" => match m100 {
            1 => "one",
            2 => "two",
            3 | 4 => "few",
            _ => "other",
        },
        // bg da de el en et fi hu nl sv: one for exactly 1.
        _ => {
            if n == 1 {
                "one"
            } else {
                "other"
            }
        }
    }
}

/// `de_AT` and `DE-at` both become `de-at`.
fn normalise(tag: &str) -> String {
    tag.trim().replace('_', "-").to_ascii_lowercase()
}

/// Single pass, so a value containing `{{x}}` is never substituted again. An unknown placeholder
/// stays as written.
fn fill(text: &str, args: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match after.find("}}") {
            Some(end) => {
                let name = after[..end].trim();
                match args.iter().find(|(n, _)| *n == name) {
                    Some((_, value)) => out.push_str(value),
                    None => out.push_str(&rest[start..start + 2 + end + 2]),
                }
                rest = &after[end + 2..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// Host calls exist only inside the WebAssembly component; native unit tests skip the log.
fn warn(message: &str) {
    #[cfg(target_arch = "wasm32")]
    crate::log::warn(message);
    #[cfg(not(target_arch = "wasm32"))]
    let _ = message;
}

#[cfg(test)]
mod tests {
    use super::*;

    const EN: &str = r#"{"hi":"Hello {{name}}!","votes_one":"{{count}} vote","votes_other":"{{count}} votes","only_en":"English only"}"#;
    const DE: &str = r#"{"hi":"Hallo {{name}}!","votes_one":"{{count}} Stimme","votes_other":"{{count}} Stimmen"}"#;
    const DE_AT: &str = r#"{"hi":"Servus {{name}}!"}"#;

    fn strings() -> Strings {
        Strings::new(&[("en", EN), ("de", DE), ("de-AT", DE_AT)])
    }

    #[test]
    fn starts_in_english() {
        assert_eq!(strings().t_with("hi", &[("name", "Ada")]), "Hello Ada!");
    }

    #[test]
    fn region_then_base_then_english() {
        let mut s = strings();
        s.set_language("de-AT");
        assert_eq!(s.t_with("hi", &[("name", "Ada")]), "Servus Ada!");
        // Not in de-AT: base language.
        assert_eq!(s.tn("votes", 2, &[]), "2 Stimmen");
        // Not in de at all: English.
        assert_eq!(s.t("only_en"), "English only");
        s.set_language("de_CH");
        assert_eq!(s.t_with("hi", &[("name", "Ada")]), "Hallo Ada!");
        s.set_language("fr");
        assert_eq!(s.t_with("hi", &[("name", "Ada")]), "Hello Ada!");
    }

    #[test]
    fn language_changed_event_switches() {
        let mut s = strings();
        assert!(s.on_event(&Event::LanguageChanged("de".into())));
        assert_eq!(s.t("only_en"), "English only");
        assert_eq!(s.tn("votes", 1, &[]), "1 Stimme");
    }

    #[test]
    fn plurals_pick_one_or_other() {
        let s = strings();
        assert_eq!(s.tn("votes", 0, &[]), "0 votes");
        assert_eq!(s.tn("votes", 1, &[]), "1 vote");
        assert_eq!(s.tn("votes", 5, &[]), "5 votes");
    }

    #[test]
    fn plural_categories_per_language() {
        let cases: &[(&str, &[(u64, &str)])] = &[
            ("bg", &[(0, "other"), (1, "one"), (2, "other")]),
            ("da", &[(0, "other"), (1, "one"), (5, "other")]),
            ("de", &[(0, "other"), (1, "one"), (2, "other")]),
            ("el", &[(0, "other"), (1, "one"), (2, "other")]),
            (
                "en",
                &[(0, "other"), (1, "one"), (2, "other"), (21, "other")],
            ),
            ("et", &[(0, "other"), (1, "one"), (2, "other")]),
            ("fi", &[(0, "other"), (1, "one"), (2, "other")]),
            ("hu", &[(0, "other"), (1, "one"), (2, "other")]),
            ("nl", &[(0, "other"), (1, "one"), (2, "other")]),
            ("sv", &[(0, "other"), (1, "one"), (2, "other")]),
            (
                "cs",
                &[
                    (0, "other"),
                    (1, "one"),
                    (3, "few"),
                    (5, "other"),
                    (22, "other"),
                ],
            ),
            (
                "sk",
                &[
                    (0, "other"),
                    (1, "one"),
                    (4, "few"),
                    (5, "other"),
                    (22, "other"),
                ],
            ),
            (
                "es",
                &[
                    (0, "other"),
                    (1, "one"),
                    (2, "other"),
                    (1_000_000, "many"),
                    (2_000_000, "many"),
                ],
            ),
            (
                "it",
                &[(0, "other"), (1, "one"), (2, "other"), (1_000_000, "many")],
            ),
            (
                "fr",
                &[
                    (0, "one"),
                    (1, "one"),
                    (2, "other"),
                    (1_000_000, "many"),
                    (1_500_000, "other"),
                ],
            ),
            (
                "pt",
                &[(0, "one"), (1, "one"), (2, "other"), (1_000_000, "many")],
            ),
            ("pt-BR", &[(0, "one"), (2, "other")]),
            (
                "ga",
                &[
                    (1, "one"),
                    (2, "two"),
                    (4, "few"),
                    (8, "many"),
                    (11, "other"),
                    (0, "other"),
                ],
            ),
            (
                "hr",
                &[
                    (1, "one"),
                    (21, "one"),
                    (11, "other"),
                    (3, "few"),
                    (23, "few"),
                    (13, "other"),
                    (5, "other"),
                    (0, "other"),
                ],
            ),
            (
                "lt",
                &[
                    (1, "one"),
                    (21, "one"),
                    (11, "other"),
                    (2, "few"),
                    (9, "few"),
                    (12, "other"),
                    (10, "other"),
                    (0, "other"),
                ],
            ),
            (
                "lv",
                &[
                    (0, "zero"),
                    (10, "zero"),
                    (11, "zero"),
                    (19, "zero"),
                    (1, "one"),
                    (21, "one"),
                    (2, "other"),
                    (22, "other"),
                ],
            ),
            (
                "mt",
                &[
                    (1, "one"),
                    (2, "two"),
                    (0, "few"),
                    (3, "few"),
                    (10, "few"),
                    (11, "many"),
                    (19, "many"),
                    (20, "other"),
                    (102, "few"),
                    (111, "many"),
                ],
            ),
            (
                "pl",
                &[
                    (1, "one"),
                    (2, "few"),
                    (4, "few"),
                    (5, "many"),
                    (12, "many"),
                    (22, "few"),
                    (0, "many"),
                    (21, "many"),
                ],
            ),
            (
                "ro",
                &[
                    (1, "one"),
                    (0, "few"),
                    (2, "few"),
                    (19, "few"),
                    (20, "other"),
                    (101, "few"),
                    (120, "other"),
                ],
            ),
            (
                "sl",
                &[
                    (1, "one"),
                    (101, "one"),
                    (2, "two"),
                    (102, "two"),
                    (3, "few"),
                    (4, "few"),
                    (5, "other"),
                    (0, "other"),
                ],
            ),
        ];
        let languages: Vec<&str> = cases.iter().map(|(tag, _)| *tag).collect();
        for tag in LANGUAGES.iter() {
            assert!(languages.contains(tag), "no plural test for {tag}");
        }
        for (tag, expected) in cases {
            for (count, category) in *expected {
                assert_eq!(plural_category(tag, *count), *category, "{tag} {count}");
            }
        }
    }

    #[test]
    fn tn_uses_the_category_key_and_falls_back_to_other() {
        let pl = r#"{"v_one":"{{count}} glos","v_few":"{{count}} glosy","v_many":"{{count}} glosow","v_other":"{{count}} glosa"}"#;
        let mut s = Strings::new(&[("en", EN), ("pl", pl)]);
        s.set_language("pl");
        assert_eq!(s.tn("v", 3, &[]), "3 glosy");
        assert_eq!(s.tn("v", 5, &[]), "5 glosow");
        let only_other = r#"{"v_other":"{{count}} x"}"#;
        let mut s = Strings::new(&[("en", EN), ("pl", only_other)]);
        s.set_language("pl");
        assert_eq!(s.tn("v", 3, &[]), "3 x");
    }

    #[test]
    fn unknown_key_returns_the_key() {
        let s = strings();
        assert_eq!(s.t("nope"), "nope");
        assert_eq!(s.tn("nope", 2, &[]), "nope_other");
    }

    #[test]
    fn placeholders_are_single_pass() {
        let s = Strings::new(&[("en", r#"{"a":"{{x}} {{y}} {{x"}"#)]);
        assert_eq!(s.t_with("a", &[("x", "{{y}}"), ("y", "2")]), "{{y}} 2 {{x");
        assert_eq!(s.t("a"), "{{x}} {{y}} {{x");
    }

    #[test]
    fn invalid_file_is_skipped() {
        let s = Strings::new(&[("en", EN), ("de", "[1]")]);
        assert_eq!(s.t("only_en"), "English only");
    }
}
