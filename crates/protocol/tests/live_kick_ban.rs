//! Kick and ban of a person against the local test server (issue #253, AC-10.6–AC-10.9).
//! Start the server first, then:
//! `MUMCORD_LIVE=1 cargo test -p mumcord-protocol --test live_kick_ban -- --ignored --test-threads=1`
//!
//! What the run measures on mumble-server 1.5.735 with the default settings:
//! - the kicked client receives `UserRemove` with the reason and can connect again afterwards;
//! - a kick without the right is refused with `PermissionDenied`, and the server reports the Kick
//!   and Ban mask it checked, so the menu item is not offered to that user (FR-10.2, FR-10.60);
//! - `UserRemove` with `ban = true` takes the server's own default length and both targets, which is
//!   why a chosen length or a single target goes through the ban list (FR-10.12, FR-10.13);
//! - an entry added to the list ends at start + duration and blocks that client for the length
//!   Mumcord chose, while `UserRemove ban = true` uses the server's default length;
//! - `UserStats` carries the address only to admins (FR-10.65).
//!
//! The test server is shared by the worktrees, so the run clears the list at the start and after
//! every entry it writes.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use mumcord_protocol::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use mumcord_protocol::{
    ClientIdentity, ConnectConfig, DisconnectReason, Event, ban, cert_hash, connect, permissions,
    wire_mask,
};
use tokio::sync::mpsc::UnboundedReceiver;

const PORT: u16 = 64738;
const WAIT: Duration = Duration::from_secs(10);

/// A certificate the same client can present again after it is kicked, so the ban entry matches it
/// on the second connect (issue #253).
struct SameIdentity {
    name: String,
    cert: Vec<u8>,
    key: Vec<u8>,
    hash: String,
}

fn identity(name: &str) -> SameIdentity {
    let generated = rcgen::generate_simple_self_signed(vec![name.to_owned()]).expect("cert");
    SameIdentity {
        name: name.to_owned(),
        cert: generated.cert.der().to_vec(),
        key: generated.signing_key.serialize_der(),
        hash: cert_hash(&CertificateDer::from(generated.cert.der().to_vec())),
    }
}

impl SameIdentity {
    fn client(&self) -> ClientIdentity {
        ClientIdentity {
            chain: vec![CertificateDer::from(self.cert.clone())],
            key: PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.key.clone())),
        }
    }

    fn config(&self, admin: bool) -> ConnectConfig {
        config(&self.name, self.client(), admin)
    }
}

