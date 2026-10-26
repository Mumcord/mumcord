//! Screen share end to end (AC-2.29, FR-2.42 – FR-2.46) against the local test server, with the
//! sharer in its own process so its CPU and memory can be measured on their own: the sharer bot
//! is this test binary started again with `SS13_SHARER=1`, driven over stdin/stdout. Ten viewer
//! engines run in this process: eight watch the test pattern at 720p30, the ninth gets "This
//! share is full", one in another channel cannot watch. A throttled upload drops quality to
//! 480p15 and recovers; a second share is watched at the same time; unwatching and stopping end
//! the streams. Prints CPU, memory, bitrate and fps per process
//! (`docs/screen-share-performance.md`).
//! `testenv/up.sh v15`, then:
//! `MUMCORD_LIVE=1 cargo test --release -p mumcord-engine --features test-backend --test live_screen_share_e2e -- --ignored --test-threads=1 --nocapture`
#![cfg(feature = "test-backend")]

mod common;

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mumcord_bot::env;
use mumcord_engine::p2p::P2pConfig;
use mumcord_engine::screen_share::wire::{Frame, FrameHeader, FrameKind};
use mumcord_engine::screen_share::{I420Frame, ScreenFrameSink, ScreenShareView, StartOutcome};
use mumcord_engine::store::Consent;
use mumcord_engine::{
    Engine, EngineEvent, ErrorKind, EventReceiver, PasswordChange, SecretStore, ServerFields,
    ServerId, Snapshot,
};
use tokio::sync::mpsc;

const WAIT: Duration = Duration::from_secs(30);
/// The test server allows 4 plugin messages per second per user (burst 15); a sharer sends
/// three per new viewer (direct connection answer, accept, viewer list).
const WATCH_PACE: Duration = Duration::from_millis(800);
/// How long each measurement runs.
const MEASURE: Duration = Duration::from_secs(10);
/// Upload limits for all eight viewers together: room for eight 480p15 streams (8 × 700 kbps)
/// but not for 720p, and a slow home upload with room for only about half of that.
const THROTTLE_KBPS: u32 = 7000;
const SLOW_KBPS: u32 = 3000;
const VIEWERS: usize = 8;
/// Marks the sharer bot's protocol lines among the test harness output.
const BOT: &str = "SS13BOT ";

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

async fn wait_shares(
    rx: &mut EventReceiver,
    what: &str,
    mut check: impl FnMut(&[ScreenShareView]) -> bool,
) -> Vec<ScreenShareView> {
    wait_for(rx, what, |e| match e {
        EngineEvent::ScreenSharesChanged(shares) if check(shares) => Some(shares.clone()),
        _ => None,
    })
    .await
}

fn share_of(shares: &[ScreenShareView], session: u32) -> Option<&ScreenShareView> {
    shares.iter().find(|s| s.session == session)
}

struct Client {
    engine: Engine,
    rx: EventReceiver,
    snapshot: Snapshot,
}

impl Client {
    fn server(&self) -> ServerId {
        self.engine
            .connections()
            .first()
            .and_then(mumcord_engine::ConnState::server_id)
            .unwrap_or(0)
    }

    fn session(&self) -> u32 {
        self.snapshot.self_session
    }

