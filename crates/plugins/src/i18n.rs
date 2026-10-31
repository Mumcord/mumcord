//! Translated plugin name and description (#269): `i18n/<lang>.json` holds flat string keys;
//! `manifest.name` and `manifest.description` override the manifest's English text. The app
//! language wins, then its base language, then English, then the manifest itself.

use std::collections::BTreeMap;
use std::path::Path;

use crate::manifest::{Manifest, is_language_tag};

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
    pub description: Option<String>,
}

/// Looks up the display text for `language` (e.g. `de-AT`) in a plugin directory.
pub fn display_text(manifest: &Manifest, plugin_dir: &Path, language: &str) -> DisplayText {
    let chain = fallback_chain(manifest, language);
    let tables: Vec<BTreeMap<String, String>> = chain
        .iter()
        .filter_map(|tag| read_table(&plugin_dir.join("i18n").join(format!("{tag}.json"))))
        .collect();
    let lookup = |key: &str, max: usize| {
        tables
            .iter()
            .find_map(|t| t.get(key))
            .and_then(|text| clean(text, max))
    };
    DisplayText {
        name: lookup(NAME_KEY, MAX_NAME_CHARS).unwrap_or_else(|| manifest.name.clone()),
        description: lookup(DESCRIPTION_KEY, MAX_DESCRIPTION_CHARS)
            .or_else(|| manifest.description.clone()),
    }
}

/// `de-AT` → `de-AT`, `de`, then English; only languages the manifest declares.
fn fallback_chain(manifest: &Manifest, language: &str) -> Vec<String> {
    let base = language.split('-').next().unwrap_or_default();
    let mut chain: Vec<String> = Vec::new();
    for tag in [language, base, "en"] {
        if is_language_tag(tag)
            && manifest.i18n.available.iter().any(|a| a == tag)
            && !chain.iter().any(|c| c == tag)
        {
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
    match serde_json::from_slice::<BTreeMap<String, serde_json::Value>>(&bytes) {
        Ok(map) => Some(
            map.into_iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k, s.to_owned())))
                .collect(),
        ),
        Err(e) => {
            tracing::warn!(file = %path.display(), error = %e, "plugin translation file is not valid JSON, ignored");
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
            "author":{"name":"A"},"description":"English text","category":"fun","license":"MIT",
            "entry":"plugin.wasm","i18n":{"default":"en","available":["en","de"]}}"#;
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
                description: Some("Deutsch".into())
            }
        );

        let fr = display_text(&m, dir.path(), "fr");
        assert_eq!(
            fr,
            DisplayText {
                name: "Pets EN".into(),
                description: Some("English text".into())
            }
        );
    }

    #[test]
    fn ignores_undeclared_and_unsafe_translations() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir(dir.path().join("i18n")).expect("mkdir");
        std::fs::write(dir.path().join("i18n/en.json"), "{}").expect("write");
        // Present on disk but not declared in the manifest.
        std::fs::write(
            dir.path().join("i18n/es.json"),
            r#"{"manifest.name":"Mascotas"}"#,
        )
        .expect("write");
        std::fs::write(
            dir.path().join("i18n/de.json"),
            "{\"manifest.name\":\"Bad\\u0007\"}",
        )
        .expect("write");
        let m = manifest();
        assert_eq!(display_text(&m, dir.path(), "es").name, "Pets");
        assert_eq!(display_text(&m, dir.path(), "de").name, "Pets");
    }
}
