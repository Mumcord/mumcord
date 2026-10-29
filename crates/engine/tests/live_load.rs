//! Voice quality under CPU load (issue #123), against the local 1.5 test server: a bot plays a
//! steady tone to us while we send speech to it, both at once like a call. The test reports
//! glitches in what we play back, gaps in what the bot receives and the audio health counters of
//! the connection info, and checks that a quiet machine plays clean audio.
//!
//! Run it once on a quiet machine and once while the CPU is loaded to compare (for example
//! `stress-ng --cpu $(nproc)` in a second terminal):
//! `testenv/up.sh v15`, then:
//! `MUMCORD_LIVE=1 cargo test -p mumcord-engine --features test-backend --test live_load -- --ignored --nocapture`
#![cfg(feature = "test-backend")]

mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use mumbot::voice::{BotVoice, Clip, PlayOptions};
use mumbot::{Bot, BotConfig, env};
use mumcord_engine::EventReceiver;
use mumcord_engine::{EngineEvent, PasswordChange, SecretStore, ServerFields, Snapshot};

const WAIT: Duration = Duration::from_secs(20);
const SAMPLE_RATE: usize = 48_000;
const MEASURE: Duration = Duration::from_secs(20);
/// Silence before the peer starts talking, to check that idle time is not counted as a glitch.
const IDLE: Duration = Duration::from_secs(5);
const TONE_HZ: f32 = 440.0;
const TONE_AMPLITUDE: f32 = 0.2;
/// Samples per 10 ms block of the glitch analysis.
const BLOCK: usize = SAMPLE_RATE / 100;
/// Mumble packets carry 20 ms (two 10 ms sequence steps) with the engine's encoder settings.
const SEQ_PER_PACKET: u64 = 2;

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

/// Glitches in a recording of the steady tone.
#[derive(Debug, Default, PartialEq)]
struct ToneGlitches {
    blocks: usize,
    /// 10 ms blocks with a click: a jump the tone cannot make (a gap edge, a splice, a phase
    /// jump of concealment).
    clicks: usize,
    /// 10 ms blocks far below the tone's level (silence or faded concealment).
    dropouts: usize,
}

/// A sine satisfies `x[n+1] + x[n-1] = 2 cos(ω) x[n]`; the residual of that recursion stays at
/// the codec's noise floor for clean audio and jumps at every discontinuity.
fn tone_glitches(samples: &[f32]) -> ToneGlitches {
    let k = 2.0 * (TONE_HZ * std::f32::consts::TAU / SAMPLE_RATE as f32).cos();
    let click = TONE_AMPLITUDE / 4.0;
    let quiet = TONE_AMPLITUDE / std::f32::consts::SQRT_2 / 2.0;
    let mut glitches = ToneGlitches::default();
    for block in samples.as_chunks::<BLOCK>().0 {
        glitches.blocks += 1;
        let residual = block
            .windows(3)
            .map(|w| (w[2] + w[0] - k * w[1]).abs())
            .fold(0.0f32, f32::max);
        let rms = (block.iter().map(|s| s * s).sum::<f32>() / BLOCK as f32).sqrt();
        if rms < quiet {
            glitches.dropouts += 1;
        } else if residual > click {
            glitches.clicks += 1;
        }
    }
    glitches
}

/// Gaps in the packets the bot received from us.
#[derive(Debug, Default, PartialEq)]
struct SendGaps {
    packets: usize,
    /// Packets missing inside a talk spurt (sequence jumps).
    missing: u64,
    /// Arrivals more than 40 ms after the previous packet of the spurt (one 20 ms packet late).
    late: usize,
    max_gap: Duration,
}

/// `(arrival, seq, is_last)` of every packet in arrival order.
fn send_gaps(packets: &[(Instant, u64, bool)]) -> SendGaps {
    let mut gaps = SendGaps {
        packets: packets.len(),
        ..SendGaps::default()
    };
    for pair in packets.windows(2) {
        let ((at, seq, last), (next_at, next_seq, _)) = (pair[0], pair[1]);
        // A new spurt starts after the last packet or after a pause in the speech.
        if last || next_seq <= seq || next_seq - seq > 50 * SEQ_PER_PACKET {
            continue;
        }
        gaps.missing += (next_seq - seq) / SEQ_PER_PACKET - 1;
        let gap = next_at.saturating_duration_since(at);
        if gap > Duration::from_millis(40) {
            gaps.late += 1;
        }
        gaps.max_gap = gaps.max_gap.max(gap);
    }
    gaps
}

