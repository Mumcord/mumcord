//! Ban list read, edit and save against the local test server (issue #254), and the result of
//! spike S1 of spec 10 FR-10.68. Start the server first, then:
//! `MUMCORD_LIVE=1 cargo test -p mumcord-protocol --test live_ban_list -- --ignored --test-threads=1`
//!
//! What the run measures on mumble-server 1.5.735 with the default settings:
//! - a `BanList` message without `query` replaces the server's list, so an empty one clears it;
//! - the server keeps the subnet length as the client sends it and answers `start` in extended
//!   ISO form, which `ban::parse_iso` reads;
//! - the certificate part of an entry is enforced for a registered user; for a guest the server
//!   matches the address only, and an entry with an empty hash matches any client whose stored
//!   certificate it does not know, so Mumcord never sends an entry without an address;
//! - the query is answered only with the Ban right in the root channel.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use mumcord_protocol::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use mumcord_protocol::{
    ClientIdentity, ConnectConfig, DenyKind, Event, ban, cert_hash, connect, permissions,
};
use tokio::sync::mpsc::UnboundedReceiver;

const PORT: u16 = 64738;
const WAIT: Duration = Duration::from_secs(10);

fn identity(name: &str) -> ClientIdentity {
    let generated = rcgen::generate_simple_self_signed(vec![name.to_owned()]).expect("cert");
    ClientIdentity {
        chain: vec![CertificateDer::clone(generated.cert.der())],
        key: PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
            generated.signing_key.serialize_der(),
        )),
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

fn config(name: &str, ident: ClientIdentity, admin: bool) -> ConnectConfig {
    ConnectConfig {
        host: "127.0.0.1".to_owned(),
        port: PORT,
        username: name.to_owned(),
        password: if admin { superuser_password() } else { None },
        tokens: Vec::new(),
        identity: Some(ident),
        verify_cert: Arc::new(|_| true),
        login_timeout: WAIT,
    }
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

fn entry(hash: Option<String>, address: &str, mask: u32, reason: &str) -> ban::Ban {
    ban::Ban {
        address: address.parse().expect("address"),
        mask,
        name: Some("ban-test".into()),
        hash,
        reason: Some(reason.to_owned()),
        start: Some(ban::format_start(SystemTime::now())),
        duration: 3600,
    }
}

async fn connect_admin() -> mumcord_protocol::Connection {
    connect(config("SuperUser", identity("ban-admin"), true))
        .await
        .expect("SuperUser login")
}

#[tokio::test]
#[ignore = "needs the local test server (testenv/up.sh v15)"]
async fn reads_edits_and_saves_the_ban_list() {
    if std::env::var("MUMCORD_LIVE").is_ok_and(|v| v != "1") {
        return;
    }
    let banned = identity("ban-banned-certificate");
    let hash = cert_hash(&banned.chain[0]);
    let mut admin = connect_admin().await;

    // Without the Ban right the query is refused, so the menu item is not shown (FR-10.2).
    let mut guest = connect(config("ban-guest", identity("ban-guest"), false))
        .await
        .expect("guest login");
    guest.handle.query_ban_list().expect("query");
    let denied = tokio::time::timeout(WAIT, async {
        loop {
            match guest.events.recv().await {
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
    guest.handle.disconnect();

    admin.handle.query_ban_list().expect("query");
    let mut list = read_list(&mut admin.events).await;
    assert!(list.is_empty(), "the test server starts without bans");

    // Add: one entry with a certificate and one with an address only (spike S1).
    list.push(entry(
        Some(hash.clone()),
        "203.0.113.1",
        32,
        "certificate only",
    ));
    list.push(entry(None, "198.51.100.9", 32, "address only"));
    admin.handle.send_ban_list(&list).expect("save");
    admin.handle.flush().await.expect("flush");
    admin.handle.query_ban_list().expect("query");
    let saved = read_list(&mut admin.events).await;
    assert_eq!(saved.len(), 2, "the server takes the whole list");
    assert!(
        saved.iter().all(|b| b.mask == 32),
        "the mask comes back as sent"
    );

    // Edit the reason and the end of one entry; the change is visible on the next read (AC-10.2).
    let mut edited = saved.clone();
    for b in &mut edited {
        if b.hash.as_deref() == Some(hash.as_str()) {
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
        .find(|b| b.hash.as_deref() == Some(hash.as_str()));
    assert_eq!(row.map(|b| b.reason.as_deref()), Some(Some("spam links")));
    assert_eq!(row.map(|b| b.duration), Some(86_400));

    // The certificate entry blocks that certificate for a registered user. For a guest the server
    // compares the address, and an entry with an empty hash matches any guest, so Mumcord always
    // sends an address with the entry (spike S1, FR-10.14).
    assert!(
        connect(config("SuperUser", banned, true)).await.is_err(),
        "the banned certificate cannot connect as a registered user"
    );

    // Unban: the empty list replaces the server's, and the client can connect again (AC-10.3).
    admin.handle.send_ban_list(&[]).expect("unban");
    admin.handle.flush().await.expect("flush");
    admin.handle.query_ban_list().expect("query");
    assert!(read_list(&mut admin.events).await.is_empty());
    let again = connect(config(
        "ban-after-unban",
        identity("ban-after-unban"),
        false,
    ))
    .await
    .expect("connect after the unban");
    again.handle.disconnect();
}
