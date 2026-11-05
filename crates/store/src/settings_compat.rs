//! Settings written by a newer Mumcord (issue #521). Keys this app does not know and values it
//! cannot read (such as a new choice of an existing setting) are kept: the app runs on its
//! defaults for them and puts them back when it saves, so the newer app finds them unchanged.

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::error::{Error, Result};

/// Most unreadable values one record may hold before decoding gives up, so a record that is
/// broken throughout fails instead of looping over every value.
const MAX_UNREADABLE: usize = 64;

/// One step from the root of the settings JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    Key(String),
    Index(usize),
}

/// A part of the stored settings this app does not use as it is.
#[derive(Debug, Clone, PartialEq)]
enum Kept {
    /// A key this app does not know, below objects only.
    UnknownKey { path: Vec<String>, value: Value },
    /// A value this app could not read (or a list holding an unknown key), at an object key.
    /// The app saw `decoded` instead; while it saves that unchanged, `raw` is put back.
    Replaced {
        path: Vec<String>,
        raw: Value,
        decoded: Option<Value>,
    },
}

/// Decodes `json` leniently: every value that fails to decode is dropped and takes its
/// default. Returns the record and what was not used as stored.
fn decode_keeping<T: DeserializeOwned + Serialize>(json: &str) -> Result<(T, Vec<Kept>)> {
    let original: Value = serde_json::from_str(json)?;
    let mut cleaned = original.clone();
    let mut unreadable: Vec<Vec<String>> = Vec::new();
    let decoded: T = loop {
        match serde_path_to_error::deserialize::<_, T>(&cleaned) {
            Ok(decoded) => break decoded,
            Err(error) => {
                let steps = steps_of(error.path());
                if unreadable.len() >= MAX_UNREADABLE || !remove(&mut cleaned, &steps) {
                    return Err(Error::Settings(error.into_inner()));
                }
                let path = object_prefix(&steps);
                if !unreadable.contains(&path) {
                    unreadable.push(path);
                }
            }
        }
    };

    let mut ignored: Vec<Vec<Step>> = Vec::new();
    serde_ignored::deserialize(&cleaned, |path| ignored.push(ignored_steps(&path)))
        .map(|_: T| ())?;

    let mut kept = Vec::new();
    for steps in ignored {
        let path = object_prefix(&steps);
        if path.len() == steps.len() {
            if let Some(value) = get(&original, &path) {
                kept.push(Kept::UnknownKey {
                    path,
                    value: value.clone(),
                });
            }
        } else if !unreadable.contains(&path) {
            // Inside a list: entries have no stable identity, so the whole list is kept.
            unreadable.push(path);
        }
    }
    let seen = serde_json::to_value(&decoded)?;
    for path in unreadable {
        if let Some(raw) = get(&original, &path) {
            kept.push(Kept::Replaced {
                raw: raw.clone(),
                decoded: get(&seen, &path).cloned(),
                path,
            });
        }
    }
    Ok((decoded, kept))
}

/// Decodes a stored settings record, tolerating keys and values from a newer app.
pub(crate) fn decode<T: DeserializeOwned + Serialize>(json: &str) -> Result<T> {
    let (decoded, kept) = decode_keeping(json)?;
    if !kept.is_empty() {
        tracing::debug!(
            count = kept.len(),
            "settings hold values from a newer Mumcord; they are kept"
        );
    }
    Ok(decoded)
}

/// The JSON to store for `settings`, with everything kept from the `stored` record put back.
pub(crate) fn encode<T: DeserializeOwned + Serialize>(
    settings: &T,
    stored: Option<&str>,
) -> Result<String> {
    let mut value = serde_json::to_value(settings)?;
    // A stored record that cannot be read at all has nothing usable to keep.
    let kept = stored
        .and_then(|json| decode_keeping::<T>(json).ok())
        .map(|(_, kept)| kept)
        .unwrap_or_default();
    for kept in kept {
        match kept {
            Kept::UnknownKey { path, value: old } => {
                if get(&value, &path).is_none() {
                    set(&mut value, &path, old);
                }
            }
            Kept::Replaced { path, raw, decoded } => {
                if get(&value, &path) == decoded.as_ref() {
                    set(&mut value, &path, raw);
                }
            }
        }
    }
    Ok(serde_json::to_string(&value)?)
}

fn steps_of(path: &serde_path_to_error::Path) -> Vec<Step> {
    use serde_path_to_error::Segment;

    path.iter()
        .map_while(|segment| match segment {
            Segment::Map { key } => Some(Step::Key(key.clone())),
            Segment::Seq { index } => Some(Step::Index(*index)),
            // Inside an enum or an unknown container: the value holding it is dropped.
            Segment::Enum { .. } | Segment::Unknown => None,
        })
        .collect()
}

fn ignored_steps(path: &serde_ignored::Path<'_>) -> Vec<Step> {
    use serde_ignored::Path;

    match path {
        Path::Root => Vec::new(),
        Path::Seq { parent, index } => {
            let mut steps = ignored_steps(parent);
            steps.push(Step::Index(*index));
            steps
        }
        Path::Map { parent, key } => {
            let mut steps = ignored_steps(parent);
            steps.push(Step::Key(key.clone()));
            steps
        }
        Path::Some { parent }
        | Path::NewtypeStruct { parent }
        | Path::NewtypeVariant { parent } => ignored_steps(parent),
    }
}