fn raw_identity(name: &str) -> ClientIdentity {
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

async fn connect_admin() -> mumcord_protocol::Connection {
    // SuperUser is the login name plus the password; the certificate only identifies the client.
    connect(config("SuperUser", raw_identity("kick-ban-admin"), true))
        .await
        .expect("SuperUser login")
}

async fn read_denial(events: &mut UnboundedReceiver<Event>) -> Event {
    tokio::time::timeout(WAIT, async {
        loop {
            match events.recv().await {
                Some(event @ Event::PermissionDenied(_)) => return event,
                Some(_) => {}
                None => panic!("stream ended"),
            }
        }
    })
    .await
    .expect("no refusal")
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

/// The person's own connection figures, as the server answers the admin's request.
async fn read_stats(
    events: &mut UnboundedReceiver<Event>,
) -> Box<mumcord_protocol::proto::UserStats> {
    tokio::time::timeout(WAIT, async {
        loop {
            match events.recv().await {
                Some(Event::UserStats(stats)) => return stats,
                Some(Event::PermissionDenied(denied)) => panic!("refused: {denied:?}"),
                Some(_) => {}
                None => panic!("stream ended"),
            }
        }
    })
    .await
    .expect("no stats answer")
}

#[tokio::test]
#[ignore = "needs the local test server (testenv/up.sh v15)"]
async fn kicks_with_a_reason_and_bans_with_the_chosen_length() {
    if std::env::var("MUMCORD_LIVE").is_ok_and(|v| v != "1") {
        return;
    }
    let guest = identity("kick-ban-guest");
    let quick = identity("kick-ban-quick");
    let mut admin = connect_admin().await;
    // Every run starts and ends with an empty list: the entry below covers the address this test
    // environment connects from, so it must not be left standing for the other worktrees.
    admin.handle.send_ban_list(&[]).expect("clear");
    admin.handle.flush().await.expect("flush");

    // A user without the right is refused, and the server names the mask it checked (FR-10.2).
    let admin_session = admin
        .handle
        .with_state(|state| state.session)
        .expect("admin session");
    let mut plain = connect(config(
        "kick-ban-plain",
        raw_identity("kick-ban-plain"),
        false,
    ))
    .await
    .expect("plain login");
    plain
        .handle
        .remove_user(admin_session, Some("no right"), false)
        .expect("send");
    let denied = read_denial(&mut plain.events).await;
    assert_eq!(
        match denied {
            Event::PermissionDenied(d) => d.permission,
            _ => panic!("expected a refusal"),
        },
        Some(permissions::KICK | permissions::BAN)
    );
    plain.handle.disconnect();

    let mut second = connect(guest.config(false))
        .await
        .expect("second client login");
    let session = second
        .handle
        .with_state(|state| state.session)
        .expect("session");

    // The address the ban entry needs is what only an admin gets in UserStats (FR-10.65).
    admin
        .handle
        .request_user_stats(session, false)
        .expect("stats");
    let stats = read_stats(&mut admin.events).await;
    assert_eq!(stats.session, Some(session));
    let address = stats
        .address
        .as_deref()
        .and_then(ban::read_address)
        .expect("address");
    // A single address goes out in the length the server counts on the 16-byte wire form, which the
    // UI still shows as /32 (issue #253).
    assert_eq!(wire_mask(address, 32), 128);

    // Kick with a reason: the person sees the reason and can come back (AC-10.6). For the person who
    // is removed the message ends the connection as `DisconnectReason::Kicked`.
    admin
        .handle
        .remove_user(session, Some("spam"), false)
        .expect("kick");
    admin.handle.flush().await.expect("flush");
    let kicked = tokio::time::timeout(WAIT, async {
        loop {
            match second.events.recv().await {
                Some(Event::Disconnected(DisconnectReason::Kicked { reason, ban, .. })) => {
                    return (reason, ban);
                }
                Some(_) => {}
                None => panic!("stream ended"),
            }
        }
    })
    .await
    .expect("no kick delivered");
    assert_eq!(kicked.0.as_deref(), Some("spam"));
    assert!(!kicked.1, "a kick is not a ban");

    let again = connect(guest.config(false))
        .await
        .expect("the kicked client can reconnect");

    // Precise ban with a chosen length (AC-10.7): the entry Mumcord writes is what the server keeps,
    // its end is start plus the length, and the client is refused (issue #254, spike S1).
    let entry = ban::Ban {
        address,
        mask: 32,
        name: Some("kick-ban-second".into()),
        hash: Some(guest.hash.clone()),
        reason: Some("spam links".into()),
        start: Some(ban::format_start(SystemTime::now())),
        duration: 3600,
    };
    admin.handle.send_ban_list(&[entry]).expect("save");
    admin.handle.flush().await.expect("flush");
    admin.handle.query_ban_list().expect("query");
    let saved = read_list(&mut admin.events).await;
    assert_eq!(saved.len(), 1);
    assert_eq!(
        saved[0].duration, 3600,
        "the server keeps the chosen length"
    );
    assert_eq!(saved[0].mask, 32, "the family's own length reads back");
    assert!(
        saved[0].end_time().is_some(),
        "the entry has an end the UI can show"
    );

    again.handle.disconnect();
    assert!(
        connect(guest.config(false)).await.is_err(),
        "the banned client cannot connect"
    );
    admin.handle.send_ban_list(&[]).expect("unban");
    admin.handle.flush().await.expect("flush");

    // Quick ban: the standard message takes the server's own default length, which Mumcord cannot
    // choose, so a chosen length needs the list (FR-10.13). On this server that default is 0, which
    // the client reads as permanent.
    let quick_conn = connect(quick.config(false)).await.expect("quick login");
    let quick_session = quick_conn
        .handle
        .with_state(|state| state.session)
        .expect("session");
    admin
        .handle
        .remove_user(quick_session, Some("default length"), true)
        .expect("quick ban");
    admin.handle.flush().await.expect("flush");
    admin.handle.query_ban_list().expect("query");
    let after = read_list(&mut admin.events).await;
    assert_eq!(after.len(), 1, "the quick ban wrote one entry");
    assert_eq!(
        after[0].duration, 0,
        "this server's default length is permanent, which is why the dialog cannot choose a length
         when the list cannot be read (FR-10.13)"
    );
    assert_eq!(after[0].hash.as_deref(), Some(quick.hash.as_str()));
    assert_eq!(after[0].mask, 32, "the entry covers exactly that address");

    admin.handle.send_ban_list(&[]).expect("clear");
    admin.handle.flush().await.expect("flush");
}
