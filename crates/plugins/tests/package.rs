//! `.mcplugin` packages built in memory: the good case and each way a hostile package can be bad.

use std::io::Write;

use mumcord_plugins::package::{MAX_PACKAGE_BYTES, check_unofficial_id};
use mumcord_plugins::{Digest, ExpectedDigest, Package, PackageError};
use zip::write::SimpleFileOptions;

const MANIFEST: &str = r#"{"manifest":1,"id":"dice-roller","name":"Dice","version":"1.0.0","api":"^1.0",
    "author":{"name":"Tests"},"category":"games","license":"MIT","entry":"plugin.wasm",
    "icons":{"64":"assets/icon-64.png"},"i18n":{"default":"en","available":["en"]}}"#;

/// The files of a valid package; tests change or add entries.
fn good_files() -> Vec<(String, Vec<u8>)> {
    vec![
        ("manifest.json".into(), MANIFEST.into()),
        ("plugin.wasm".into(), b"\0asm".to_vec()),
        ("i18n/en.json".into(), b"{}".to_vec()),
        ("assets/icon-64.png".into(), vec![0; 100]),
    ]
}

fn zip(files: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, data) in files {
        writer
            .start_file(name.as_str(), options)
            .expect("start file");
        writer.write_all(data).expect("write");
    }
    writer.finish().expect("finish").into_inner()
}

fn with(change: impl FnOnce(&mut Vec<(String, Vec<u8>)>)) -> Result<Package, PackageError> {
    let mut files = good_files();
    change(&mut files);
    Package::read(&zip(&files))
}

#[test]
fn accepts_and_installs_a_good_package() {
    let bytes = zip(&good_files());
    let package =
        Package::read_verified(&bytes, &ExpectedDigest(Digest::of(&bytes))).expect("valid");
    assert_eq!(package.manifest.id, "dice-roller");

    let root = tempfile::tempdir().expect("temp dir");
    let dir = package.install(root.path()).expect("installs");
    assert_eq!(dir, root.path().join("dice-roller"));
    assert_eq!(
        std::fs::read(dir.join("plugin.wasm")).expect("read"),
        b"\0asm"
    );
    // Installing again replaces the old files and leaves no staging directory behind.
    package.install(root.path()).expect("reinstalls");
    assert_eq!(std::fs::read_dir(root.path()).expect("list").count(), 1);
}

#[test]
fn digest_must_match_the_index() {
    let bytes = zip(&good_files());
    let mut expected = Digest::of(&bytes);
    assert_eq!(expected.sha256_hex().len(), 64);
    expected.sha256[0] ^= 1;
    assert!(matches!(
        Package::read_verified(&bytes, &ExpectedDigest(expected)),
        Err(PackageError::Verification(_))
    ));
}

#[test]
fn rejects_path_traversal_and_absolute_paths() {
    for name in [
        "../evil.wasm",
        "assets/../../evil",
        "/etc/passwd",
        "C:\\evil",
        "a\\..\\b",
    ] {
        let result = with(|f| f.push((name.into(), b"x".to_vec())));
        assert!(
            matches!(result, Err(PackageError::BadEntry(_))),
            "{name}: {result:?}"
        );
    }
}

#[test]
fn rejects_symlinks() {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default();
    for (name, data) in good_files() {
        writer
            .start_file(name.as_str(), options)
            .expect("start file");
        writer.write_all(&data).expect("write");
    }
    writer
        .add_symlink("i18n/de.json", "/home/user/.ssh/id_ed25519", options)
        .expect("symlink");
    let bytes = writer.finish().expect("finish").into_inner();
    assert!(matches!(
        Package::read(&bytes),
        Err(PackageError::BadEntry(_))
    ));
}

#[test]
fn rejects_too_many_files_and_oversize_packages() {
    let result = with(|f| f.extend((0..200).map(|i| (format!("i18n/x{i}.json"), Vec::new()))));
    assert!(matches!(result, Err(PackageError::TooManyFiles)));

    let big = vec![0u8; MAX_PACKAGE_BYTES as usize + 1];
    assert!(matches!(Package::read(&big), Err(PackageError::TooLarge)));

    // A zip bomb in small: 9 MiB of text deflate to a few KiB, the unpacked limit catches it.
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let deflate = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, data) in good_files() {
        writer
            .start_file(name.as_str(), deflate)
            .expect("start file");
        writer.write_all(&data).expect("write");
    }
    writer.start_file("README.md", deflate).expect("start file");
    writer
        .write_all(&vec![b'a'; 9 * 1024 * 1024])
        .expect("write");
    let bytes = writer.finish().expect("finish").into_inner();
    assert!(matches!(
        Package::read(&bytes),
        Err(PackageError::UnpackedTooLarge)
    ));
}

#[test]
fn rejects_missing_files_bad_assets_and_case_clashes() {
    assert!(
        matches!(with(|f| f.retain(|(n, _)| n != "plugin.wasm")), Err(PackageError::MissingFile(n)) if n == "plugin.wasm")
    );
    assert!(matches!(
        with(|f| f.retain(|(n, _)| n != "i18n/en.json")),
        Err(PackageError::MissingFile(_))
    ));
    assert!(matches!(
        with(|f| f.retain(|(n, _)| n != "manifest.json")),
        Err(PackageError::MissingFile(_))
    ));
    assert!(matches!(
        with(|f| f.push(("assets/run.js".into(), vec![]))),
        Err(PackageError::BadAsset(_))
    ));
    assert!(matches!(
        with(|f| f.push(("assets/huge.png".into(), vec![0; 512 * 1024 + 1]))),
        Err(PackageError::BadAsset(_))
    ));
    assert!(matches!(
        with(|f| f.push(("Plugin.wasm".into(), vec![]))),
        Err(PackageError::DuplicateEntry(_))
    ));
    assert!(matches!(
        with(|f| f[0].1 = MANIFEST.replace("\"games\"", "\"crypto\"").into_bytes()),
        Err(PackageError::Manifest(_))
    ));
}

#[test]
fn unofficial_packages_cannot_take_official_or_reserved_ids() {
    let official = |id: &str| id == "echolings";
    assert!(matches!(
        check_unofficial_id("echolings", official),
        Err(PackageError::OfficialId(_))
    ));
    assert!(matches!(
        check_unofficial_id("mumcord-pets", official),
        Err(PackageError::ReservedId(_))
    ));
    assert!(matches!(
        check_unofficial_id("core-tools", official),
        Err(PackageError::ReservedId(_))
    ));
    assert!(check_unofficial_id("my-pets", official).is_ok());
    let error = check_unofficial_id("echolings", official)
        .expect_err("refused")
        .to_string();
    assert!(error.contains("official plugin"), "{error}");
}
