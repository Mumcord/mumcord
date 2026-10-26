//! Watch party (FR-2.39 – FR-2.41, AC-2.28) against the local test server: two engines in the
//! same channel start and join a party by click, play/pause/seek reach the other side with who
//! did it, a latecomer gets the current position, and leaving the channel leaves the party.
//! The playlist (FR-2.69, AC-2.50): entries keep one order on every client, reorder and remove
//! reach the others, a latecomer gets the list, and skip and the end of a video advance once.
//! Party announce (FR-2.75, AC-2.56): a Mumcord user entering the channel gets a running party
//! and share within 2 s, from the host (later from another participant), exactly once.
//! `testenv/up.sh v15`, then:
//! `MUMCORD_LIVE=1 cargo test -p mumcord-engine --features test-backend --test live_watch_party -- --ignored --test-threads=1`
#![cfg(feature = "test-backend")]

mod common;

use std::path::{Path, PathBuf};
use std::time::Duration;

use mumcord_bot::env;
use mumcord_engine::screen_share::ScreenShareView;
use mumcord_engine::{
    Engine, EngineEvent, EventReceiver, PasswordChange, SecretStore, ServerFields, Snapshot,
    WatchCommand, WatchPartyView,
};

const WAIT: Duration = Duration::from_secs(20);
const VIDEO: &str = "dQw4w9WgXcQ";
/// How fast a newcomer must learn a running party or share (AC-2.56).
const ANNOUNCE: Duration = Duration::from_secs(2);

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

/// Waits for a party change that satisfies `pick`.
async fn wait_party(
    rx: &mut EventReceiver,
    what: &str,
    pick: impl Fn(Option<&WatchPartyView>) -> bool,
) -> Option<WatchPartyView> {
    wait_for(rx, what, |e| match e {
        EngineEvent::WatchPartyChanged(party) if pick(party.as_ref()) => Some(party.clone()),
        _ => None,
    })
    .await
}

struct Client {
    engine: Engine,
    rx: EventReceiver,
    name: String,
    snapshot: Snapshot,
}

impl Client {
    fn server(&self) -> mumcord_engine::ServerId {
        self.engine
            .connections()
            .first()
            .and_then(mumcord_engine::ConnState::server_id)
            .unwrap_or(0)
    }

    fn session(&self) -> u32 {
        self.snapshot.self_session
    }
}