    fn channel(&self, name: &str) -> u32 {
        self.snapshot
            .channels
            .iter()
            .find(|ch| ch.name == name)
            .unwrap_or_else(|| panic!("channel {name}"))
            .id
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
    let mut settings = engine.settings().expect("settings");
    settings.allow_direct_connections = Consent::Yes;
    engine.update_settings(settings).await.expect("consent");
    // Engines on one machine reach each other over loopback; the router is never asked.
    engine.p2p_set_config(P2pConfig {
        connect_timeout: Duration::from_secs(10),
        port_mapping: false,
        allow_loopback: true,
    });
    let name = unique(who);
    let server = engine
        .server_add(ServerFields {
            label: format!("test {} {name}", common::v15()),
            address: "127.0.0.1".into(),
            port: common::v15(),
            username: name,
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
        snapshot,
    }
}

async fn wait_watchable(client: &mut Client, sharer: u32) {
    if client
        .engine
        .screen_shares(client.server())
        .iter()
        .any(|s| s.session == sharer && s.blocked.is_none())
    {
        return;
    }
    wait_shares(&mut client.rx, "a watchable share", |shares| {
        share_of(shares, sharer).is_some_and(|s| s.blocked.is_none())
    })
    .await;
}

/// What one viewer received: arrival time, header and size of every frame.
#[derive(Default)]
struct Received {
    encoded: Vec<(Instant, u32, FrameHeader, usize)>,
    decoded: Vec<Instant>,
}

#[derive(Default)]
struct Sink(Mutex<Received>);

impl ScreenFrameSink for Sink {
    fn on_encoded(&self, _: ServerId, sharer: u32, frame: &Frame) {
        self.0.lock().expect("sink").encoded.push((
            Instant::now(),
            sharer,
            frame.header,
            frame.payload.len(),
        ));
    }

    fn on_decoded(&self, _: ServerId, _: u32, _: &I420Frame) {
        self.0.lock().expect("sink").decoded.push(Instant::now());
    }
}

/// Video frames per second, received kbps and newest frame size over the last `window`.
struct Seen {
    fps: f64,
    kbps: f64,
    size: (u16, u16),
    decoded_fps: f64,
}

impl Sink {
    fn recent(&self, window: Duration) -> Seen {
        self.recent_from(None, window)
    }

    /// Like [`Sink::recent`], only counting frames of `sharer` when given.
    fn recent_from(&self, sharer: Option<u32>, window: Duration) -> Seen {
        let got = self.0.lock().expect("sink");
        let since = Instant::now() - window;
        let video = || {
            got.encoded.iter().filter(|(at, from, h, _)| {
                *at >= since && h.kind == FrameKind::Video && sharer.is_none_or(|s| s == *from)
            })
        };
        let bytes: usize = video().map(|(.., len)| len).sum();
        Seen {
            fps: video().count() as f64 / window.as_secs_f64(),
            kbps: bytes as f64 * 8.0 / 1000.0 / window.as_secs_f64(),
            size: video()
                .next_back()
                .map_or((0, 0), |(_, _, h, _)| (h.width, h.height)),
            decoded_fps: got.decoded.iter().filter(|at| **at >= since).count() as f64
                / window.as_secs_f64(),
        }
    }

    fn first_keyframe(&self) -> Option<bool> {
        self.0
            .lock()
            .expect("sink")
            .encoded
            .iter()
            .find(|(_, _, h, _)| h.kind == FrameKind::Video)
            .map(|(_, _, h, _)| h.keyframe)
    }

    fn count(&self) -> usize {
        self.0.lock().expect("sink").encoded.len()
    }
}

/// CPU seconds (user + system) of a process or thread from its `stat` file.
fn stat_cpu(stat: &str) -> f64 {
    let fields: Vec<&str> = stat
        .rsplit_once(')')
        .map(|(_, rest)| rest.split_whitespace().collect())
        .unwrap_or_default();
    let ticks = |i: usize| {
        fields
            .get(i)
            .and_then(|f| f.parse::<f64>().ok())
            .unwrap_or(0.0)
    };
    // utime and stime are fields 14 and 15; `fields` starts at field 3.
    (ticks(11) + ticks(12)) / 100.0
}

fn process_cpu(pid: u32) -> f64 {
    stat_cpu(&std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default())
}

/// CPU seconds per thread name, summed over threads of the same name.
fn thread_cpu(pid: u32) -> BTreeMap<String, f64> {
    let mut by_name = BTreeMap::new();
    let Ok(tasks) = std::fs::read_dir(format!("/proc/{pid}/task")) else {
        return by_name;
    };
    for task in tasks.flatten() {
        let stat = std::fs::read_to_string(task.path().join("stat")).unwrap_or_default();
        let name = stat
            .split_once('(')
            .and_then(|(_, rest)| rest.rsplit_once(')'))
            .map_or_else(String::new, |(name, _)| name.to_owned());
        *by_name.entry(name).or_insert(0.0) += stat_cpu(&stat);
    }
    by_name
}

/// Resident and peak resident memory in MiB.
fn memory_mib(pid: u32) -> (f64, f64) {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap_or_default();
    let field = |key: &str| {
        status
            .lines()
            .find_map(|l| l.strip_prefix(key))
            .and_then(|v| v.split_whitespace().next())
            .and_then(|kb| kb.parse::<f64>().ok())
            .unwrap_or(0.0)
            / 1024.0
    };
    (field("VmRSS:"), field("VmHWM:"))
}

/// The sharer bot's numbers, as it prints them.
#[derive(Debug, Default)]
struct BotStats {
    step: usize,
    width: u32,
    height: u32,
    fps: u32,
    kbps: u32,
    encode_ms: f64,
    keyframes: u32,
    viewers: usize,
}

impl BotStats {
    fn parse(line: &str) -> Self {
        let v: Vec<&str> = line.split_whitespace().collect();
        let n = |i: usize| v.get(i).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
        Self {
            step: n(0) as usize,
            width: n(1) as u32,
            height: n(2) as u32,
            fps: n(3) as u32,
            kbps: n(4) as u32,
            encode_ms: n(5),
            keyframes: n(6) as u32,
            viewers: n(7) as usize,
        }
    }
}

/// The sharer in its own process, driven line by line.
struct SharerBot {
    child: Child,
    stdin: ChildStdin,
    lines: mpsc::UnboundedReceiver<String>,
}

impl SharerBot {
    fn spawn() -> Self {
        let mut child = Command::new(std::env::current_exe().expect("test binary"))
            .args([
                "sharer_bot",
                "--exact",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("SS13_SHARER", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("sharer bot");
        let stdin = child.stdin.take().expect("bot stdin");
        let stdout = child.stdout.take().expect("bot stdout");
        let (tx, lines) = mpsc::unbounded_channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                // The harness prints "test sharer_bot ... " in front of the first line.
                if let Some((_, msg)) = line.split_once(BOT) {
                    let _ = tx.send(msg.to_owned());
                }
            }
        });
        Self {
            child,
            stdin,
            lines,
        }
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Sends `command` and waits for the bot's answer.
    async fn ask(&mut self, command: &str) -> String {
        writeln!(self.stdin, "{command}").expect("bot command");
        self.stdin.flush().expect("bot flush");
        self.next(command).await
    }

    async fn next(&mut self, what: &str) -> String {
        let answer = tokio::time::timeout(WAIT, self.lines.recv())
            .await
            .unwrap_or_else(|_| panic!("sharer bot did not answer {what}"))
            .expect("sharer bot alive");
        assert!(!answer.starts_with("error"), "sharer bot: {what}: {answer}");
        answer
    }

    async fn stats(&mut self) -> BotStats {
        BotStats::parse(&self.ask("stats").await)
    }
}

/// CPU of the sharer process and of this (viewer) process over [`MEASURE`].
struct Load {
    sharer: f64,
    viewers: f64,
    sharer_threads: Vec<(String, f64)>,
}

async fn measure(bot_pid: u32) -> Load {
    let own = std::process::id();
    let (s0, v0, t0) = (process_cpu(bot_pid), process_cpu(own), thread_cpu(bot_pid));
    tokio::time::sleep(MEASURE).await;
    let (s1, v1, t1) = (process_cpu(bot_pid), process_cpu(own), thread_cpu(bot_pid));
    let secs = MEASURE.as_secs_f64();
    let mut threads: BTreeMap<String, f64> = BTreeMap::new();
    for (name, cpu) in t1 {
        let before = t0.get(&name).copied().unwrap_or(0.0);
        *threads.entry(name).or_insert(0.0) += (cpu - before) / secs * 100.0;
    }
    let mut sharer_threads: Vec<(String, f64)> =
        threads.into_iter().filter(|(_, pct)| *pct >= 0.5).collect();
    sharer_threads.sort_by(|a, b| b.1.total_cmp(&a.1));
    Load {
        sharer: (s1 - s0) / secs * 100.0,
        viewers: (v1 - v0) / secs * 100.0,
        sharer_threads,
    }
}

fn report(label: &str, load: &Load, stats: &BotStats, bot_pid: u32, sinks: &[Arc<Sink>]) {
    let (rss, peak) = memory_mib(bot_pid);
    let (vrss, vpeak) = memory_mib(std::process::id());
    println!("== {label}");
    println!(
        "sharer: CPU {:.0}% of one core, RSS {rss:.0} MiB (peak {peak:.0}); encoder {}x{} {} fps {} kbps, encode {:.1} ms/frame, {} keyframes, {} viewers",
        load.sharer,
        stats.width,
        stats.height,
        stats.fps,
        stats.kbps,
        stats.encode_ms,
        stats.keyframes,
        stats.viewers
    );
    let threads: Vec<String> = load
        .sharer_threads
        .iter()
        .map(|(name, pct)| format!("{name} {pct:.0}%"))
        .collect();
    println!("sharer threads: {}", threads.join(", "));
    println!(
        "viewer process (10 engines): CPU {:.0}% of one core, RSS {vrss:.0} MiB (peak {vpeak:.0})",
        load.viewers
    );
    for (i, sink) in sinks.iter().enumerate() {
        let seen = sink.recent(MEASURE);
        println!(
            "viewer {i}: {:.1} fps, {:.0} kbps, {:?}, decoded {:.1} fps",
            seen.fps, seen.kbps, seen.size, seen.decoded_fps
        );
    }
}

/// The sharer bot: connects, prints its session, then follows commands from stdin. Only runs
/// when started by [`screen_share_ac_2_29`].
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "started by screen_share_ac_2_29"]
async fn sharer_bot() {
    if std::env::var("SS13_SHARER").is_err() {
        return;
    }
    // SAFETY: set before any audio thread exists; the bot runs alone in its process.
    unsafe { std::env::set_var("MUMCORD_AUDIO_BACKEND", "null") };
    let dir = work_dir("live_screen_share_e2e_sharer");
    let alice = start(&dir, "ss13-sharer").await;
    let server = alice.server();
    println!("{BOT}ready {}", alice.session());
    let (tx, mut commands) = mpsc::unbounded_channel();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    while let Some(line) = commands.recv().await {
        let mut words = line.split_whitespace();
        let answer = match (words.next(), words.next()) {
            (Some("start"), _) => match alice.engine.screen_share_start(server, false).await {
                Ok(StartOutcome::Started) => "ok".to_owned(),
                Ok(StartOutcome::Cancelled) => "error cancelled".to_owned(),
                Err(e) => format!("error {e}"),
            },
            (Some("limit"), kbps) => {
                let kbps = kbps.and_then(|k| k.parse().ok());
                match alice
                    .engine
                    .screen_share_set_upload_limit(server, None, kbps)
                {
                    Ok(()) => "ok".to_owned(),
                    Err(e) => format!("error {e}"),
                }
            }
            (Some("stats"), _) => match alice.engine.screen_share_media_stats(server) {
                Some(s) => format!(
                    "{} {} {} {} {} {:.2} {} {}",
                    s.step,
                    s.width,
                    s.height,
                    s.fps,
                    s.kbps,
                    s.encode_ms,
                    s.keyframes,
                    s.viewers.len()
                ),
                None => "error not sharing".to_owned(),
            },
            (Some("stop"), _) => match alice.engine.screen_share_stop(server) {
                Ok(()) => "ok".to_owned(),
                Err(e) => format!("error {e}"),
            },
            (Some("quit"), _) => break,
            _ => format!("error unknown command {line}"),
        };
        println!("{BOT}{answer}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v15 (MUMCORD_LIVE=1)"]
async fn screen_share_ac_2_29() {
    if !live() || std::env::var("SS13_SHARER").is_ok() {
        return;
    }
    // SAFETY: set before any audio thread exists; tests run with --test-threads=1.
    unsafe { std::env::set_var("MUMCORD_AUDIO_BACKEND", "null") };
    let dir = work_dir("live_screen_share_e2e");
    let mut bot = SharerBot::spawn();
    let a: u32 = bot
        .next("ready")
        .await
        .strip_prefix("ready ")
        .and_then(|s| s.parse().ok())
        .expect("sharer session");
    let pid = bot.pid();

    let mut viewers = Vec::new();
    for i in 0..=VIEWERS {
        viewers.push(start(&dir, &format!("ss13-v{i}")).await);
    }
    let outsider = start(&dir, "ss13-out").await;
    let afk = outsider.channel("AFK");
    outsider
        .engine
        .join_channel(outsider.server(), afk)
        .expect("move outsider");
    let sinks: Vec<Arc<Sink>> = (0..=VIEWERS).map(|_| Arc::new(Sink::default())).collect();
    for (v, sink) in viewers.iter().zip(&sinks) {
        v.engine.set_screen_frame_sink(sink.clone());
    }
    let outsider_sink = Arc::new(Sink::default());
    outsider.engine.set_screen_frame_sink(outsider_sink.clone());

    bot.ask("start").await;
    for v in &mut viewers {
        wait_watchable(v, a).await;
    }

    // Someone in another channel neither sees the share nor can watch it.
    assert!(
        outsider.engine.screen_shares(outsider.server()).is_empty(),
        "no share outside the channel"
    );
    let refused = outsider
        .engine
        .screen_share_watch(outsider.server(), a)
        .await
        .expect_err("outsider cannot watch");
    println!("outsider: {refused}");

    // Eight watch over direct streams; the ninth gets "This share is full".
    for v in &viewers[..VIEWERS] {
        v.engine
            .screen_share_watch(v.server(), a)
            .await
            .expect("watch");
        assert!(v.engine.p2p_connected(v.server(), a), "direct connection");
        tokio::time::sleep(WATCH_PACE).await;
    }
    let ninth = &viewers[VIEWERS];
    let full = ninth
        .engine
        .screen_share_watch(ninth.server(), a)
        .await
        .expect_err("full");
    assert_eq!(full.kind, ErrorKind::ShareFull);
    println!("ninth viewer: {full}");

    // All eight get 720p30, starting at a keyframe.
    tokio::time::sleep(Duration::from_secs(3)).await;
    let load = measure(pid).await;
    let stats = bot.stats().await;
    report("720p30, 8 viewers", &load, &stats, pid, &sinks[..VIEWERS]);
    assert_eq!((stats.step, stats.viewers), (0, VIEWERS));
    for (i, sink) in sinks[..VIEWERS].iter().enumerate() {
        let seen = sink.recent(MEASURE);
        assert_eq!(
            sink.first_keyframe(),
            Some(true),
            "viewer {i} keyframe first"
        );
        assert_eq!(seen.size, (1280, 720), "viewer {i} size");
        assert!(seen.fps >= 25.0, "viewer {i} gets {:.1} fps", seen.fps);
    }
    assert_eq!(sinks[VIEWERS].count(), 0, "the ninth gets no frames");
    assert_eq!(outsider_sink.count(), 0, "the outsider gets no frames");

    // Worst case for viewers: everyone decodes in Rust (the fallback without WebCodecs).
    for v in &viewers[..VIEWERS] {
        v.engine
            .screen_share_set_decode_fallback(v.server(), a, true)
            .expect("fallback");
    }
    tokio::time::sleep(Duration::from_secs(2)).await;
    let load = measure(pid).await;
    let stats = bot.stats().await;
    report(
        "720p30, 8 viewers, all decoding in Rust",
        &load,
        &stats,
        pid,
        &sinks[..VIEWERS],
    );
    for (i, sink) in sinks[..VIEWERS].iter().enumerate() {
        let seen = sink.recent(MEASURE);
        assert!(
            seen.decoded_fps >= 25.0,
            "viewer {i} decodes {:.1} fps",
            seen.decoded_fps
        );
    }
    for v in &viewers[..VIEWERS] {
        v.engine
            .screen_share_set_decode_fallback(v.server(), a, false)
            .expect("fallback off");
    }

    // A slow upload for everyone steps down to 480p15 ...
    let stats = throttle(&mut bot, THROTTLE_KBPS, pid, &sinks[..VIEWERS]).await;
    assert_eq!((stats.width, stats.height), (854, 480));
    assert!(stats.fps <= 16, "sender at {} fps", stats.fps);
    for (i, sink) in sinks[..VIEWERS].iter().enumerate() {
        let seen = sink.recent(MEASURE);
        assert_eq!(seen.size, (854, 480), "viewer {i} size");
        assert!(seen.fps >= 12.0, "viewer {i} gets {:.1} fps", seen.fps);
    }
    // ... and below what 480p15 needs, viewers lose frame rate but keep the picture.
    let stats = throttle(&mut bot, SLOW_KBPS, pid, &sinks[..VIEWERS]).await;
    assert_eq!((stats.width, stats.height), (854, 480));
    for (i, sink) in sinks[..VIEWERS].iter().enumerate() {
        let seen = sink.recent(MEASURE);
        assert_eq!(seen.size, (854, 480), "viewer {i} size");
        assert!(seen.fps > 0.0, "viewer {i} still gets frames");
    }

    // ... and back up once the upload recovers.
    bot.ask("limit").await;
    let began = Instant::now();
    let deadline = began + Duration::from_secs(60);
    loop {
        if bot.stats().await.step == 0 {
            break;
        }
        assert!(Instant::now() < deadline, "no 720p30 within 60 s");
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    println!(
        "recovered to 720p30 after {:.1} s",
        began.elapsed().as_secs_f64()
    );
    tokio::time::sleep(Duration::from_secs(4)).await;
    for (i, sink) in sinks[..VIEWERS].iter().enumerate() {
        let seen = sink.recent(Duration::from_secs(2));
        assert_eq!(seen.size, (1280, 720), "viewer {i} back at 720p");
        assert!(seen.fps >= 20.0, "viewer {i} back at {:.1} fps", seen.fps);
    }

    // A second share in the channel is watched at the same time as the first.
    let b = viewers[VIEWERS].session();
    let second = &viewers[VIEWERS];
    second
        .engine
        .screen_share_start(second.server(), false)
        .await
        .expect("second share");
    wait_watchable(&mut viewers[1], b).await;
    viewers[1]
        .engine
        .screen_share_watch(viewers[1].server(), b)
        .await
        .expect("watch the second share");
    tokio::time::sleep(Duration::from_secs(3)).await;
    let from_a = sinks[1].recent_from(Some(a), Duration::from_secs(2));
    let from_b = sinks[1].recent_from(Some(b), Duration::from_secs(2));
    println!(
        "viewer 1 watching two shares: {:.1} fps {:?} and {:.1} fps {:?}",
        from_a.fps, from_a.size, from_b.fps, from_b.size
    );
    assert!(from_a.fps >= 25.0 && from_b.fps >= 25.0, "both shares play");
    viewers[1]
        .engine
        .screen_share_unwatch(viewers[1].server(), b)
        .expect("unwatch the second share");
    let second = &viewers[VIEWERS];
    second
        .engine
        .screen_share_stop(second.server())
        .expect("stop the second share");
    wait_shares(&mut viewers[1].rx, "the second share gone", |shares| {
        share_of(shares, b).is_none()
    })
    .await;

    // "Stop watching" ends that stream and frees the place for the ninth.
    let v0 = viewers[0].session();
    viewers[0]
        .engine
        .screen_share_unwatch(viewers[0].server(), a)
        .expect("unwatch");
    wait_shares(&mut viewers[1].rx, "the unwatch", |shares| {
        share_of(shares, a)
            .is_some_and(|s| s.viewers.len() == VIEWERS - 1 && !s.viewers.contains(&v0))
    })
    .await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    let after = sinks[0].count();
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(sinks[0].count(), after, "no frames after stop watching");
    tokio::time::sleep(WATCH_PACE).await;
    let ninth = &viewers[VIEWERS];
    ninth
        .engine
        .screen_share_watch(ninth.server(), a)
        .await
        .expect("the freed place");
    tokio::time::sleep(Duration::from_secs(3)).await;
    let seen = sinks[VIEWERS].recent(Duration::from_secs(2));
    assert_eq!(sinks[VIEWERS].first_keyframe(), Some(true));
    assert!(seen.fps >= 25.0, "the ninth gets {:.1} fps", seen.fps);
    assert_eq!(bot.stats().await.viewers, VIEWERS);

    // Stopping ends the share for everyone.
    bot.ask("stop").await;
    for v in &mut viewers {
        let gone = v.engine.screen_shares(v.server()).is_empty();
        if !gone {
            wait_shares(&mut v.rx, "the share gone", <[_]>::is_empty).await;
        }
    }
    tokio::time::sleep(Duration::from_secs(1)).await;
    let counts: Vec<usize> = sinks.iter().map(|s| s.count()).collect();
    tokio::time::sleep(Duration::from_secs(2)).await;
    let later: Vec<usize> = sinks.iter().map(|s| s.count()).collect();
    assert_eq!(counts, later, "no frames after stop");

    writeln!(bot.stdin, "quit").expect("quit");
    let status = bot.child.wait().expect("sharer bot exit");
    assert!(status.success(), "sharer bot failed: {status}");
}

/// Limits the sharer's whole upload to `kbps`, waits for 480p15 and measures.
async fn throttle(bot: &mut SharerBot, kbps: u32, pid: u32, sinks: &[Arc<Sink>]) -> BotStats {
    bot.ask(&format!("limit {kbps}")).await;
    let began = Instant::now();
    let deadline = began + Duration::from_secs(30);
    while bot.stats().await.step != 2 {
        assert!(
            Instant::now() < deadline,
            "no 480p15 within 30 s at {kbps} kbps"
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    println!(
        "upload limited to {kbps} kbps: 480p15 after {:.1} s",
        began.elapsed().as_secs_f64()
    );
    // Long enough for the failed tries to step up (after 5 s, then 10 s) to be over, so the
    // measurement shows the steady state under the limit.
    let settle_end = Instant::now() + Duration::from_secs(22);
    let (mut step, mut tries) = (2, 0);
    while Instant::now() < settle_end {
        let now = bot.stats().await.step;
        if now < step {
            tries += 1;
        }
        step = now;
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    println!("{tries} steps up while settling at {kbps} kbps");
    let load = measure(pid).await;
    let stats = bot.stats().await;
    report(
        &format!("throttled to {kbps} kbps"),
        &load,
        &stats,
        pid,
        sinks,
    );
    stats
}
