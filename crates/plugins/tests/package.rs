//! `.mcplugin` packages built in memory: the good case and each way a hostile package can be bad.

use std::io::Write;

use mumcord_plugins::package::{MAX_PACKAGE_BYTES, check_unofficial_id};
use mumcord_plugins::{Digest, ExpectedDigest, ManifestError, Package, PackageError, Source};
use zip::write::SimpleFileOptions;

const MANIFEST: &str = r#"{"manifest":1,"id":"dice-roller","name":"Dice","version":"1.0.0","api":"^1.0",
    "authors":["Tests"],"description":"Roll dice","categories":["games"],"license":"MIT",
    "icon":"assets/icon-64.png"}"#;

const REPO: Source = Source::Repo { repository_id: 1 };

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
    Package::read(&zip(&files), REPO)
}

fn manifest_with(permissions: &str) -> Vec<u8> {
    MANIFEST
        .replace(
            "\"license\"",
            &format!("\"permissions\":{permissions},\"license\""),
        )
        .into_bytes()
}

#[test]
fn accepts_and_installs_a_good_package() {
    let bytes = zip(&good_files());
    let package =
        Package::read_verified(&bytes, REPO, &ExpectedDigest(Digest::of(&bytes))).expect("valid");
    assert_eq!(package.manifest.id, "dice-roller");

    let root = tempfile::tempdir().expect("temp dir");
    let dir = package.install(root.path()).expect("installs");
    assert_eq!(dir, root.path().join("dice-roller").join("1.0.0"));
    assert_eq!(
        std::fs::read(dir.join("plugin.wasm")).expect("read"),
        b"\0asm"
    );
    // Installing again replaces the old files and leaves no staging directory behind.
    std::fs::write(dir.join("stale.txt"), b"old").expect("write");
    package.install(root.path()).expect("reinstalls");
    assert!(!dir.join("stale.txt").exists());
    let entries = |path: &std::path::Path| std::fs::read_dir(path).expect("list").count();
    assert_eq!(entries(root.path()), 1);
    assert_eq!(entries(&root.path().join("dice-roller")), 1);

    // A new version goes next to the old one, which stays for rollback.
    let mut files = good_files();
    files[0].1 = MANIFEST.replace("1.0.0", "1.1.0").into_bytes();
    let newer = Package::read(&zip(&files), REPO).expect("valid");
    let newer_dir = newer.install(root.path()).expect("installs");
    assert_eq!(newer_dir, root.path().join("dice-roller").join("1.1.0"));
    assert!(dir.join("plugin.wasm").exists());
    assert_eq!(entries(&root.path().join("dice-roller")), 2);
}

#[test]
fn a_failed_install_leaves_nothing_behind() {
    let package = Package::read(&zip(&good_files()), REPO).expect("valid");
    let root = tempfile::tempdir().expect("temp dir");
    // A file where the plugin's directory should be makes the install fail.
    std::fs::write(root.path().join("dice-roller"), b"in the way").expect("write");
    assert!(matches!(
        package.install(root.path()),
        Err(PackageError::Io(_))
    ));
    assert_eq!(std::fs::read_dir(root.path()).expect("list").count(), 1);
}

#[test]
fn digest_must_match_the_index() {
    let bytes = zip(&good_files());
    let mut expected = Digest::of(&bytes);
    assert_eq!(expected.sha256_hex().len(), 64);
    expected.sha256[0] ^= 1;
    assert!(matches!(
        Package::read_verified(&bytes, REPO, &ExpectedDigest(expected)),
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
        Package::read(&bytes, REPO),
        Err(PackageError::BadEntry(_))
    ));
}

#[test]
fn rejects_too_many_files_and_oversize_packages() {
    let result = with(|f| f.extend((0..200).map(|i| (format!("i18n/x{i}.json"), Vec::new()))));
    assert!(matches!(result, Err(PackageError::TooManyFiles(200))));

    let official = |extra: usize| {
        let mut files = good_files();
        files.push(("README.md".into(), b"# Dice".to_vec()));
        files.push(("LICENSE".into(), b"MIT".to_vec()));
        let have = files.len();
        files.extend((0..400 - have + extra).map(|i| {
            let (a, b) = (b'k' + (i / 26) as u8, b'a' + (i % 26) as u8);
            (
                format!("i18n/{}{}.json", a as char, b as char),
                b"{}".to_vec(),
            )
        }));
        Package::read(&zip(&files), Source::Official)
    };
    assert!(official(0).is_ok());
    assert!(matches!(official(1), Err(PackageError::TooManyFiles(400))));

    let big = vec![0u8; MAX_PACKAGE_BYTES as usize + 1];
    assert!(matches!(
        Package::read(&big, REPO),
        Err(PackageError::TooLarge)
    ));

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
        Package::read(&bytes, REPO),
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
    assert!(matches!(
        with(|f| f.retain(|(n, _)| n != "assets/icon-64.png")),
        Err(PackageError::MissingFile(n)) if n == "assets/icon-64.png"
    ));
}

