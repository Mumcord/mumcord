//! Inline images in synced history travel as hash references (issue #402, AC-1.51e … AC-1.51h)
//! against the local test server: a 0.7.0 requester gets placeholders, fetches each distinct
//! image once over plugin data and ends with the holder's exact bodies; images it already has
//! are not fetched; a locked channel's images go only to people inside; a 0.6.2 client keeps
//! getting complete inline bodies. `testenv/up.sh v15`, then:
//! `MUMCORD_LIVE=1 cargo test -p mumcord-engine --features test-backend --test live_history_images -- --ignored --test-threads=1`
//! The 0.6.2 test needs `MUMCORD_OLD_PEER=$(scripts/build-old-peer.sh | tail -n 1)`.
//!
//! Direct (P2P) image streams are not exercised here: the test server's clients have no
//! consented direct connections, so every image goes the plugin-data way.
#![cfg(feature = "test-backend")]

mod common;

use std::collections::HashMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::Duration;

use image::{ImageFormat, Rgb, RgbImage};
use mumbot::{Bot, BotConfig, UserCheck, env};
use mumcord_engine::full_image::FullRef;
use mumcord_engine::inspector::{PacketView, TapDirection};
use mumcord_engine::store::Consent;
use mumcord_engine::{
    ChatMessageView, ConversationRef, Engine, EngineEvent, EventReceiver, PasswordChange,
    SecretStore, ServerFields, ServerView, Snapshot,
};
// The bot speaks the published protocol crate it was built with.
use mumbot::protocol::proto;

const WAIT: Duration = Duration::from_secs(30);
const REFERENCE: &str = "data-mumcord-image";
const HISTORY: &str = "mumcord.history";
const IMAGE: &str = "mumcord.image";

fn live() -> bool {
    let on = std::env::var("MUMCORD_LIVE").is_ok_and(|v| v == "1");
    if !on {
        eprintln!("skipped: set MUMCORD_LIVE=1 and start the test servers");
    }
    on
}

fn unique(prefix: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!("{prefix}-{:06x}", (nanos ^ std::process::id()) & 0xff_ffff)
}

fn work_dir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("work dir");
    dir
}

async fn wait_for<T>(
    rx: &mut EventReceiver,
    what: &str,
    mut pick: impl FnMut(&EngineEvent) -> Option<T>,
) -> T {
    let deadline = tokio::time::Instant::now() + WAIT;
    loop {
        let event = tokio::time::timeout_at(deadline, rx.recv())
            .await
            .unwrap_or_else(|_| panic!("timed out waiting for {what}"))
            .expect("event stream open");
        if let Some(found) = pick(&event) {
            return found;
        }
    }
}

async fn wait_message(rx: &mut EventReceiver, contains: &str) -> ChatMessageView {
    wait_for(rx, contains, |e| match e {
        EngineEvent::MessageReceived(m) if m.body.contains(contains) => Some((**m).clone()),
        _ => None,
    })
    .await
}

/// What a requester saw of one channel's sync: the first body of every synced message (in
/// arrival order) and its latest body by message id.
struct Synced {
    first: Vec<(String, String)>,
    latest: HashMap<String, String>,
    added: u32,
}

/// Collects a channel's sync until it has ended and every synced body has its images back.
/// A failed image load fails the test.
async fn wait_sync_restored(rx: &mut EventReceiver, channel: u32) -> Synced {
    let mut synced = Synced {
        first: Vec::new(),
        latest: HashMap::new(),
        added: 0,
    };
    let mut ended = false;
    wait_for(rx, "history sync with restored images", |e| {
        match e {
            EngineEvent::HistorySynced {
                channel_id,
                messages,
            } if *channel_id == channel => {
                for m in messages {
                    if !synced.latest.contains_key(&m.id) {
                        synced.first.push((m.id.clone(), m.body.clone()));
                    }
                    synced.latest.insert(m.id.clone(), m.body.clone());
                }
            }
            EngineEvent::HistorySyncStatus {
                channel_id,
                running: false,
                added,
            } if *channel_id == channel => {
                ended = true;
                synced.added = *added;
            }
            EngineEvent::HistoryImageStatus { reference, state } => {
                panic!("history image {reference} not loaded: {state:?}")
            }
            // Fetching synced images is background work: it never interrupts the user.
            EngineEvent::DirectConsentRequired => {
                panic!("a background history image fetch asked for direct connections")
            }
            _ => {}
        }
        (ended && synced.latest.values().all(|b| !b.contains(REFERENCE))).then_some(())
    })
    .await;
    synced
}

