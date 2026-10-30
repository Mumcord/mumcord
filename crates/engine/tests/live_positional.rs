//! Positional audio against the local 1.5 test server (FR-1.41, FR-1.72, AC-1.31, AC-1.55).
//! `testenv/up.sh v15`, then:
//! `MUMCORD_LIVE=1 cargo test -p mumcord-engine --features test-backend --test live_positional -- --ignored --nocapture --test-threads=1`
#![cfg(feature = "test-backend")]

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use mumbot::voice::{BotVoice, Clip, PlayOptions};
use mumbot::{Bot, BotConfig, env};
use mumcord_audio::mixer::{Mixer, VoicePacket as MixerPacket};
use mumcord_audio::spatial::Listener;
use mumcord_audio::{FRAME_SAMPLES, Frame};
use mumcord_engine::EventReceiver;
use mumcord_engine::positional_sources::{Fetch, Pose, PositionSource, Positional};
use mumcord_engine::{EngineEvent, PasswordChange, SecretStore, ServerFields, Snapshot};
use mumcord_protocol::voice::VoicePacket;
use tokio::sync::broadcast;

const WAIT: Duration = Duration::from_secs(20);
const SAMPLE_RATE: usize = 48_000;
const CHANNEL: &str = "Raid";

fn live() -> bool {
    let on = std::env::var("MUMCORD_LIVE").is_ok_and(|v| v == "1");
    if !on {
        eprintln!("skipped: set MUMCORD_LIVE=1 and start testenv/up.sh v15");
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

fn speech() -> Clip {
    Clip::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testenv/audio/speech.wav"))
        .expect("speech clip")
}

async fn bot_in_channel(dir: &Path, name: &str, context: &[u8]) -> (Bot, BotVoice) {
    let bot = Bot::connect(BotConfig {
        data_dir: Some(dir.join(name)),
        ..BotConfig::new(name, common::v15())
    })
    .await
    .expect("connect");
    bot.join(CHANNEL).await.expect("join");
    bot.handle()
        .set_plugin_context(context.to_vec(), name.to_owned())
        .expect("plugin context");
    let voice = BotVoice::start(&bot).await.expect("voice");
    (bot, voice)
}

fn talk(position: [f32; 3]) -> PlayOptions {
    PlayOptions {
        max_duration: Some(Duration::from_secs(2)),
        position: Some(position),
        ..PlayOptions::default()
    }
}

/// Packets of `session` arriving within `window`.
async fn packets_of(
    rx: &mut broadcast::Receiver<VoicePacket>,
    session: u32,
    window: Duration,
) -> Vec<VoicePacket> {
    let deadline = tokio::time::Instant::now() + window;
    let mut out = Vec::new();
    while let Ok(received) = tokio::time::timeout_at(deadline, rx.recv()).await {
        match received {
            Ok(p) if p.session == session => out.push(p),
            Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
    out
}

/// Plays `packets` through the real mixer with a listener at the origin looking along +Z and
/// returns the RMS of the left and right channel in the steady part.
fn render(packets: &[VoicePacket]) -> (f32, f32) {
    let listener = Arc::new(Listener::default());
    listener.set_pose([0.0; 3], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]);
    let mut mixer = Mixer::new().expect("mixer");
    mixer.set_listener(listener);
    let first = packets.first().expect("packets").seq;
    let arrival = |p: &VoicePacket| (p.seq - first) * 10 + 1;
    let end = packets.last().map_or(0, arrival) + 200;
    let (mut left, mut right) = (Vec::new(), Vec::new());
    let (mut l, mut r): (Frame, Frame) = ([0.0; FRAME_SAMPLES], [0.0; FRAME_SAMPLES]);
    let mut now = 0;
    let mut next = 0;
    while now < end {
        while next < packets.len() && arrival(&packets[next]) <= now {
            let p = &packets[next];
            let packet = MixerPacket::new(
                p.session,
                p.seq,
                &p.payload,
                p.is_last,
                false,
                Instant::now(),
            )
            .expect("payload fits")
            .with_position(p.positional);
            mixer.push_packet(&packet, arrival(p));
            next += 1;
        }
        mixer.mix(&mut l, &mut r, None, now, &mut |_| {});
        left.extend_from_slice(&l);
        right.extend_from_slice(&r);
        now += 10;
    }
    let rms = |s: &[f32]| (s.iter().map(|x| x * x).sum::<f32>() / s.len().max(1) as f32).sqrt();
    let steady = 300 * SAMPLE_RATE / 1000..left.len().saturating_sub(300 * SAMPLE_RATE / 1000);
    (rms(&left[steady.clone()]), rms(&right[steady]))
}

/// AC-1.31 (automated channel-level part): two people in the same game, one left and one
/// further away on the right of the listener, are heard from the matching side and the far
/// one quieter; a person in another game is not placed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v15 (MUMCORD_LIVE=1)"]
async fn voices_are_heard_from_their_side_and_quieter_with_distance() {
    if !live() {
        return;
    }
    let dir = work_dir("engine-live-positional-channels");
    let game = format!("Link\0{}", unique("game")).into_bytes();
    let (_l, listener) = bot_in_channel(&dir, &unique("pa-ear"), &game).await;
    let (left_bot, left) = bot_in_channel(&dir, &unique("pa-left"), &game).await;
    let (right_bot, right) = bot_in_channel(&dir, &unique("pa-right"), &game).await;
    let (other_bot, other) = bot_in_channel(&dir, &unique("pa-other"), b"Link\0elsewhere").await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    let clip = speech();

    let mut rx = listener.subscribe_packets();
    let session = |bot: &Bot| bot.own_user().expect("own user").session;
    let mut heard = BTreeMap::new();
    for (bot, voice, position) in [
        (&left_bot, &left, [-3.0, 0.0, 0.0]),
        (&right_bot, &right, [6.0, 0.0, 0.0]),
        (&other_bot, &other, [-3.0, 0.0, 0.0]),
    ] {
        let s = session(bot);
        let (played, packets) = tokio::join!(
            voice.play(&clip, talk(position)),
            packets_of(&mut rx, s, Duration::from_secs(3))
        );
        played.expect("play");
        assert!(packets.len() > 50, "listener got {} packets", packets.len());
        heard.insert(s, packets);
    }

    let left_packets = &heard[&session(&left_bot)];
    assert!(
        left_packets
            .iter()
            .all(|p| p.positional == Some([-3.0, 0.0, 0.0]))
    );
    let (ll, lr) = render(left_packets);
    let (rl, rr) = render(&heard[&session(&right_bot)]);
    eprintln!("left voice L {ll:.4} R {lr:.4}; right voice L {rl:.4} R {rr:.4}");
    assert!(ll > 2.0 * lr, "left voice from the left");
    assert!(rr > 2.0 * rl, "right voice from the right");
    assert!(ll + lr > 1.5 * (rl + rr), "the farther voice is quieter");

    let other_packets = &heard[&session(&other_bot)];
    assert!(
        other_packets.iter().all(|p| p.positional.is_none()),
        "the server strips positions of another game"
    );
    let (ol, or) = render(other_packets);
    assert!((ol - or).abs() < 1e-4, "another game plays unplaced");
}

/// A fake game for the engine.
struct FakeGame {
    active: Arc<AtomicBool>,
    context: Vec<u8>,
}

impl PositionSource for FakeGame {
    fn fetch(&mut self, _now: Instant) -> Option<Fetch> {
        self.active.load(Ordering::Relaxed).then(|| Fetch {
            source: "Fake game source".into(),
            game: Some("Fake Game".into()),
            data: Positional {
                avatar: Pose {
                    position: [2.0, 0.0, 0.0],
                    front: [0.0, 0.0, 1.0],
                    top: [0.0, 1.0, 0.0],
                },
                camera: Pose {
                    position: [0.0; 3],
                    front: [0.0, 0.0, 1.0],
                    top: [0.0, 1.0, 0.0],
                },
                context: self.context.clone(),
                identity: "me".into(),
            },
        })
    }
}

/// Waits for a "sending positions" change of `session`; fails on a change of any session in
/// `never`.
async fn sending_change(
    rx: &mut EventReceiver,
    session: u32,
    never: &[u32],
    within: Duration,
) -> Option<bool> {
    let deadline = tokio::time::Instant::now() + within;
    while let Ok(Some(event)) = tokio::time::timeout_at(deadline, rx.recv()).await {
        if let EngineEvent::PositionalSendingChanged {
            session: s,
            sending,
        } = event
        {
            eprintln!("3D: session {s} sending {sending}");
            assert!(!never.contains(&s), "unexpected change for {s}");
            if s == session {
                return Some(sending);
            }
        }
    }
    None
}

/// FR-1.72 / AC-1.55 (engine part): the own entry is flagged while a game delivers positions,
/// positions go only to people in the same game, and others are flagged while their voice
/// carries positions.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v15 (MUMCORD_LIVE=1)"]
async fn engine_sends_positions_to_its_game_and_flags_senders() {
    if !live() {
        return;
    }
    let dir = work_dir("engine-live-positional");
    let name = unique("pa-me");
    let game = format!("Link\0{}", unique("game")).into_bytes();
    let clip = speech();

    // Microphone: 30 s silence (the other checks run first), 4 s speech, then silence.
    let mut mic = vec![0.0; SAMPLE_RATE * 30];
    mic.extend(clip.samples.iter().take(SAMPLE_RATE * 4));
    mic.extend(std::iter::repeat_n(0.0, SAMPLE_RATE * 60));
    let (input, output) = (dir.join("in.wav"), dir.join("out.wav"));
    Clip::from_samples(mic).save(&input).expect("save input");
    // SAFETY: set before any audio thread exists; no other thread reads the environment yet.
    unsafe {
        std::env::set_var(
            "MUMCORD_AUDIO_BACKEND",
            format!("file:{}:{}", input.display(), output.display()),
        );
    }

    let (peer_bot, peer) = bot_in_channel(&dir, &unique("pa-peer"), &game).await;
    let (stranger_bot, stranger) =
        bot_in_channel(&dir, &unique("pa-stranger"), b"Link\0elsewhere").await;

    let (engine, mut rx) = mumcord_engine::Engine::new(
        &dir.join("data"),
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
    )
    .expect("engine");
    let active = Arc::new(AtomicBool::new(true));
    engine.set_position_sources(vec![Box::new(FakeGame {
        active: active.clone(),
        context: game.clone(),
    })]);
    let mut settings = engine.settings().expect("settings");
    settings.positional.enabled = true;
    engine.update_settings(settings).await.expect("enable");
    engine
        .identity_create(name.clone(), None)
        .await
        .expect("identity");
    let server = engine
        .server_add(ServerFields {
            label: "v15".into(),
            address: env::HOST.into(),
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
    let deadline = tokio::time::Instant::now() + WAIT;
    let snapshot: Snapshot = loop {
        let event = tokio::time::timeout_at(deadline, rx.recv())
            .await
            .expect("timed out waiting for sync")
            .expect("event stream open");
        match event {
            EngineEvent::CertTrustRequired { fingerprint, .. } => engine
                .cert_trust_decide(server.id, &fingerprint, true)
                .expect("trust"),
            EngineEvent::ServerSynced(s) => break *s,
            _ => {}
        }
    };
    let channel = snapshot
        .channels
        .iter()
        .find(|c| c.name == CHANNEL)
        .expect("channel")
        .id;
    engine
        .join_channel(only_server(&engine), channel)
        .expect("join");
    let me = snapshot.self_session;
    let peer_session = peer_bot.own_user().expect("peer").session;
    let stranger_session = stranger_bot.own_user().expect("stranger").session;

    // The flag may be reported before `ServerSynced` arrives; the status has it either way.
    let flagged_by = Instant::now() + Duration::from_secs(5);
    while !engine.positional_status().senders.contains(&me) {
        assert!(Instant::now() < flagged_by, "own entry flagged");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let status = engine.positional_status();
    assert_eq!(status.source.as_deref(), Some("Fake game source"));
    assert_eq!(status.game.as_deref(), Some("Fake Game"));
    assert!(status.senders.contains(&me));

    // A person in another game: the server strips the positions, no flag.
    let strangers = [stranger_session];
    let (played, flag) = tokio::join!(
        stranger.play(&clip, talk([1.0, 0.0, 1.0])),
        sending_change(
            &mut rx,
            stranger_session,
            &strangers,
            Duration::from_secs(3)
        ),
    );
    played.expect("stranger plays");
    assert_eq!(flag, None);

    // A person in the same game: flagged while talking, unflagged a few seconds later.
    let (played, flag) = tokio::join!(
        peer.play(&clip, talk([1.0, 0.0, 1.0])),
        sending_change(&mut rx, peer_session, &strangers, Duration::from_secs(3)),
    );
    played.expect("peer plays");
    assert_eq!(flag, Some(true));
    assert_eq!(
        sending_change(&mut rx, peer_session, &[], Duration::from_secs(6)).await,
        Some(false),
        "flag expires"
    );

    // We talk: the peer gets our position, the stranger does not.
    let mut peer_rx = peer.subscribe_packets();
    let mut stranger_rx = stranger.subscribe_packets();
    let (to_peer, to_stranger) = tokio::join!(
        packets_of(&mut peer_rx, me, Duration::from_secs(25)),
        packets_of(&mut stranger_rx, me, Duration::from_secs(25)),
    );
    assert!(to_peer.len() > 20, "peer got {} packets", to_peer.len());
    assert!(
        to_peer
            .iter()
            .all(|p| p.positional == Some([2.0, 0.0, 0.0]))
    );
    assert!(!to_stranger.is_empty());
    assert!(to_stranger.iter().all(|p| p.positional.is_none()));

    // The game closes: the own flag goes away.
    active.store(false, Ordering::Relaxed);
    assert_eq!(
        sending_change(&mut rx, me, &[], Duration::from_secs(2)).await,
        Some(false)
    );
}

/// The test's only connected server (0, which no call accepts, when none is).
fn only_server(engine: &mumcord_engine::Engine) -> mumcord_engine::ServerId {
    engine
        .connections()
        .first()
        .and_then(mumcord_engine::ConnState::server_id)
        .unwrap_or(0)
}