#[test]
fn glitch_analysis_finds_gaps_and_jumps() {
    let tone: Vec<f32> = (0..SAMPLE_RATE)
        .map(|i| {
            TONE_AMPLITUDE * (i as f32 * TONE_HZ * std::f32::consts::TAU / SAMPLE_RATE as f32).sin()
        })
        .collect();
    assert_eq!(
        tone_glitches(&tone),
        ToneGlitches {
            blocks: 100,
            ..ToneGlitches::default()
        }
    );
    let mut broken = tone.clone();
    broken[10 * BLOCK..12 * BLOCK].fill(0.0);
    broken.drain(50 * BLOCK + 7..50 * BLOCK + 40);
    let found = tone_glitches(&broken);
    assert_eq!((found.dropouts, found.clicks), (2, 1), "{found:?}");

    let t0 = Instant::now();
    let at = |ms: u64| t0 + Duration::from_millis(ms);
    let gaps = send_gaps(&[
        (at(0), 10, false),
        (at(20), 12, false),
        (at(70), 16, false),
        (at(90), 18, true),
        (at(900), 100, false),
    ]);
    assert_eq!((gaps.packets, gaps.missing, gaps.late), (5, 1, 1));
    assert_eq!(gaps.max_gap, Duration::from_millis(50));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v15 (MUMCORD_LIVE=1)"]
async fn voice_stays_clean_under_load() {
    if !live() {
        return;
    }
    let dir = work_dir("engine-live-load");
    let name = unique("mc-me");
    let (input, output) = (dir.join("in.wav"), dir.join("out.wav"));
    let speech =
        Clip::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testenv/audio/speech.wav"))
            .expect("speech clip");
    let mic: Vec<f32> = speech
        .samples
        .iter()
        .copied()
        .cycle()
        .take(SAMPLE_RATE * 60)
        .collect();
    Clip::from_samples(mic).save(&input).expect("save input");
    // SAFETY: set before any audio thread exists; no other thread reads the environment yet.
    unsafe {
        std::env::set_var(
            "MUMCORD_AUDIO_BACKEND",
            format!("file:{}:{}", input.display(), output.display()),
        );
    }

    let peer = Bot::connect(BotConfig {
        data_dir: Some(dir.join("peer")),
        ..BotConfig::new(&unique("mc-peer"), common::v15())
    })
    .await
    .expect("peer");
    peer.join("AFK").await.expect("peer joins");
    let peer_voice = BotVoice::start(&peer).await.expect("peer voice");
    let tone = Clip::from_samples(
        (0..SAMPLE_RATE)
            .map(|i| {
                TONE_AMPLITUDE
                    * (i as f32 * TONE_HZ * std::f32::consts::TAU / SAMPLE_RATE as f32).sin()
            })
            .collect(),
    );

    let (engine, mut rx) = mumcord_engine::Engine::new(
        &dir.join("data"),
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
    )
    .expect("engine");
    engine
        .identity_create(name.clone())
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
    let snapshot: Snapshot = wait_for(&mut rx, "synced", |e| match e {
        EngineEvent::CertTrustRequired { fingerprint, .. } => {
            engine
                .cert_trust_decide(server.id, fingerprint, true)
                .expect("trust");
            None
        }
        EngineEvent::ServerSynced(s) => Some((**s).clone()),
        _ => None,
    })
    .await;
    let afk = snapshot
        .channels
        .iter()
        .find(|c| c.name == "AFK")
        .expect("AFK")
        .id;
    engine.join_channel(server.id, afk).expect("join AFK");
    let me = snapshot.self_session;

    // Nobody else talks yet: idle silence must not count as concealment (issue #152).
    tokio::time::sleep(IDLE).await;
    let idle = engine
        .connection_stats(server.id)
        .and_then(|s| s.audio)
        .expect("audio health while connected");
    eprintln!(
        "idle: {:?} {:?}, concealed {}",
        idle.rating.rating, idle.rating.issue, idle.concealed_frames
    );
    assert_eq!(
        idle.concealed_frames, 0,
        "silence is not concealment: {idle:?}"
    );

    let mut incoming = peer_voice.subscribe_packets();
    let collect = tokio::spawn(async move {
        let mut packets = Vec::new();
        let deadline = tokio::time::Instant::now() + MEASURE + Duration::from_secs(2);
        while let Ok(Ok(packet)) = tokio::time::timeout_at(deadline, incoming.recv()).await {
            if packet.session == me {
                packets.push((Instant::now(), packet.seq, packet.is_last));
            }
        }
        packets
    });
    let played = peer_voice
        .play(
            &tone,
            PlayOptions {
                forever: true,
                max_duration: Some(MEASURE + Duration::from_secs(2)),
                ..PlayOptions::default()
            },
        )
        .await;
    played.expect("peer plays");
    let health = engine
        .connection_stats(server.id)
        .and_then(|s| s.audio)
        .expect("audio health while connected");
    let packets = collect.await.expect("packet collector");
    engine.disconnect(server.id);
    drop(engine);

    let recording = Clip::load(&output).expect("recording").samples;
    // The tone starts after the idle phase; earlier sound is the join notification.
    let idle_samples = IDLE.as_secs() as usize * SAMPLE_RATE;
    let start = idle_samples
        + recording[idle_samples.min(recording.len())..]
            .iter()
            .position(|s| s.abs() > TONE_AMPLITUDE / 2.0)
            .expect("tone reached the recording");
    // Skip the first second (jitter buffer settling, join sound) and the fade at the end.
    let window = &recording[(start + SAMPLE_RATE).min(recording.len())
        ..(start + MEASURE.as_secs() as usize * SAMPLE_RATE).min(recording.len())];
    let playback = tone_glitches(window);
    let sending = send_gaps(&packets);
    eprintln!("playback (tone from the peer): {playback:?}");
    eprintln!("sending (speech to the peer): {sending:?}");
    eprintln!("audio health: {health:?}");
    eprintln!(
        "rating under the call: {:?} {:?}",
        health.rating.rating, health.rating.issue
    );

    assert!(playback.blocks > 1500, "tone recorded: {playback:?}");
    assert!(sending.packets > 200, "speech received: {sending:?}");
    // The test machine may be busy, so only gross breakage fails the test; compare the printed
    // figures between a quiet and a loaded run.
    assert!(
        (playback.clicks + playback.dropouts) * 20 < playback.blocks,
        "playback broken: {playback:?} {health:?}"
    );
}
