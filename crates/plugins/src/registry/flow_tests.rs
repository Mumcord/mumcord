//! End-to-end client flows against a local HTTP server with keys generated per test
//! (AC-6.18, AC-6.22 logic).

use std::path::Path;

use minisign::KeyPair;
use mumcord_store::PluginRepository;
use reqwest::Url;
use serde_json::json;

use super::index::tests::plugin;
use super::sync::{fetch_official_with, fetch_revoked_with};
use super::test_server::{TestServer, dead_base};
use super::*;
use crate::verify::{RegistryKeys, VerifyFailure};

fn keypair() -> KeyPair {
    KeyPair::generate_unencrypted_keypair().expect("test step")
}

fn public(keys: &KeyPair) -> String {
    keys.pk.to_base64()
}

fn sign(keys: &KeyPair, data: &[u8]) -> String {
    minisign::sign(Some(&keys.pk), &keys.sk, data, None, None)
        .expect("test step")
        .into_string()
}

fn client() -> Client {
    Client::new(UrlPolicy::new(true)).expect("test step")
}

fn publish(server: &TestServer, path: &str, keys: &KeyPair, body: &[u8]) {
    server.files.put(&format!("/{path}"), body);
    server
        .files
        .put(&format!("/{path}{SIGNATURE_SUFFIX}"), sign(keys, body));
}

fn official_index(sequence: u64) -> Vec<u8> {
    serde_json::to_vec(&json!({"format": 1, "sequence": sequence, "plugins": [plugin("dice")]}))
        .expect("test step")
}

fn base(text: &str) -> Url {
    official_base(Some(text)).expect("test step")
}

#[tokio::test]
async fn official_index_cache_offline_rollback_and_wrong_key() {
    let official = keypair();
    let keys = RegistryKeys::pinned(&public(&official)).expect("test step");
    let server = TestServer::start();
    let cache = tempfile::tempdir().expect("test step");
    let client = client();
    let root = base(&server.base);

    let none = fetch_official_with(&client, cache.path(), &root, None, &keys).await;
    assert!(matches!(none, Err(RegistryError::Status(404))));

    publish(&server, "index.json", &official, &official_index(7));
    let loaded = fetch_official_with(&client, cache.path(), &root, None, &keys)
        .await
        .expect("test step");
    assert!(matches!(loaded.freshness, Freshness::Fresh));
    assert_eq!(loaded.index.sequence, 7);
    let first = &server.files.requests()[1];
    assert!(first.contains("user-agent: Mumcord\r\n"), "{first}");
    assert!(!first.contains("cookie") && !first.contains("referer"));

    let again = fetch_official_with(&client, cache.path(), &root, Some(7), &keys)
        .await
        .expect("test step");
    assert!(matches!(again.freshness, Freshness::NotModified));
    assert!(
        server
            .files
            .requests()
            .last()
            .expect("test step")
            .contains("if-none-match")
    );

    // AC-6.18: a lower sequence is rejected and the cached list kept.
    publish(&server, "index.json", &official, &official_index(6));
    let rolled = fetch_official_with(&client, cache.path(), &root, Some(7), &keys)
        .await
        .expect("test step");
    assert!(matches!(
        rolled.freshness,
        Freshness::Rejected(RegistryError::Verify(VerifyFailure::Rollback))
    ));
    assert_eq!(rolled.index.sequence, 7);

    // AC-6.18: signed with a wrong key → "Signature check failed", cached list kept.
    publish(&server, "index.json", &keypair(), &official_index(8));
    let forged = fetch_official_with(&client, cache.path(), &root, Some(7), &keys)
        .await
        .expect("test step");
    match forged.freshness {
        Freshness::Rejected(error) => assert_eq!(error.to_string(), "Signature check failed"),
        other => panic!("{other:?}"),
    }
    assert_eq!(forged.index.sequence, 7);

    let offline = fetch_official_with(&client, cache.path(), &base(&dead_base()), Some(7), &keys)
        .await
        .expect("test step");
    assert!(matches!(offline.freshness, Freshness::Offline));
    assert_eq!(offline.index.plugins[0].id, "dice");

    let empty = tempfile::tempdir().expect("test step");
    let never = fetch_official_with(&client, empty.path(), &base(&dead_base()), None, &keys).await;
    assert_eq!(never.expect_err("must fail").to_string(), UNREACHABLE_TEXT);
}

