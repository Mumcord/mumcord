//! Ban list read, edit and save against the local test server (issue #254), and the result of
//! spike S1 of spec 10 FR-10.68. Start the server first, then:
//! `MUMCORD_LIVE=1 cargo test -p mumcord-protocol --test live_ban_list -- --ignored --test-threads=1`
//!
//! What the run measures on mumble-server 1.5.735 with the default settings:
//! - a `BanList` message without `query` replaces the server's list, so an empty one clears it;
//! - the server answers `start` in the extended ISO form, which `ban::parse_iso` reads, and keeps the
//!   subnet length it was given. It counts the length on the 16-byte wire address, so a stored 32
//!   covers only the first four bytes of that form and matches every IPv4 client: Mumcord writes a
//!   single IPv4 as 128 and shows it as /32 (issue #253);
//! - at connect the server matches the **address** part of an entry; the certificate is only reached
//!   when the address matches too, so an entry with a documentation address reaches nobody. Mumcord
//!   therefore writes the address the server reports for the person (FR-10.14): a ban by
//!   certificate alone does not work on a standard server;
//! - the query is answered only with the Ban right in the root channel.
//!
//! The test server is shared by the worktrees, so the run starts and ends with an empty list.

use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use mumcord_protocol::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use mumcord_protocol::{
    ClientIdentity, ConnectConfig, DenyKind, Event, ban, cert_hash, connect, permissions, wire_mask,
};
use tokio::sync::mpsc::UnboundedReceiver;

const PORT: u16 = 64738;
const WAIT: Duration = Duration::from_secs(10);

/// The same certificate again, so one client can be checked before and after an entry is written.
struct TestIdentity {
    cert: Vec<u8>,
    key: Vec<u8>,
    hash: String,
}

fn identity(name: &str) -> TestIdentity {
    let generated = rcgen::generate_simple_self_signed(vec![name.to_owned()]).expect("cert");
    TestIdentity {
        cert: generated.cert.der().to_vec(),
        key: generated.signing_key.serialize_der(),
        hash: cert_hash(&CertificateDer::from(generated.cert.der().to_vec())),
    }
}

impl TestIdentity {
    fn config(&self, username: &str, admin: bool) -> ConnectConfig {
        ConnectConfig {
            host: "127.0.0.1".to_owned(),
            port: PORT,
            username: username.to_owned(),
            password: if admin { superuser_password() } else { None },
            tokens: Vec::new(),
            identity: Some(ClientIdentity {
                chain: vec![CertificateDer::from(self.cert.clone())],
                key: PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.key.clone())),
            }),
            verify_cert: Arc::new(|_| true),
            login_timeout: WAIT,
        }
    }
}

