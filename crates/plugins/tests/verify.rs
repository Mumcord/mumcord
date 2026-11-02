//! Signing and verification (FR-6.49 … FR-6.51, AC-6.18 logic) with keys generated per test run.

use std::io::Write;

use minisign::KeyPair;
use mumcord_plugins::verify::{
    IndexEntry, MISMATCH_TEXT, PERMISSIONS_TEXT, RegistryKeys, SIGNATURE_TEXT, VerifyFailure,
    verify_package,
};
use mumcord_plugins::{Digest, Source};
use zip::write::SimpleFileOptions;

const MANIFEST: &str = r#"{"manifest":1,"id":"dice-roller","name":"Dice","version":"1.0.0","api":"^1.0",
    "authors":["Tests"],"description":"Roll dice","categories":["games"],"license":"MIT",
    "permissions":{"required":["ui.panel"],"optional":[{"id":"events.voice"}]}}"#;

const REPO: Source = Source::Repo { repository_id: 1 };

fn package(manifest: &str) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, data) in [
        ("manifest.json", manifest.as_bytes()),
        ("plugin.wasm", b"\0asm".as_slice()),
        ("i18n/en.json", b"{}".as_slice()),
    ] {
        writer.start_file(name, options).expect("start file");
        writer.write_all(data).expect("write");
    }
    writer.finish().expect("finish").into_inner()
}

fn keypair() -> KeyPair {
    KeyPair::generate_unencrypted_keypair().expect("keypair")
}

fn sign(keys: &KeyPair, data: &[u8]) -> String {
    minisign::sign(Some(&keys.pk), &keys.sk, data, None, None)
        .expect("sign")
        .into_string()
}

fn entry(bytes: &[u8], author: &KeyPair) -> IndexEntry {
    IndexEntry {
        id: "dice-roller".into(),
        version: "1.0.0".into(),
        digest: Digest::of(bytes),
        author_keys: vec![author.pk.to_base64()],
        permissions: vec!["ui.panel".into(), "events.voice".into()],
    }
}

fn failure_text(result: Result<mumcord_plugins::verify::Verified, VerifyFailure>) -> String {
    match result {
        Ok(_) => panic!("expected a failure"),
        Err(e) => e.to_string(),
    }
}

#[test]
fn accepts_a_correctly_signed_package() {
    let author = keypair();
    let bytes = package(MANIFEST);
    let verified = verify_package(
        &bytes,
        &entry(&bytes, &author),
        &sign(&author, &bytes),
        REPO,
    )
    .expect("ok");
    assert_eq!(verified.package.manifest.id, "dice-roller");
    assert_eq!(verified.author_key, author.pk.to_base64());
    assert!(!verified.official);
}

#[test]
fn any_pinned_author_key_may_sign() {
    let (old, new) = (keypair(), keypair());
    let bytes = package(MANIFEST);
    let mut entry = entry(&bytes, &old);
    entry.author_keys.push(new.pk.to_base64());
    let verified = verify_package(&bytes, &entry, &sign(&new, &bytes), REPO).expect("ok");
    assert_eq!(verified.author_key, new.pk.to_base64());
}

#[test]
fn hash_and_size_mismatch_abort() {
    let author = keypair();
    let bytes = package(MANIFEST);
    let mut tampered = bytes.clone();
    *tampered.last_mut().expect("bytes") ^= 1;
    let signature = sign(&author, &tampered);
    assert_eq!(
        failure_text(verify_package(
            &tampered,
            &entry(&bytes, &author),
            &signature,
            REPO
        )),
        MISMATCH_TEXT
    );

    let mut longer = bytes.clone();
    longer.push(0);
    assert_eq!(
        failure_text(verify_package(
            &longer,
            &entry(&bytes, &author),
            &signature,
            REPO
        )),
        MISMATCH_TEXT
    );
}

#[test]
fn author_signature_must_come_from_a_pinned_key() {
    let (author, stranger) = (keypair(), keypair());
    let bytes = package(MANIFEST);
    let entry = entry(&bytes, &author);
    assert_eq!(
        failure_text(verify_package(
            &bytes,
            &entry,
            &sign(&stranger, &bytes),
            REPO
        )),
        SIGNATURE_TEXT
    );
    assert_eq!(
        failure_text(verify_package(&bytes, &entry, "not a signature", REPO)),
        SIGNATURE_TEXT
    );
}

#[test]
fn manifest_must_match_the_index_entry() {
    let author = keypair();
    let bytes = package(MANIFEST);
    let signature = sign(&author, &bytes);

    let mut other_version = entry(&bytes, &author);
    other_version.version = "1.0.1".into();
    assert_eq!(
        failure_text(verify_package(&bytes, &other_version, &signature, REPO)),
        MISMATCH_TEXT
    );

    let mut other_id = entry(&bytes, &author);
    other_id.id = "dice-other".into();
    assert_eq!(
        failure_text(verify_package(&bytes, &other_id, &signature, REPO)),
        MISMATCH_TEXT
    );

    let mut fewer = entry(&bytes, &author);
    fewer.permissions.retain(|p| p != "events.voice");
    assert_eq!(
        failure_text(verify_package(&bytes, &fewer, &signature, REPO)),
        PERMISSIONS_TEXT
    );

    // Listing more than the manifest asks for is fine.
    let mut more = entry(&bytes, &author);
    more.permissions.push("peers.messages".into());
    assert!(verify_package(&bytes, &more, &signature, REPO).is_ok());
}

#[test]
fn official_status_follows_the_source() {
    let author = keypair();
    let manifest = MANIFEST.replace("\"id\":\"dice-roller\"", "\"id\":\"dice\"");
    let mut files = Vec::new();
    {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut files));
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, data) in [
            ("manifest.json", manifest.as_bytes()),
            ("plugin.wasm", b"\0asm".as_slice()),
            ("i18n/en.json", b"{}".as_slice()),
            ("README.md", b"# Dice".as_slice()),
            ("LICENSE", b"AGPL".as_slice()),
        ] {
            writer.start_file(name, options).expect("start file");
            writer.write_all(data).expect("write");
        }
        writer.finish().expect("finish");
    }
    let mut entry = entry(&files, &author);
    entry.id = "dice".into();
    let verified = verify_package(&files, &entry, &sign(&author, &files), Source::Official)
        .expect("official package");
    assert!(verified.official);
}

#[test]
fn registry_files_need_a_trusted_key() {
    let (registry, stranger) = (keypair(), keypair());
    let index = br#"{"sequence":3}"#;
    let keys = RegistryKeys::pinned(&registry.pk.to_base64()).expect("key");
    assert!(keys.verify(index, &sign(&registry, index)).is_ok());

    for bad in [sign(&stranger, index), "garbage".into()] {
        assert!(matches!(
            keys.verify(index, &bad),
            Err(VerifyFailure::Signature)
        ));
    }
    let mut changed = index.to_vec();
    changed[0] = b' ';
    assert!(keys.verify(&changed, &sign(&registry, index)).is_err());

    // The whole `.pub` file is accepted as well as the bare key line.
    let pub_file = registry.pk.to_box().expect("box").into_string();
    assert!(RegistryKeys::pinned(&pub_file).is_some());
    assert!(RegistryKeys::pinned("nope").is_none());
    // A file signed by a test key is not trusted by the official keys alone.
    assert_eq!(
        RegistryKeys::official()
            .verify(index, &sign(&registry, index))
            .map_err(|e| e.to_string()),
        Err(SIGNATURE_TEXT.to_owned())
    );
}