/// The leading object keys of `steps`, up to the first list index.
fn object_prefix(steps: &[Step]) -> Vec<String> {
    steps
        .iter()
        .map_while(|step| match step {
            Step::Key(key) => Some(key.clone()),
            Step::Index(_) => None,
        })
        .collect()
}

/// Removes the value at `steps`; false if there is none (or `steps` is the root).
fn remove(value: &mut Value, steps: &[Step]) -> bool {
    let Some((last, parents)) = steps.split_last() else {
        return false;
    };
    let mut parent = value;
    for step in parents {
        let next = match (step, parent) {
            (Step::Key(key), Value::Object(map)) => map.get_mut(key),
            (Step::Index(index), Value::Array(list)) => list.get_mut(*index),
            _ => None,
        };
        let Some(next) = next else {
            return false;
        };
        parent = next;
    }
    match (last, parent) {
        (Step::Key(key), Value::Object(map)) => map.remove(key).is_some(),
        (Step::Index(index), Value::Array(list)) if *index < list.len() => {
            list.remove(*index);
            true
        }
        _ => false,
    }
}

fn get<'a>(value: &'a Value, path: &[String]) -> Option<&'a Value> {
    path.iter().try_fold(value, |value, key| value.get(key))
}

/// Sets the value at `path` if its parent object exists.
fn set(value: &mut Value, path: &[String], new: Value) {
    let Some((last, parents)) = path.split_last() else {
        return;
    };
    let parent = parents
        .iter()
        .try_fold(value, |value, key| value.get_mut(key));
    if let Some(Value::Object(map)) = parent {
        map.insert(last.clone(), new);
    }
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;
    use serde_json::json;

    use super::*;

    #[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    enum Shade {
        #[default]
        Light,
        Dark,
    }

    #[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
    #[serde(default)]
    struct Inner {
        on: bool,
    }

    #[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
    #[serde(default)]
    struct Record {
        name: String,
        shade: Shade,
        inner: Inner,
        shades: Vec<Shade>,
        #[serde(alias = "colour")]
        color: String,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        tags: Vec<String>,
    }

    fn stored(value: &Value) -> String {
        value.to_string()
    }

    fn saved(record: &Record, old: &Value) -> Value {
        let json = encode(record, Some(&stored(old))).expect("encode");
        serde_json::from_str(&json).expect("json")
    }

    #[test]
    fn unknown_keys_are_ignored_and_kept_on_save() {
        let old = json!({ "name": "a", "future": [1, 2], "inner": { "on": true, "later": "x" } });
        let mut record: Record = decode(&stored(&old)).expect("decode");
        assert_eq!(record.name, "a");
        assert!(record.inner.on);

        record.name = "b".into();
        record.inner.on = false;
        let new = saved(&record, &old);
        assert_eq!(new["name"], "b");
        assert_eq!(new["future"], json!([1, 2]));
        assert_eq!(new["inner"], json!({ "on": false, "later": "x" }));
    }

    #[test]
    fn unknown_enum_values_use_the_default_and_survive_a_save() {
        let old = json!({ "name": "a", "shade": "neon" });
        let mut record: Record = decode(&stored(&old)).expect("decode");
        assert_eq!(record.shade, Shade::Light);

        record.name = "b".into();
        assert_eq!(saved(&record, &old)["shade"], "neon");
    }

    #[test]
    fn a_value_the_user_changed_replaces_the_unreadable_one() {
        let old = json!({ "shade": "neon" });
        let mut record: Record = decode(&stored(&old)).expect("decode");
        record.shade = Shade::Dark;
        assert_eq!(saved(&record, &old)["shade"], "dark");
    }

    #[test]
    fn a_list_with_an_unknown_entry_is_kept_whole_while_unchanged() {
        let old = json!({ "shades": ["dark", "neon", "light"] });
        let mut record: Record = decode(&stored(&old)).expect("decode");
        assert_eq!(record.shades, [Shade::Dark, Shade::Light]);
        assert_eq!(
            saved(&record, &old)["shades"],
            json!(["dark", "neon", "light"])
        );

        record.shades.push(Shade::Dark);
        assert_eq!(
            saved(&record, &old)["shades"],
            json!(["dark", "light", "dark"])
        );
    }

    #[test]
    fn aliases_and_cleared_fields_are_not_brought_back() {
        let old = json!({ "colour": "red", "tags": ["x"] });
        let mut record: Record = decode(&stored(&old)).expect("decode");
        assert_eq!(record.color, "red");
        record.tags.clear();
        let new = saved(&record, &old);
        assert_eq!(
            new,
            json!({
                "name": "", "shade": "light", "inner": { "on": false }, "shades": [], "color": "red"
            })
        );
    }

    #[test]
    fn a_record_that_is_not_settings_still_fails() {
        assert!(decode::<Record>("\"text\"").is_err());
        assert!(decode::<Record>("not json").is_err());
    }
}
