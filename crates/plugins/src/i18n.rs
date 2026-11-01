//! Translated plugin name and description (FR-6.10): `i18n/<lang>.json` holds flat string keys;
//! `manifest.name` and `manifest.description` override the manifest's English text. The app
//! language wins, then its base language, then English, then the manifest itself. The languages a
//! plugin offers are simply the files present. `chat_patterns.<n>.description` translates the
//! description of the manifest's chat pattern `n` the same way.

use std::collections::BTreeMap;
use std::path::Path;

use crate::manifest::{Manifest, is_language_tag};
use crate::package::Package;
use crate::patterns::MAX_PATTERN_DESCRIPTION_CHARS;

/// Larger translation files are ignored.
pub const MAX_I18N_BYTES: u64 = 64 * 1024;

const NAME_KEY: &str = "manifest.name";
const DESCRIPTION_KEY: &str = "manifest.description";
const MAX_NAME_CHARS: usize = 32;
const MAX_DESCRIPTION_CHARS: usize = 140;

/// Name and description in the user's language, as plain text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayText {
    pub name: String,
    pub description: String,
}

/// Looks up the display text for `language` (e.g. `de-AT`) in a plugin directory.
pub fn display_text(manifest: &Manifest, plugin_dir: &Path, language: &str) -> DisplayText {
    let tables = tables(language, |tag| {
        read_table(&plugin_dir.join("i18n").join(format!("{tag}.json")))
    });
    let lookup = |key: &str, max: usize| {
        tables
            .iter()
            .find_map(|t| t.get(key))
            .and_then(|text| clean(text, max))
    };
    DisplayText {
        name: lookup(NAME_KEY, MAX_NAME_CHARS).unwrap_or_else(|| manifest.name.clone()),
        description: lookup(DESCRIPTION_KEY, MAX_DESCRIPTION_CHARS)
            .unwrap_or_else(|| manifest.description.clone()),
    }
}

/// The descriptions of the manifest's chat patterns in `language`, read from a package before it
/// is installed (the install prompt shows them with `chat.patterns`).
pub fn chat_pattern_descriptions(package: &Package, language: &str) -> Vec<String> {
    let tables = tables(language, |tag| {
        let bytes = package.file(&format!("i18n/{tag}.json"))?;
        if bytes.len() as u64 > MAX_I18N_BYTES {
            tracing::warn!(tag, "plugin translation file too large, ignored");
            return None;
        }
        parse_table(bytes, tag)
    });
    package
        .manifest
        .chat_patterns
        .iter()
        .enumerate()
        .map(|(n, pattern)| {
            let key = format!("chat_patterns.{n}.description");
            tables
                .iter()
                .find_map(|t| clean(t.get(&key)?, MAX_PATTERN_DESCRIPTION_CHARS))
                .unwrap_or_else(|| pattern.description.clone())
        })
        .collect()
}

/// The translation tables for `language`, best first.
fn tables(
    language: &str,
    read: impl Fn(&str) -> Option<BTreeMap<String, String>>,
) -> Vec<BTreeMap<String, String>> {
    fallback_chain(language)
        .iter()
        .filter_map(|tag| read(tag))
        .collect()
}

/// `de-AT` → `de-AT`, `de`, then English. Only valid tags, so the file name stays inside `i18n/`.
fn fallback_chain(language: &str) -> Vec<String> {
    let base = language.split('-').next().unwrap_or_default();
    let mut chain: Vec<String> = Vec::new();
    for tag in [language, base, "en"] {
        if is_language_tag(tag) && !chain.iter().any(|c| c == tag) {
            chain.push(tag.to_owned());
        }
    }
    chain
}

fn read_table(path: &Path) -> Option<BTreeMap<String, String>> {
    let size = std::fs::metadata(path).ok()?.len();
    if size > MAX_I18N_BYTES {
        tracing::warn!(file = %path.display(), "plugin translation file too large, ignored");
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    parse_table(&bytes, &path.display().to_string())
}

fn parse_table(bytes: &[u8], file: &str) -> Option<BTreeMap<String, String>> {
    match serde_json::from_slice::<BTreeMap<String, serde_json::Value>>(bytes) {
        Ok(map) => Some(
            map.into_iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k, s.to_owned())))
                .collect(),
        ),
        Err(e) => {
            tracing::warn!(file, error = %e, "plugin translation file is not valid JSON, ignored");
            None
        }
    }
}

/// Translations are untrusted text: no control characters, capped like the manifest fields.
fn clean(text: &str, max: usize) -> Option<String> {
    let text = text.trim();
    (!text.is_empty() && !text.chars().any(char::is_control))
        .then(|| text.chars().take(max).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> Manifest {
        let json = r#"{"manifest":1,"id":"pets","name":"Pets","version":"1.0.0","api":"^1.0",
            "authors":["A"],"description":"English text","categories":["fun"],"license":"MIT"}"#;
        Manifest::parse(json.as_bytes()).expect("valid")
    }

    #[test]
    fn follows_the_app_language_with_english_fallback() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir(dir.path().join("i18n")).expect("mkdir");
        std::fs::write(
            dir.path().join("i18n/en.json"),
            r#"{"manifest.name":"Pets EN"}"#,
        )
        .expect("write");
        std::fs::write(
            dir.path().join("i18n/de.json"),
            r#"{"manifest.name":"Haustiere","manifest.description":"Deutsch"}"#,
        )
        .expect("write");
        let m = manifest();

        let de = display_text(&m, dir.path(), "de-AT");
        assert_eq!(
            de,
            DisplayText {
                name: "Haustiere".into(),
                description: "Deutsch".into()
            }
        );

        let fr = display_text(&m, dir.path(), "fr");
        assert_eq!(
            fr,
            DisplayText {
                name: "Pets EN".into(),
                description: "English text".into()
            }
        );
    }

    #[test]
    fn ignores_missing_and_unsafe_translations() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir(dir.path().join("i18n")).expect("mkdir");
        std::fs::write(dir.path().join("i18n/en.json"), "{}").expect("write");
        std::fs::write(
            dir.path().join("i18n/de.json"),
            "{\"manifest.name\":\"Bad\\u0007\"}",
        )
        .expect("write");
        let m = manifest();
        assert_eq!(display_text(&m, dir.path(), "es").name, "Pets");
        assert_eq!(display_text(&m, dir.path(), "../en").name, "Pets");
        assert_eq!(display_text(&m, dir.path(), "de").name, "Pets");
    }
}