#[tokio::test]
async fn release_policy_refuses_plain_http() {
    let client = Client::new(UrlPolicy::new(false)).expect("test step");
    let result = fetch_official(
        &client,
        Path::new("/nonexistent"),
        &base(&dead_base()),
        None,
    )
    .await;
    assert!(matches!(result, Err(RegistryError::InsecureUrl)));
}

#[tokio::test]
async fn revoked_merges_with_bundled_and_survives_offline() {
    let official = keypair();
    let keys = RegistryKeys::pinned(&public(&official)).expect("test step");
    let server = TestServer::start();
    let cache = tempfile::tempdir().expect("test step");
    let client = client();
    let list = serde_json::to_vec(&json!({"format": 1, "sequence": 2, "revoked": [
        {"id": "echolings", "severity": "disable", "reason": {"en": "Unsafe"}, "since": "2026-10-01"}
    ]}))
    .expect("test step");
    publish(&server, "revoked.json", &official, &list);

    let loaded = fetch_revoked_with(&client, cache.path(), &base(&server.base), &keys)
        .await
        .expect("test step");
    assert!(matches!(loaded.freshness, Freshness::Fresh));
    assert!(loaded.list.find("echolings", "1.0.0", "", None).is_some());

    let offline = fetch_revoked_with(&client, cache.path(), &base(&dead_base()), &keys)
        .await
        .expect("test step");
    assert!(matches!(offline.freshness, Freshness::Offline));
    assert!(offline.list.find("echolings", "1.0.0", "", None).is_some());

    let fresh_dir = tempfile::tempdir().expect("test step");
    let bundled = fetch_revoked_with(&client, fresh_dir.path(), &base(&dead_base()), &keys)
        .await
        .expect("test step");
    assert_eq!(bundled.list, RevocationList::bundled());
}

fn repo_index(keys: &KeyPair, sequence: u64, plugins: Vec<serde_json::Value>) -> serde_json::Value {
    json!({
        "format": 1, "sequence": sequence,
        "repository": {"name": "Foo Repo", "homepage": "https://foo.example",
                       "contact": "foo@example.org", "public_key": public(keys)},
        "plugins": plugins
    })
}

fn record(url: String, key: &KeyPair) -> PluginRepository {
    PluginRepository {
        id: 1,
        url,
        name: "Foo Repo".into(),
        public_key: public(key),
        pinned_at: 0,
        enabled: true,
        paused_reason: None,
        last_sequence: 0,
        last_checked: None,
    }
}