async fn start_engine(dir: &Path, name: &str) -> (Engine, EventReceiver) {
    start_engine_with(dir, name, Consent::No).await
}

/// An engine whose direct connection consent is `consent`.
async fn start_engine_with(dir: &Path, name: &str, consent: Consent) -> (Engine, EventReceiver) {
    let (engine, rx) = Engine::new(
        &dir.join("data"),
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
    )
    .expect("engine");
    engine
        .identity_create(name.to_owned(), None)
        .await
        .expect("identity");
    // Consent::No: plugin data only, which every test here measures.
    let mut settings = engine.settings().expect("settings");
    settings.allow_direct_connections = consent;
    engine.update_settings(settings).await.expect("consent");
    (engine, rx)
}

async fn add_server(engine: &Engine, name: &str) -> ServerView {
    engine
        .server_add(ServerFields {
            label: "test".into(),
            address: "127.0.0.1".into(),
            port: common::v15(),
            username: name.into(),
            password: match env::testenv_var("SERVER_PASSWORD") {
                Some(p) => PasswordChange::Set(p),
                None => PasswordChange::Keep,
            },
        })
        .await
        .expect("server")
}

/// Connects, trusting the server certificate on first use.
async fn connect(engine: &Engine, rx: &mut EventReceiver, server: &ServerView) -> Snapshot {
    engine.connect(server.id, None).expect("connect");
    loop {
        let event = wait_for(rx, "sync", |e| match e {
            EngineEvent::CertTrustRequired { .. } | EngineEvent::ServerSynced(_) => Some(e.clone()),
            _ => None,
        })
        .await;
        match event {
            EngineEvent::CertTrustRequired {
                server_id,
                fingerprint,
                ..
            } => engine
                .cert_trust_decide(server_id, &fingerprint, true)
                .expect("trust"),
            EngineEvent::ServerSynced(s) => return *s,
            _ => unreachable!(),
        }
    }
}

fn channel(snapshot: &Snapshot, name: &str) -> u32 {
    snapshot
        .channels
        .iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("channel {name}"))
        .id
}

fn session_of(snapshot: &Snapshot, name: &str) -> u32 {
    snapshot
        .users
        .iter()
        .find(|u| u.name == name)
        .unwrap_or_else(|| panic!("user {name}"))
        .session
}

fn in_channel(name: &str) -> UserCheck {
    UserCheck {
        in_channel: Some(name.into()),
        ..UserCheck::default()
    }
}