async fn start(dir: &Path, who: &str) -> Client {
    let (engine, mut rx) = Engine::new(
        &dir.join(who),
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
    )
    .expect("engine");
    engine
        .identity_create(format!("{who} Tester"))
        .await
        .expect("identity");
    let name = unique(who);
    let server = engine
        .server_add(ServerFields {
            label: format!("test {} {name}", common::v15()),
            address: "127.0.0.1".into(),
            port: common::v15(),
            username: name.clone(),
            password: match env::testenv_var("SERVER_PASSWORD") {
                Some(p) => PasswordChange::Set(p),
                None => PasswordChange::Keep,
            },
        })
        .await
        .expect("server");
    engine.connect(server.id, None).expect("connect");
    let snapshot = loop {
        let event = wait_for(&mut rx, "sync", |e| match e {
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
            EngineEvent::ServerSynced(s) => break *s,
            _ => unreachable!(),
        }
    };
    Client {
        engine,
        rx,
        name,
        snapshot,
    }
}

/// Waits until `name` is known as a Mumcord user with watch-party support.
async fn wait_mumcord(rx: &mut EventReceiver, name: &str) {
    wait_for(rx, "a Mumcord user", |e| match e {
        EngineEvent::UserUpserted(u)
            if u.name == name
                && u.mumcord_features
                    .as_ref()
                    .is_some_and(|f| f.iter().any(|x| x == "watch-party")) =>
        {
            Some(())
        }
        _ => None,
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v15 (MUMCORD_LIVE=1)"]
async fn watch_party_syncs_between_mumcord_clients() {
    if !live() {
        return;
    }
    // SAFETY: set before any audio thread exists; tests run with --test-threads=1.
    unsafe { std::env::set_var("MUMCORD_AUDIO_BACKEND", "null") };
    let dir = work_dir("live_watch_party");
    let mut a = start(&dir, "b24a").await;
    let mut b = start(&dir, "b24b").await;
    wait_mumcord(&mut a.rx, &b.name).await;
    wait_mumcord(&mut b.rx, &a.name).await;

    // A starts; B is only invited until it clicks "Join".
    let started = a
        .engine
        .watch_party_start(a.server(), VIDEO)
        .expect("start");
    assert!(
        started.joined && !started.playing,
        "a new party starts paused"
    );
    assert_eq!(started.participants, [a.session()]);
    let invite = wait_party(&mut b.rx, "the invitation", |p| p.is_some())
        .await
        .expect("party");
    assert_eq!(invite.video_id, VIDEO);
    assert!(!invite.joined, "nothing starts by itself");
    assert!(
        a.engine.watch_party_start(a.server(), VIDEO).is_err(),
        "one party per channel"
    );

    let joined = b.engine.watch_party_join(b.server()).expect("join");
    assert!(joined.joined);
    let a_session = a.session();
    let b_session = b.session();
    wait_party(&mut a.rx, "B in the party", |p| {
        p.is_some_and(|p| p.participants.contains(&b_session))
    })
    .await;

    // Pause on A arrives at B within a second, with who did it.
    let sent = std::time::Instant::now();
    a.engine
        .watch_party_command(a.server(), WatchCommand::Pause { position_ms: 5000 })
        .expect("pause");
    // The party starts paused, so the pause shows by its position and author.
    let paused = wait_party(&mut b.rx, "the pause", |p| {
        p.is_some_and(|p| !p.playing && p.last_action.is_some())
    })
    .await
    .expect("party");
    assert!(
        sent.elapsed() < Duration::from_secs(1),
        "{:?}",
        sent.elapsed()
    );
    assert_eq!(paused.position_ms, 5000);
    let last = paused.last_action.expect("who paused");
    assert_eq!(last.session, a_session);

    // Seek and play on B arrive at A.
    b.engine
        .watch_party_command(
            b.server(),
            WatchCommand::Seek {
                position_ms: 60_000,
            },
        )
        .expect("seek");
    let sought = wait_party(&mut a.rx, "the seek", |p| {
        p.is_some_and(|p| p.position_ms == 60_000)
    })
    .await
    .expect("party");
    assert_eq!(sought.last_action.map(|l| l.session), Some(b_session));
    b.engine
        .watch_party_command(
            b.server(),
            WatchCommand::Play {
                position_ms: 60_000,
            },
        )
        .expect("play");
    wait_party(&mut a.rx, "the play", |p| p.is_some_and(|p| p.playing)).await;

    // A latecomer gets the party at the current position, invited only.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let mut c = start(&dir, "b24c").await;
    let late = wait_party(&mut c.rx, "the party for the latecomer", |p| p.is_some())
        .await
        .expect("party");
    assert!(!late.joined);
    assert!(late.playing);
    assert!(late.position_ms >= 61_000, "{} ms", late.position_ms);
    let mut participants = late.participants.clone();
    participants.sort_unstable();
    let mut expected = vec![a_session, b_session];
    expected.sort_unstable();
    assert_eq!(participants, expected);
    let c_joined = c.engine.watch_party_join(c.server()).expect("C joins");
    assert!(c_joined.position_ms >= 61_000);

    // B leaves the channel and with it the party.
    let afk = b
        .snapshot
        .channels
        .iter()
        .find(|ch| ch.name == "AFK")
        .expect("AFK")
        .id;
    b.engine.join_channel(b.server(), afk).expect("move B");
    wait_party(&mut b.rx, "B's party ending", |p| p.is_none()).await;
    wait_party(&mut a.rx, "B leaving the party", |p| {
        p.is_some_and(|p| !p.participants.contains(&b_session))
    })
    .await;
    assert!(b.engine.watch_party(b.server()).is_none());

    // Leaving by button keeps the party for the others.
    c.engine.watch_party_leave(c.server()).expect("C leaves");
    let remaining = wait_party(&mut a.rx, "C leaving", |p| {
        p.is_some_and(|p| p.participants == [a_session])
    })
    .await;
    assert!(remaining.is_some());
}

/// The video ids of a party's playlist, in order.
fn order(party: &WatchPartyView) -> Vec<String> {
    party.playlist.iter().map(|e| e.video_id.clone()).collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v15 (MUMCORD_LIVE=1)"]
async fn watch_party_playlist_syncs_between_mumcord_clients() {
    if !live() {
        return;
    }
    // SAFETY: set before any audio thread exists; tests run with --test-threads=1.
    unsafe { std::env::set_var("MUMCORD_AUDIO_BACKEND", "null") };
    const SECOND: &str = "a-b_c-d_e-f";
    const THIRD: &str = "0123456789a";
    const FOURTH: &str = "ZZZZZZZZZZZ";
    let dir = work_dir("live_watch_playlist");
    let mut a = start(&dir, "b51a").await;
    let mut b = start(&dir, "b51b").await;
    wait_mumcord(&mut a.rx, &b.name).await;
    wait_mumcord(&mut b.rx, &a.name).await;
    a.engine
        .watch_party_start(a.server(), VIDEO)
        .expect("start");
    wait_party(&mut b.rx, "the invitation", |p| p.is_some()).await;
    b.engine.watch_party_join(b.server()).expect("join");
    let b_session = b.session();
    wait_party(&mut a.rx, "B in the party", |p| {
        p.is_some_and(|p| p.participants.contains(&b_session))
    })
    .await;

    // Two adds right after each other leave bundled and arrive in the same order.
    a.engine.watch_party_add(a.server(), SECOND).expect("add");
    a.engine.watch_party_add(a.server(), THIRD).expect("add");
    let on_b = wait_party(&mut b.rx, "two entries", |p| {
        p.is_some_and(|p| p.playlist.len() == 2)
    })
    .await
    .expect("party");
    assert_eq!(order(&on_b), [SECOND, THIRD]);
    assert_eq!(on_b.playlist[0].added_by, a.session());
    assert_eq!(
        on_b.playlist[0].added_by_name.as_deref(),
        Some(a.name.as_str())
    );

    // B reorders and removes; A shows the same.
    let third = on_b.playlist[1].id;
    b.engine
        .watch_party_move(b.server(), third, 0)
        .expect("move");
    wait_party(&mut a.rx, "the new order", |p| {
        p.is_some_and(|p| order(p) == [THIRD, SECOND])
    })
    .await;
    b.engine
        .watch_party_remove(b.server(), third)
        .expect("remove");
    b.engine.watch_party_add(b.server(), FOURTH).expect("add");
    let on_a = wait_party(&mut a.rx, "remove and add", |p| {
        p.is_some_and(|p| order(p) == [SECOND, FOURTH])
    })
    .await
    .expect("party");
    assert_eq!(on_a.playlist[1].added_by, b_session);

    // A latecomer gets the list and the position.
    let mut c = start(&dir, "b51c").await;
    let late = wait_party(&mut c.rx, "the playlist for the latecomer", |p| {
        p.is_some_and(|p| p.playlist.len() == 2)
    })
    .await
    .expect("party");
    assert_eq!(order(&late), [SECOND, FOURTH]);
    assert_eq!(late.current_entry, 0);
    c.engine.watch_party_join(c.server()).expect("C joins");

    // Skip on A: everyone goes to the next entry and sees who skipped.
    let a_session = a.session();
    let second = on_a.playlist[0].id;
    a.engine
        .watch_party_next(a.server(), 0, true)
        .expect("skip");
    for (rx, who) in [(&mut b.rx, "B"), (&mut c.rx, "C")] {
        let skipped = wait_party(rx, who, |p| p.is_some_and(|p| p.current_entry == second))
            .await
            .expect("party");
        assert_eq!(skipped.video_id, SECOND);
        assert_eq!(skipped.skipped_by, Some(a_session));
        assert_eq!(order(&skipped), [FOURTH]);
    }

    // The video ends on two clients at once: the party advances only once.
    b.engine
        .watch_party_next(b.server(), second, false)
        .expect("next on B");
    c.engine
        .watch_party_next(c.server(), second, false)
        .expect("next on C");
    let ended = wait_party(&mut a.rx, "the next video", |p| {
        p.is_some_and(|p| p.video_id == FOURTH)
    })
    .await
    .expect("party");
    assert!(ended.playlist.is_empty());
    assert_eq!(ended.skipped_by, None);
    tokio::time::sleep(Duration::from_millis(500)).await;
    let now = a.engine.watch_party(a.server()).expect("party");
    assert_eq!(now.current_entry, ended.current_entry);
}

/// What a newcomer learned within [`ANNOUNCE`] and right after: every party change, and the
/// share of `sharer` if it arrived.
async fn announced(
    rx: &mut EventReceiver,
    sharer: Option<u32>,
) -> (Vec<Option<WatchPartyView>>, Option<ScreenShareView>) {
    let deadline = tokio::time::Instant::now() + ANNOUNCE;
    let mut parties = Vec::new();
    let mut share = None;
    while let Ok(Some(event)) = tokio::time::timeout_at(deadline, rx.recv()).await {
        match event {
            EngineEvent::WatchPartyChanged(party) => parties.push(party),
            EngineEvent::ScreenSharesChanged(shares) => {
                share = shares.into_iter().find(|s| Some(s.session) == sharer);
            }
            _ => {}
        }
    }
    (parties, share)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v15 (MUMCORD_LIVE=1)"]
async fn newcomers_in_the_channel_get_the_party_once() {
    if !live() {
        return;
    }
    // SAFETY: set before any audio thread exists; tests run with --test-threads=1.
    unsafe { std::env::set_var("MUMCORD_AUDIO_BACKEND", "null") };
    let dir = work_dir("live_watch_party_announce");
    let mut a = start(&dir, "b60a").await;
    let mut b = start(&dir, "b60b").await;
    let mut c = start(&dir, "b60c").await;
    let mut d = start(&dir, "b60d").await;
    for (rx, name) in [
        (&mut a.rx, &d.name),
        (&mut b.rx, &d.name),
        (&mut c.rx, &a.name),
        (&mut d.rx, &a.name),
    ] {
        wait_mumcord(rx, name).await;
    }
    let (a_session, b_session) = (a.session(), b.session());
    let home = a
        .snapshot
        .users
        .iter()
        .find(|u| u.session == a_session)
        .expect("A")
        .channel_id;
    let afk = a
        .snapshot
        .channels
        .iter()
        .find(|ch| ch.name == "AFK")
        .expect("AFK")
        .id;
    for x in [&c, &d] {
        x.engine.join_channel(x.server(), afk).expect("move to AFK");
    }
    tokio::time::sleep(Duration::from_millis(500)).await;

    // A hosts a party, B joins it and shares the screen.
    a.engine
        .watch_party_start(a.server(), VIDEO)
        .expect("start");
    wait_party(&mut b.rx, "the invite", |p| p.is_some()).await;
    b.engine.watch_party_join(b.server()).expect("B joins");
    wait_party(&mut a.rx, "B in the party", |p| {
        p.is_some_and(|p| p.participants.contains(&b_session))
    })
    .await;
    b.engine
        .screen_share_start(b.server(), false)
        .await
        .expect("share");
    tokio::time::sleep(Duration::from_millis(500)).await;
    while c.rx.try_recv().is_ok() {}

    // C comes into the channel: one announcement, the share too.
    c.engine.join_channel(c.server(), home).expect("C moves");
    let (parties, share) = announced(&mut c.rx, Some(b_session)).await;
    let got: Vec<_> = parties.iter().flatten().collect();
    assert_eq!(got.len(), 1, "one announcement for C: {parties:?}");
    assert!(!got[0].joined && got[0].video_id == VIDEO);
    assert!(share.is_some(), "B's share announced to C");

    // A leaves the party, B stays: D still gets it, from B.
    a.engine.watch_party_leave(a.server()).expect("A leaves");
    wait_party(&mut b.rx, "A leaving", |p| {
        p.is_some_and(|p| p.participants == [b_session])
    })
    .await;
    while d.rx.try_recv().is_ok() {}
    d.engine.join_channel(d.server(), home).expect("D moves");
    let (parties, _) = announced(&mut d.rx, Some(b_session)).await;
    let got: Vec<_> = parties.iter().flatten().collect();
    assert_eq!(got.len(), 1, "one announcement for D: {parties:?}");
    assert_eq!(got[0].participants, [b_session]);

    b.engine.screen_share_stop(b.server()).expect("stop");
}