#[tokio::test]
async fn third_party_preview_fetch_key_change_and_rotation() {
    let old = keypair();
    let new = keypair();
    let server = TestServer::start();
    let client = client();
    let url = format!("{}repo/index.json", server.base);
    let is_official = |id: &str| id == "poll";
    let body = serde_json::to_vec(&repo_index(&old, 1, vec![plugin("poll"), plugin("foo")]))
        .expect("test step");
    publish(&server, "repo/index.json", &old, &body);

    // AC-6.22: fingerprint shown; `poll` ignored.
    let preview = preview_repository(&client, &url, None, &is_official)
        .await
        .expect("test step");
    assert_eq!(
        preview.fingerprint,
        key_fingerprint(&public(&old)).expect("test step")
    );
    assert_eq!(preview.info.name, "Foo Repo");
    assert_eq!(preview.info.contact.as_deref(), Some("foo@example.org"));
    assert_eq!(preview.plugin_count, 1);
    assert_eq!(preview.ignored[0].reason, IgnoredReason::OfficialId);
    let expected = preview_repository(&client, &url, Some(&public(&new)), &is_official).await;
    assert!(matches!(expected, Err(RegistryError::ExpectedKeyMismatch)));
    assert!(
        preview_repository(&client, &url, Some(&public(&old)), &is_official)
            .await
            .is_ok()
    );

    let cache = tempfile::tempdir().expect("test step");
    let repository = record(url.clone(), &old);
    match fetch_repository(&client, cache.path(), &repository, &is_official)
        .await
        .expect("test step")
    {
        RepoOutcome::Loaded(loaded) => assert_eq!(loaded.index.plugins[0].id, "foo"),
        other => panic!("{other:?}"),
    }

    // AC-6.22: a new key without a rotation entry pauses the repository.
    let body = serde_json::to_vec(&repo_index(&new, 2, vec![plugin("foo")])).expect("test step");
    publish(&server, "repo/index.json", &new, &body);
    match fetch_repository(&client, cache.path(), &repository, &is_official)
        .await
        .expect("test step")
    {
        RepoOutcome::KeyChanged {
            new_key,
            fingerprint,
        } => {
            assert_eq!(new_key, public(&new));
            assert_eq!(
                fingerprint,
                key_fingerprint(&public(&new)).expect("test step")
            );
        }
        other => panic!("{other:?}"),
    }

    // FR-6.75: a rotation signed by the old key is accepted.
    let mut rotated = repo_index(&new, 3, vec![plugin("foo")]);
    rotated["key_rotation"] =
        json!({"new_key": public(&new), "signature": sign(&old, public(&new).as_bytes())});
    publish(
        &server,
        "repo/index.json",
        &new,
        &serde_json::to_vec(&rotated).expect("test step"),
    );
    match fetch_repository(&client, cache.path(), &repository, &is_official)
        .await
        .expect("test step")
    {
        RepoOutcome::Rotated { new_key, loaded } => {
            assert_eq!(new_key, public(&new));
            assert_eq!(loaded.index.sequence, 3);
        }
        other => panic!("{other:?}"),
    }

    // A rotation signed by anyone else is a key change.
    rotated["key_rotation"]["signature"] = json!(sign(&new, public(&new).as_bytes()));
    publish(
        &server,
        "repo/index.json",
        &new,
        &serde_json::to_vec(&rotated).expect("test step"),
    );
    let fresh = tempfile::tempdir().expect("test step");
    assert!(matches!(
        fetch_repository(&client, fresh.path(), &repository, &is_official)
            .await
            .expect("test step"),
        RepoOutcome::KeyChanged { .. }
    ));
}

#[tokio::test]
async fn third_party_index_size_cap() {
    let keys = keypair();
    let server = TestServer::start();
    let mut index = repo_index(&keys, 1, vec![]);
    index["padding"] = json!("x".repeat(MAX_REPOSITORY_INDEX_BYTES));
    publish(
        &server,
        "big/index.json",
        &keys,
        &serde_json::to_vec(&index).expect("test step"),
    );
    let url = format!("{}big/index.json", server.base);
    let result = preview_repository(&client(), &url, None, &|_| false).await;
    assert!(matches!(result, Err(RegistryError::TooLarge(_))));
}

#[test]
fn repository_url_rules() {
    assert!(check_repository_url("https://foo.example/index.json", false).is_ok());
    assert!(matches!(
        check_repository_url("http://foo.example/index.json", true),
        Err(RegistryError::InsecureUrl)
    ));
    assert!(check_repository_url("http://localhost:9000/index.json", true).is_ok());
    assert!(check_repository_url("http://localhost:9000/index.json", false).is_err());
    assert!(check_repository_url("not a url", true).is_err());
}

#[tokio::test]
async fn download_returns_package_and_author_signature() {
    let author = keypair();
    let server = TestServer::start();
    publish(&server, "dice-1.0.0.mcplugin", &author, b"package bytes");
    let url = format!("{}dice-1.0.0.mcplugin", server.base);
    let download = client().download(&url, 13).await.expect("test step");
    assert_eq!(download.bytes, b"package bytes");
    assert_eq!(download.signature, sign_check(&author, &download));
    let short = client().download(&url, 5).await;
    assert!(matches!(
        short,
        Err(RegistryError::Verify(VerifyFailure::Mismatch))
    ));
}

/// The served signature verifies with the author key.
fn sign_check(author: &KeyPair, download: &Download) -> String {
    RegistryKeys::pinned(&public(author))
        .expect("test step")
        .verify(&download.bytes, &download.signature)
        .expect("test step");
    download.signature.clone()
}
