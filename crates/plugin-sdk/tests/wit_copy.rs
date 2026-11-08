//! The SDK generates its bindings from its own copy of the WIT (`wit/<version>/`), because a
//! crate on crates.io cannot read files outside its directory. The canonical WIT lives in
//! `crates/plugins/wit/<version>/`; this test fails as soon as a copy differs from it.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

fn wit_files(dir: &Path) -> BTreeMap<String, String> {
    fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .map(|entry| {
            let path = entry.expect("directory entry").path();
            let name = path
                .file_name()
                .expect("file name")
                .to_string_lossy()
                .into_owned();
            let text = fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
            // Git may check the files out with CRLF line endings on Windows.
            (name, text.replace("\r\n", "\n"))
        })
        .collect()
}

/// Every published API minor; the SDK copies each one.
const VERSIONS: [&str; 3] = ["1.0.0", "1.1.0", "1.2.0"];

#[test]
fn sdk_wit_matches_the_canonical_wit() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let versions = |dir: &Path| -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
            .map(|entry| {
                entry
                    .expect("directory entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort_unstable();
        names
    };
    assert_eq!(versions(&root.join("wit")), VERSIONS);
    assert_eq!(versions(&root.join("../plugins/wit")), VERSIONS);
    for version in VERSIONS {
        let canonical = wit_files(&root.join("../plugins/wit").join(version));
        let copy = wit_files(&root.join("wit").join(version));
        assert_eq!(
            canonical.keys().collect::<Vec<_>>(),
            copy.keys().collect::<Vec<_>>(),
            "crates/plugin-sdk/wit/{version} must hold the same files as crates/plugins/wit/{version}"
        );
        for (name, text) in &canonical {
            assert!(
                copy[name] == *text,
                "crates/plugin-sdk/wit/{version}/{name} differs from \
                 crates/plugins/wit/{version}/{name}; copy it over"
            );
        }
    }
}