/// A temporary channel of its own keeps other clients' chatter out of the history. Only an
/// admin may create it; it moves its creator in and disappears when everybody has left, so the
/// returned admin must stay connected for the whole test. The bot `bot_name` is in the room.
async fn temp_room(dir: &Path, bot_name: &str) -> (Bot, Bot, String) {
    let bot = Bot::connect(BotConfig {
        data_dir: Some(dir.join("bot")),
        ..BotConfig::new(bot_name, common::v15())
    })
    .await
    .expect("bot");
    let room = unique("mc-hiroom");
    let admin = Bot::connect(BotConfig::superuser(common::v15()))
        .await
        .expect("SuperUser bot (SUPERUSER_PASSWORD in testenv/.env)");
    admin
        .handle()
        .send_channel_state(proto::ChannelState {
            parent: Some(0),
            name: Some(room.clone()),
            temporary: Some(true),
            ..Default::default()
        })
        .expect("create channel");
    admin
        .wait_user("SuperUser", &in_channel(&room))
        .await
        .expect("room created");
    // The bot learns of the new channel a moment after the admin did.
    for _ in 0..50 {
        if bot.channel_id(&room).is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    bot.join(&room).await.expect("bot joins the room");
    (admin, bot, room)
}

/// A noisy 36×36 PNG of about 4 KB: it does not compress, and its inline body stays well under
/// the 8 KiB a 0.6.2 client accepts.
fn picture(seed: u32) -> Vec<u8> {
    let mut state = seed | 1;
    let image = RgbImage::from_fn(36, 36, |_, _| {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        let [r, g, b, _] = state.to_le_bytes();
        Rgb([r, g, b])
    });
    let mut out = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
        .expect("png");
    out
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, &b)| n | u32::from(b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(TABLE[(n >> (18 - 6 * i)) as usize & 63]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// A chat message with a text and one inline picture.
fn image_message(text: &str, png: &[u8]) -> String {
    format!(
        "{text}<br /><img src=\"data:image/png;base64,{}\" />",
        base64(png)
    )
}

fn reference(png: &[u8]) -> String {
    FullRef::of(png).to_string()
}

fn hex_decode(hex: &str) -> Vec<u8> {
    (0..hex.len() / 2)
        .filter_map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok())
        .collect()
}

/// Packets `engine` sent with the plugin data id `kind` since packet `after`.
fn sent(engine: &Engine, after: Option<u64>, kind: &str) -> Vec<PacketView> {
    engine
        .poll_packet_capture(after)
        .packets
        .into_iter()
        .filter(|p| p.direction == TapDirection::Sent && p.kind == kind)
        .collect()
}

fn last_packet(engine: &Engine) -> Option<u64> {
    engine
        .poll_packet_capture(None)
        .packets
        .last()
        .map(|p| p.id)
}

fn bytes(packets: &[PacketView]) -> u64 {
    packets.iter().map(|p| u64::from(p.size)).sum()
}

/// The message stream of the `part` packets, without their headers, in sending order.
fn part_stream(packets: &[PacketView]) -> String {
    let mut stream = Vec::new();
    for packet in packets {
        let raw = hex_decode(&packet.raw_hex);
        let Some(split) = raw.iter().position(|&b| b == b'\n') else {
            continue;
        };
        if String::from_utf8_lossy(&raw[..split]).contains("\"t\":\"part\"") {
            stream.extend_from_slice(&raw[split + 1..]);
        }
    }
    String::from_utf8_lossy(&stream).into_owned()
}

/// Kinds (first byte) of the full-image messages A sent to `session`.
fn image_kinds_to(packets: &[PacketView], session: u32) -> Vec<u8> {
    packets
        .iter()
        .filter(|p| p.receivers.contains(&session))
        .filter_map(|p| hex_decode(&p.raw_hex).first().copied())
        .collect()
}

/// Bodies of `channel` stored by `engine`, oldest first.
fn stored(engine: &Engine, channel: u32) -> Vec<String> {
    engine
        .history_load(
            only_server(engine),
            ConversationRef::Channel {
                channel_id: channel,
            },
            None,
            50,
        )
        .expect("history")
        .into_iter()
        .map(|m| m.body)
        .collect()
}

/// The test's only connected server (0, which no call accepts, when none is).
fn only_server(engine: &Engine) -> mumcord_engine::ServerId {
    engine
        .connections()
        .first()
        .and_then(mumcord_engine::ConnState::server_id)
        .unwrap_or(0)
}

fn audio_off() {
    // SAFETY: set before any audio thread exists; tests run with --test-threads=1.
    unsafe { std::env::set_var("MUMCORD_AUDIO_BACKEND", "null") };
}

/// AC-1.51e and AC-1.51g: references first, each distinct image fetched once, exact bodies at
/// the end; nothing fetched again for a repeated sync or an image already held.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v15 (MUMCORD_LIVE=1)"]
async fn synced_images_travel_once_as_references() {
    if !live() {
        return;
    }
    audio_off();
    let dir = work_dir("engine-live-history-images");
    let (a_name, b_name, bot_name) = (unique("mc-hia"), unique("mc-hib"), unique("mc-hibot"));
    let (a, mut a_rx) = start_engine(&dir.join("a"), &a_name).await;
    let (b, mut b_rx) = start_engine(&dir.join("b"), &b_name).await;
    let (_admin, bot, room) = temp_room(&dir, &bot_name).await;
    let (red, blue) = (picture(11), picture(23));

    // A is in the room while three messages with two distinct pictures (one twice) are sent.
    let a_server = add_server(&a, &a_name).await;
    let snapshot = connect(&a, &mut a_rx, &a_server).await;
    let lobby = channel(&snapshot, &room);
    a.join_channel(only_server(&a), lobby)
        .expect("A joins the room");
    bot.wait_user(&a_name, &in_channel(&room))
        .await
        .expect("A in the room");
    for (text, png) in [("one red", &red), ("one blue", &blue), ("red again", &red)] {
        bot.send_channel(Some(&room), &image_message(text, png))
            .expect("send");
        let got = wait_message(&mut a_rx, text).await;
        assert!(got.body.contains(";base64,"), "A keeps the inline picture");
    }
    let originals = stored(&a, lobby);
    assert_eq!(originals.len(), 3);
    tokio::time::sleep(Duration::from_millis(1100)).await;

    // B (0.7.0) joins and syncs; A's packets to it are measured.
    a.set_packet_capture(true);
    a.clear_packet_capture();
    let b_server = add_server(&b, &b_name).await;
    connect(&b, &mut b_rx, &b_server).await;
    b.join_channel(only_server(&b), lobby)
        .expect("B joins the room");
    let synced = wait_sync_restored(&mut b_rx, lobby).await;
    assert_eq!(synced.added, 3);
    let first: Vec<&str> = synced.first.iter().map(|(_, b)| b.as_str()).collect();
    for body in &first {
        assert!(
            body.contains(REFERENCE) && !body.contains(";base64,"),
            "placeholders first: {body}"
        );
    }
    let restored: Vec<&str> = synced
        .first
        .iter()
        .map(|(id, _)| synced.latest[id].as_str())
        .collect();
    assert_eq!(
        restored, originals,
        "the same messages get their images back"
    );
    assert_eq!(stored(&b, lobby), originals, "B stores A's exact bodies");

    let history = sent(&a, None, HISTORY);
    let images = sent(&a, None, IMAGE);
    let stream = part_stream(&history);
    assert!(
        !stream.contains(";base64,") && stream.contains(REFERENCE),
        "no image bytes in the history stream"
    );
    let distinct = (red.len() + blue.len()) as u64;
    let inline: u64 = originals.iter().map(|b| b.len() as u64).sum();
    let (history_bytes, image_bytes) = (bytes(&history), bytes(&images));
    eprintln!(
        "AC-1.51e: history packets {history_bytes} B, image packets {image_bytes} B \
         (distinct raw images {distinct} B); inline bodies {inline} B; \
         total {} B vs inline {inline} B",
        history_bytes + image_bytes
    );
    assert!(
        image_bytes >= distinct && image_bytes <= distinct + distinct / 10 + 256,
        "each distinct image once, about its raw size: {image_bytes} for {distinct}"
    );
    assert!(history_bytes + image_bytes < inline, "cheaper than inline");

    // AC-1.51g: syncing the same range again transfers no image bytes.
    let mark = last_packet(&a);
    b.history_sync_request(only_server(&b), lobby, None)
        .expect("load earlier");
    let again = wait_sync_restored(&mut b_rx, lobby).await;
    assert!(again.first.is_empty(), "nothing new");
    assert_eq!(bytes(&sent(&a, mark, IMAGE)), 0, "no image bytes again");

    // AC-1.51g: a new message with a picture B already holds is restored without a fetch.
    b.join_channel(only_server(&b), 0)
        .expect("B leaves the room");
    tokio::time::sleep(Duration::from_millis(500)).await;
    bot.send_channel(Some(&room), &image_message("red while away", &red))
        .expect("send");
    wait_message(&mut a_rx, "red while away").await;
    // A answers a person's requests at a limited rate; the syncs so far used up the burst.
    tokio::time::sleep(Duration::from_secs(15)).await;
    let mark = last_packet(&a);
    b.join_channel(only_server(&b), lobby)
        .expect("B rejoins the room");
    let away = wait_sync_restored(&mut b_rx, lobby).await;
    assert_eq!(away.first.len(), 1, "only the new message");
    assert!(
        away.first[0].1.contains(REFERENCE),
        "it came as a reference"
    );
    let a_now = stored(&a, lobby);
    assert_eq!(
        away.latest[&away.first[0].0],
        a_now[a_now.len() - 1],
        "restored from B's own copy"
    );
    let refetched = bytes(&sent(&a, mark, IMAGE));
    eprintln!(
        "AC-1.51g: image bytes for the repeated sync 0 B, for a held image {refetched} B; \
         history packets {} B",
        bytes(&sent(&a, mark, HISTORY))
    );
    assert_eq!(refetched, 0, "a held image is not fetched again");

    a.set_packet_capture(false);
    a.disconnect(only_server(&a));
    b.disconnect(only_server(&b));
}

/// FR-1.60f with FR-2.36: B has never answered the direct connection question. Fetching the
/// synced images is not something B started, so it must not ask (the UI would show a dialog
/// out of nowhere); without a relay the images come over plugin data right away.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v15 (MUMCORD_LIVE=1)"]
async fn a_background_fetch_never_asks_for_direct_connections() {
    if !live() {
        return;
    }
    audio_off();
    let dir = work_dir("engine-live-history-images-consent");
    let (a_name, b_name, bot_name) = (unique("mc-hca"), unique("mc-hcb"), unique("mc-hcbot"));
    let (a, mut a_rx) = start_engine(&dir.join("a"), &a_name).await;
    let (b, mut b_rx) = start_engine_with(&dir.join("b"), &b_name, Consent::Unset).await;
    let (_admin, bot, room) = temp_room(&dir, &bot_name).await;

    let a_server = add_server(&a, &a_name).await;
    let snapshot = connect(&a, &mut a_rx, &a_server).await;
    let lobby = channel(&snapshot, &room);
    a.join_channel(only_server(&a), lobby)
        .expect("A joins the room");
    bot.wait_user(&a_name, &in_channel(&room))
        .await
        .expect("A in the room");
    bot.send_channel(Some(&room), &image_message("a picture", &picture(31)))
        .expect("send");
    wait_message(&mut a_rx, "a picture").await;
    tokio::time::sleep(Duration::from_millis(1100)).await;

    let b_server = add_server(&b, &b_name).await;
    connect(&b, &mut b_rx, &b_server).await;
    b.join_channel(only_server(&b), lobby)
        .expect("B joins the room");
    // Fails on a consent question; restored well within the minute an unanswered one would hold.
    let synced = wait_sync_restored(&mut b_rx, lobby).await;
    assert_eq!(synced.added, 1);
    assert_eq!(stored(&b, lobby), stored(&a, lobby), "B has A's exact body");
    assert_eq!(
        b.settings().expect("settings").allow_direct_connections,
        Consent::Unset,
        "the question is still open for the user's first own action"
    );

    a.disconnect(only_server(&a));
    b.disconnect(only_server(&b));
}

/// AC-1.51h: a locked channel's images go only to people inside it; a private chat's image
/// to nobody. Only the plugin-data path runs here (see the file comment).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v15 (MUMCORD_LIVE=1)"]
async fn locked_channel_images_go_only_to_people_inside() {
    if !live() {
        return;
    }
    audio_off();
    let dir = work_dir("engine-live-history-images-locked");
    let (a_name, b_name, c_name) = (unique("mc-hla"), unique("mc-hlb"), unique("mc-hlc"));
    let (a, mut a_rx) = start_engine(&dir.join("a"), &a_name).await;
    let (b, mut b_rx) = start_engine(&dir.join("b"), &b_name).await;
    let (c, mut c_rx) = start_engine(&dir.join("c"), &c_name).await;
    let (admin, bot, room) = temp_room(&dir, &unique("mc-hlbot")).await;
    admin.require_token(&room, "pw402").await.expect("ACL");
    let (locked_png, private_png) = (picture(31), picture(47));

    // A is inside with the password while the admin posts a picture; the bot sends A one
    // privately.
    let a_server = add_server(&a, &a_name).await;
    let snapshot = connect(&a, &mut a_rx, &a_server).await;
    let locked = channel(&snapshot, &room);
    a.channel_join_with_password(only_server(&a), locked, "pw402".into())
        .await
        .expect("A joins with the password");
    admin
        .wait_user(&a_name, &in_channel(&room))
        .await
        .expect("A in the locked room");
    admin
        .send_channel(Some(&room), &image_message("locked red", &locked_png))
        .expect("send");
    wait_message(&mut a_rx, "locked red").await;
    bot.send_private(&a_name, &image_message("private blue", &private_png))
        .expect("pm");
    wait_message(&mut a_rx, "private blue").await;
    let original = stored(&a, locked);
    tokio::time::sleep(Duration::from_millis(1100)).await;
    a.set_packet_capture(true);
    a.clear_packet_capture();

    // C, outside, asks A for both hashes over plugin data: refused, no image bytes.
    let c_server = add_server(&c, &c_name).await;
    let c_snapshot = connect(&c, &mut c_rx, &c_server).await;
    let c_id = only_server(&c);
    let a_for_c = session_of(&c_snapshot, &a_name);
    for png in [&locked_png, &private_png] {
        assert_eq!(
            c.full_image_fetch(c_id, &reference(png), Some(a_for_c), None)
                .await,
            None,
            "refused to C"
        );
    }

    // B enters with the password, syncs and gets the locked image.
    let b_server = add_server(&b, &b_name).await;
    let b_snapshot = connect(&b, &mut b_rx, &b_server).await;
    let b_id = only_server(&b);
    b.channel_join_with_password(b_id, locked, "pw402".into())
        .await
        .expect("B joins with the password");
    let synced = wait_sync_restored(&mut b_rx, locked).await;
    assert_eq!(synced.first.len(), 1);
    assert!(synced.first[0].1.contains(REFERENCE));
    assert_eq!(stored(&b, locked), original, "B got the image");

    // Even B, inside, gets nothing for the private hash; C still nothing for the locked one.
    let a_for_b = session_of(&b_snapshot, &a_name);
    assert_eq!(
        b.full_image_fetch(b_id, &reference(&private_png), Some(a_for_b), None)
            .await,
        None,
        "a private chat's image is refused to B"
    );
    assert_eq!(
        c.full_image_fetch(c_id, &reference(&locked_png), Some(a_for_c), None)
            .await,
        None,
        "still refused to C"
    );

    let images = sent(&a, None, IMAGE);
    let c_session = c_snapshot.self_session;
    let b_session = b_snapshot.self_session;
    let to_c = image_kinds_to(&images, c_session);
    // Kinds 3 (Start) and 4 (Chunk) carry image bytes; 2 (Queued) and 5 (Unavailable) do not.
    assert!(
        !to_c.is_empty() && to_c.contains(&5) && to_c.iter().all(|&k| k == 2 || k == 5),
        "C only got refusals: {to_c:?}"
    );
    let to_b = image_kinds_to(&images, b_session);
    let b_bytes: u64 = images
        .iter()
        .filter(|p| p.receivers.contains(&b_session))
        .map(|p| u64::from(p.size))
        .sum();
    assert!(to_b.contains(&5), "B's private-hash request was refused");
    assert!(
        b_bytes >= locked_png.len() as u64,
        "B got the locked image's bytes"
    );
    eprintln!(
        "AC-1.51h: A sent C {} refusals and no image bytes; B {b_bytes} B (image {} B)",
        to_c.len(),
        locked_png.len()
    );

    a.set_packet_capture(false);
    a.disconnect(only_server(&a));
    b.disconnect(b_id);
    c.disconnect(c_id);
}

/// AC-1.51f: a 0.6.2 client (`MUMCORD_OLD_PEER`, built by `scripts/build-old-peer.sh`) syncs
/// from a 0.7.0 holder and gets complete inline bodies, never a reference.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v15 (MUMCORD_LIVE=1) and MUMCORD_OLD_PEER"]
async fn an_old_client_keeps_getting_inline_images() {
    if !live() {
        return;
    }
    let Some(old_peer) = std::env::var_os("MUMCORD_OLD_PEER") else {
        eprintln!("skipped: set MUMCORD_OLD_PEER to the binary of scripts/build-old-peer.sh");
        return;
    };
    audio_off();
    let dir = work_dir("engine-live-history-images-old");
    let (a_name, old_name, bot_name) = (unique("mc-hoa"), unique("mc-hoold"), unique("mc-hobot"));
    let (a, mut a_rx) = start_engine(&dir.join("a"), &a_name).await;
    let (_admin, bot, room) = temp_room(&dir, &bot_name).await;
    let (red, blue) = (picture(53), picture(59));

    let a_server = add_server(&a, &a_name).await;
    let snapshot = connect(&a, &mut a_rx, &a_server).await;
    let lobby = channel(&snapshot, &room);
    a.join_channel(only_server(&a), lobby)
        .expect("A joins the room");
    bot.wait_user(&a_name, &in_channel(&room))
        .await
        .expect("A in the room");
    for (text, png) in [
        ("old red", &red),
        ("old blue", &blue),
        ("old red again", &red),
    ] {
        bot.send_channel(Some(&room), &image_message(text, png))
            .expect("send");
        wait_message(&mut a_rx, text).await;
    }
    let originals = stored(&a, lobby);
    tokio::time::sleep(Duration::from_millis(1100)).await;
    a.set_packet_capture(true);
    a.clear_packet_capture();

    let output = tokio::time::timeout(
        Duration::from_secs(90),
        tokio::process::Command::new(old_peer)
            .args([
                old_name.as_str(),
                room.as_str(),
                dir.join("old").to_str().expect("utf-8 path"),
            ])
            .kill_on_drop(true)
            .output(),
    )
    .await
    .expect("old peer finishes")
    .expect("old peer runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "old peer failed: {}",
        String::from_utf8_lossy(&output.stderr)
            .lines()
            .rev()
            .take(20)
            .collect::<Vec<_>>()
            .join("\n")
    );
    let lines: Vec<serde_json::Value> = stdout
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let bodies: Vec<&str> = lines.iter().filter_map(|l| l["body"].as_str()).collect();
    assert_eq!(
        lines.last().and_then(|l| l["done"].as_u64()),
        Some(3),
        "the old peer's sync ended with three messages"
    );
    assert_eq!(bodies, originals, "complete messages with their images");

    let history = sent(&a, None, HISTORY);
    let stream = part_stream(&history);
    assert!(
        stream.contains(";base64,") && !stream.contains(REFERENCE),
        "no hash references to the old client"
    );
    assert!(
        sent(&a, None, IMAGE).is_empty(),
        "no separate image transfer"
    );
    eprintln!(
        "AC-1.51f: 0.6.2 peer got {} inline bodies; history packets {} B",
        bodies.len(),
        bytes(&history)
    );

    a.set_packet_capture(false);
    a.disconnect(only_server(&a));
}