fn superuser_password() -> Option<String> {
    let from_env = std::env::var("SUPERUSER_PASSWORD")
        .ok()
        .filter(|p| !p.is_empty());
    from_env.or_else(|| {
        let dir = std::env::var("MUMCORD_TESTENV_DIR").ok()?;
        std::fs::read_to_string(format!("{dir}/.env"))
            .ok()?
            .lines()
            .filter_map(|line| line.split_once('='))
            .find(|(key, _)| *key == "SUPERUSER_PASSWORD")
            .map(|(_, value)| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    })
}

async fn read_list(events: &mut UnboundedReceiver<Event>) -> Vec<ban::Ban> {
    tokio::time::timeout(WAIT, async {
        loop {
            match events.recv().await {
                Some(Event::BanList(list)) => return list,
                Some(Event::PermissionDenied(denied)) => panic!("refused: {denied:?}"),
                Some(_) => {}
                None => panic!("stream ended"),
            }
        }
    })
    .await
    .expect("no ban list answer")
}

fn entry(hash: Option<String>, address: IpAddr, mask: u32, reason: &str) -> ban::Ban {
    ban::Ban {
        address,
        mask,
        name: Some("ban-test".into()),
        hash,
        reason: Some(reason.to_owned()),
        start: Some(ban::format_start(SystemTime::now())),
        duration: 3600,
    }
}

#[tokio::test]
#[ignore = "needs the local test server (testenv/up.sh v15)"]
async fn reads_edits_and_saves_the_ban_list() {
    if std::env::var("MUMCORD_LIVE").is_ok_and(|v| v != "1") {
        return;
    }
    let banned = identity("ban-banned-certificate");
    let doc: IpAddr = "203.0.113.1".parse().expect("address");
    let mut admin = connect(banned.config("SuperUser", true))
        .await
        .expect("SuperUser login");
    admin
        .handle
        .send_ban_list(&[])
        .expect("start from an empty list");
    admin.handle.flush().await.expect("flush");
    admin.handle.query_ban_list().expect("query");
    let mut list = read_list(&mut admin.events).await;
    assert!(list.is_empty(), "the test server starts without bans");

    // Without the Ban right the query is refused, so the menu item is not shown (FR-10.2).
    let guest = identity("ban-guest");
    let mut plain = connect(guest.config("ban-guest", false))
        .await
        .expect("guest login");
    plain.handle.query_ban_list().expect("query");
    let denied = tokio::time::timeout(WAIT, async {
        loop {
            match plain.events.recv().await {
                Some(Event::PermissionDenied(d)) => return d,
                Some(_) => {}
                None => panic!("stream ended"),
            }
        }
    })
    .await
    .expect("no denial");
    assert_eq!(denied.kind, DenyKind::Permission);
    assert_eq!(denied.permission, Some(permissions::BAN));
    plain.handle.disconnect();

    // Add: one entry with a certificate and one with an address only (spike S1).
    list.push(entry(
        Some(banned.hash.clone()),
        doc,
        32,
        "documentation address",
    ));
    list.push(entry(
        None,
        "198.51.100.9".parse().expect("address"),
        32,
        "address only",
    ));
    admin.handle.send_ban_list(&list).expect("save");
    admin.handle.flush().await.expect("flush");
    admin.handle.query_ban_list().expect("query");
    let saved = read_list(&mut admin.events).await;
    assert_eq!(saved.len(), 2, "the server takes the whole list");
    assert!(
        saved.iter().all(|b| b.mask == 32),
        "a single IPv4 reads back as the family's /32"
    );
    assert_eq!(
        wire_mask(doc, 32),
        128,
        "and goes out in the length the server counts"
    );

    // Edit the reason and the end of one entry; the change is visible on the next read (AC-10.2).
    let mut edited = saved.clone();
    for b in &mut edited {
        if b.hash.as_deref() == Some(banned.hash.as_str()) {
            b.reason = Some("spam links".into());
            b.duration = 86_400;
        }
    }
    admin.handle.send_ban_list(&edited).expect("save");
    admin.handle.flush().await.expect("flush");
    admin.handle.query_ban_list().expect("query");
    let after = read_list(&mut admin.events).await;
    let row = after
        .iter()
        .find(|b| b.hash.as_deref() == Some(banned.hash.as_str()));
    assert_eq!(row.map(|b| b.reason.as_deref()), Some(Some("spam links")));
    assert_eq!(row.map(|b| b.duration), Some(86_400));

    // What an entry reaches at connect is measured in `live_kick_ban` (issue #253): the server
    // compares the address part, so a documentation address reaches nobody, which is why Mumcord
    // writes the address it reports for the person.
    admin.handle.disconnect();
}

#[tokio::test]
#[ignore = "needs the local test server (testenv/up.sh v15)"]
async fn a_single_address_is_banned_only_for_that_address() {
    if std::env::var("MUMCORD_LIVE").is_ok_and(|v| v != "1") {
        return;
    }
    let untouched = identity("mask-spike-untouched");
    let doc: IpAddr = "203.0.113.1".parse().expect("address");
    let admin = connect(untouched.config("SuperUser", true))
        .await
        .expect("SuperUser login");

    // An entry that carries 32 while the address is 16 bytes long matches every IPv4 client, so
    // Mumcord writes the wire length instead: with it a documentation address leaves other clients
    // free (issue #253).
    admin
        .handle
        .send_ban_list(&[entry(None, doc, 32, "documentation")])
        .expect("save");
    admin.handle.flush().await.expect("flush");

    let other = identity("mask-spike-other");
    let free = connect(other.config("mask-spike-other", false)).await;
    assert!(free.is_ok(), "another address is not touched by that entry");

    admin.handle.send_ban_list(&[]).expect("unban");
    admin.handle.flush().await.expect("flush");
    if let Ok(connection) = free {
        connection.handle.disconnect();
    }
    admin.handle.disconnect();
}
