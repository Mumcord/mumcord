//! Scale probe: connects a headless engine (null audio) to the local load-test server and reports
//! time to sync, event rates towards the UI, CPU and memory. Measurement tool only.
//!
//!     MUMCORD_AUDIO_BACKEND=null cargo run --release -p mumcord-engine --features test-backend \
//!         --example scale_probe -- <port> <seconds> <join-channel-id>
//!
//! Prints one CSV line per 5 s interval on stdout; progress goes to stderr.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use mumcord_engine::{Engine, EngineEvent, PasswordChange, SecretStore, ServerFields};

fn kind(event: &EngineEvent) -> &'static str {
    match event {
        EngineEvent::UserUpserted(_) => "userUpserted",
        EngineEvent::UserRemoved(_) => "userRemoved",
        EngineEvent::TalkStateChanged { .. } => "talk",
        EngineEvent::MessageReceived(_) => "message",
        EngineEvent::ChannelUpserted(_) | EngineEvent::ChannelRemoved(_) => "channel",
        EngineEvent::MicLevel(_) => "micLevel",
        EngineEvent::UnreadChanged(_) => "unread",
        _ => "other",
    }
}

/// (user+system CPU seconds, resident MiB) of this process.
fn proc_stats() -> (f64, f64) {
    let stat = std::fs::read_to_string("/proc/self/stat").unwrap_or_default();
    // Fields after the ")" of the command name; utime/stime are fields 14/15 (1-based).
    let rest = stat.rsplit(')').next().unwrap_or("");
    let fields: Vec<&str> = rest.split_whitespace().collect();
    let ticks = |i: usize| {
        fields
            .get(i)
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.0)
    };
    let cpu = (ticks(11) + ticks(12)) / 100.0;
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let rss_kb = status
        .lines()
        .find(|l| l.starts_with("VmRSS:"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(0.0);
    (cpu, rss_kb / 1024.0)
}

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let port: u16 = args.next().unwrap_or_else(|| "64850".into()).parse()?;
    let seconds: u64 = args.next().unwrap_or_else(|| "60".into()).parse()?;
    let join: u32 = args.next().unwrap_or_else(|| "1".into()).parse()?;

    let dir = PathBuf::from(std::env::var("PROBE_DIR").unwrap_or_else(|_| "probe-data".into()));
    let _ = std::fs::remove_dir_all(&dir);
    let (engine, mut rx) = Engine::new(
        &dir,
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
    )?;
    engine.identity_create("ScaleProbe".into()).await?;
    let server = engine
        .server_add(ServerFields {
            label: "scale".into(),
            address: "127.0.0.1".into(),
            port,
            username: "ScaleProbe".into(),
            password: PasswordChange::Keep,
        })
        .await?;

    let (cpu0, rss0) = proc_stats();
    let started = Instant::now();
    engine.connect(server.id, None)?;
    loop {
        match rx.recv().await {
            Some(EngineEvent::CertTrustRequired { fingerprint, .. }) => {
                engine.cert_trust_decide(server.id, &fingerprint, true)?;
            }
            Some(EngineEvent::ServerSynced(snapshot)) => {
                let synced = started.elapsed();
                let (cpu1, rss1) = proc_stats();
                eprintln!(
                    "SYNCED in {:.0} ms: {} users, {} channels, snapshot debug size {} KiB, \
                     cpu {:.2}s, rss {:.0} -> {:.0} MiB",
                    synced.as_secs_f64() * 1000.0,
                    snapshot.users.len(),
                    snapshot.channels.len(),
                    format!("{snapshot:?}").len() / 1024,
                    cpu1 - cpu0,
                    rss0,
                    rss1
                );
                break;
            }
            Some(_) => {}
            None => anyhow::bail!("event stream closed"),
        }
    }
    // Join for voice so talking from the channel reaches us (a fresh connect is deafened).
    engine.join_channel(join)?;

    println!(
        "t_s,cpu_pct,rss_mib,events_per_s,max_per_100ms,userUpserted,userRemoved,talk,message,channel,micLevel,other"
    );
    let end = Instant::now() + Duration::from_secs(seconds);
    let mut counts: BTreeMap<&'static str, u64> = BTreeMap::new();
    let mut window_start = Instant::now();
    let mut bucket_start = Instant::now();
    let (mut bucket, mut max_bucket) = (0u64, 0u64);
    let (mut last_cpu, _) = proc_stats();
    let mut tick = tokio::time::interval(Duration::from_secs(5));
    tick.tick().await;
    while Instant::now() < end {
        tokio::select! {
            event = rx.recv() => {
                let Some(event) = event else { break };
                *counts.entry(kind(&event)).or_default() += 1;
                if bucket_start.elapsed() >= Duration::from_millis(100) {
                    max_bucket = max_bucket.max(bucket);
                    bucket = 0;
                    bucket_start = Instant::now();
                }
                bucket += 1;
            }
            _ = tick.tick() => {
                let secs = window_start.elapsed().as_secs_f64();
                let (cpu, rss) = proc_stats();
                let total: u64 = counts.values().sum();
                let get = |k: &str| counts.get(k).copied().unwrap_or(0);
                println!(
                    "{:.0},{:.1},{:.0},{:.1},{},{},{},{},{},{},{},{}",
                    started.elapsed().as_secs_f64(),
                    (cpu - last_cpu) / secs * 100.0,
                    rss,
                    total as f64 / secs,
                    max_bucket.max(bucket),
                    get("userUpserted"),
                    get("userRemoved"),
                    get("talk"),
                    get("message"),
                    get("channel"),
                    get("micLevel"),
                    get("other"),
                );
                counts.clear();
                last_cpu = cpu;
                max_bucket = 0;
                window_start = Instant::now();
            }
        }
    }
    engine.disconnect();
    Ok(())
}