#[test]
fn only_the_known_layout_is_allowed() {
    for name in [
        "run.sh",
        "plugin2.wasm",
        "docs/notes.md",
        "i18n/english.json",
        "i18n/de.txt",
        "web/app.exe",
    ] {
        let result = with(|f| f.push((name.into(), b"x".to_vec())));
        assert!(
            matches!(result, Err(PackageError::UnexpectedFile(_))),
            "{name}: {result:?}"
        );
    }
    let ok = with(|f| {
        f.push(("i18n/pt-BR.json".into(), b"{}".to_vec()));
        f.push(("README.md".into(), b"# Dice".to_vec()));
        f.push(("LICENSE".into(), b"MIT".to_vec()));
    });
    assert!(ok.is_ok(), "{ok:?}");
    assert!(matches!(
        with(|f| f.push(("i18n/de.json".into(), vec![b' '; 64 * 1024 + 1]))),
        Err(PackageError::BadTranslation(_))
    ));
}

#[test]
fn web_content_needs_the_webview_permission() {
    let web = |f: &mut Vec<(String, Vec<u8>)>| f.push(("web/index.html".into(), b"<p>".to_vec()));
    assert!(matches!(with(web), Err(PackageError::WebWithoutWebview)));

    let mut files = good_files();
    files[0].1 = manifest_with(r#"{"optional":["ui.webview"]}"#);
    web(&mut files);
    files.push(("README.md".into(), b"# Dice".to_vec()));
    files.push(("LICENSE".into(), b"MIT".to_vec()));
    assert!(Package::read(&zip(&files), Source::Official).is_ok());
}

#[test]
fn official_only_permissions_are_refused_outside_the_official_index() {
    for permission in ["ui.webview", "net.http", "secrets"] {
        let mut files = good_files();
        files[0].1 = if permission == "net.http" {
            MANIFEST
                .replace(
                    "\"license\"",
                    r#""net":{"hosts":"user-configured"},"permissions":{"required":["net.http"]},"license""#,
                )
                .into_bytes()
        } else {
            manifest_with(&format!(r#"{{"required":["{permission}"]}}"#))
        };
        for source in [REPO, Source::Dev] {
            let error = Package::read(&zip(&files), source).expect_err("refused");
            assert!(
                matches!(
                    error,
                    PackageError::Manifest(ManifestError::OfficialOnly(_))
                ),
                "{permission}: {error:?}"
            );
            assert_eq!(
                error.to_string(),
                "This permission is only available to official plugins"
            );
        }
    }
}

#[test]
fn official_packages_need_readme_and_licence() {
    let bytes = zip(&good_files());
    assert!(matches!(
        Package::read(&bytes, Source::Official),
        Err(PackageError::MissingFile(n)) if n == "README.md"
    ));
    let mut files = good_files();
    files.push(("README.md".into(), b"# Dice".to_vec()));
    assert!(matches!(
        Package::read(&zip(&files), Source::Official),
        Err(PackageError::MissingFile(n)) if n == "LICENSE"
    ));
    files.push(("LICENSE".into(), b"MIT".to_vec()));
    assert!(Package::read(&zip(&files), Source::Official).is_ok());
}

#[test]
fn svg_assets_are_sanitised_before_install() {
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" onload="alert(1)"><script>alert(1)</script><circle r="4"/></svg>"#;
    let package = with(|f| f.push(("assets/logo.svg".into(), svg.to_vec()))).expect("valid");
    let root = tempfile::tempdir().expect("temp dir");
    let dir = package.install(root.path()).expect("installs");
    let installed = std::fs::read_to_string(dir.join("assets/logo.svg")).expect("read");
    assert!(!installed.contains("alert"), "{installed}");
    assert!(installed.contains("<circle r=\"4\">"), "{installed}");
    assert!(matches!(
        with(|f| f.push(("assets/fake.svg".into(), b"<p>not an image</p>".to_vec()))),
        Err(PackageError::BadAsset(_))
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
