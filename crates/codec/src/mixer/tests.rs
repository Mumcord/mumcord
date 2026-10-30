use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::*;
use crate::jitter::tests::{TestPacket, tone_packets, tone_packets_at};

const FRAME_MS: u64 = 10;

fn packet(session: u32, p: &TestPacket, via_listener: bool) -> VoicePacket {
    VoicePacket::new(
        session,
        p.seq,
        &p.payload,
        p.is_last,
        via_listener,
        Instant::now(),
    )
    .expect("payload fits")
}

struct Speech<'a> {
    session: u32,
    packets: &'a [TestPacket],
    start_ms: u64,
    via_listener: bool,
}

/// Feeds every speaker's packets on a clean 20 ms schedule and mixes until
/// `end_ms`. Returns the mixed (mono) output and the talk events with their time.
fn run(
    mixer: &mut Mixer,
    speech: &[Speech<'_>],
    mix_in: Option<&Frame>,
    end_ms: u64,
) -> (Vec<f32>, Vec<(u64, TalkEvent)>) {
    let (left, right, events) = run_stereo(mixer, speech, &[], mix_in, end_ms);
    assert_eq!(left, right, "voices without position play centered");
    (left, events)
}

/// [`run`] with speaker positions (session, position); returns both channels.
fn run_stereo(
    mixer: &mut Mixer,
    speech: &[Speech<'_>],
    positions: &[(u32, [f32; 3])],
    mix_in: Option<&Frame>,
    end_ms: u64,
) -> (Vec<f32>, Vec<f32>, Vec<(u64, TalkEvent)>) {
    let mut out = Vec::new();
    let mut out_right = Vec::new();
    let mut events = Vec::new();
    let mut frame = [0.0; FRAME_SAMPLES];
    let mut right = [0.0; FRAME_SAMPLES];
    let mut now = 0;
    while now < end_ms {
        for s in speech {
            let first_seq = s.packets[0].seq;
            for p in s.packets {
                let arrival = s.start_ms + (p.seq - first_seq) * FRAME_MS + 1;
                if arrival + FRAME_MS > now && arrival <= now {
                    let position = positions
                        .iter()
                        .find(|(session, _)| *session == s.session)
                        .map(|(_, p)| *p);
                    let pkt = packet(s.session, p, s.via_listener).with_position(position);
                    mixer.push_packet(&pkt, arrival);
                }
            }
        }
        mixer.mix(&mut frame, &mut right, mix_in, now, &mut |e| {
            events.push((now, e))
        });
        out.extend_from_slice(&frame);
        out_right.extend_from_slice(&right);
        now += FRAME_MS;
    }
    (out, out_right, events)
}

fn rms(samples: &[f32]) -> f32 {
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

/// RMS of the steady middle of a one-second burst starting at 0 ms.
fn steady_rms(out: &[f32]) -> f32 {
    rms(&out[200 * 48..800 * 48])
}

fn one_speaker(configure: impl FnOnce(&mut Mixer), via_listener: bool) -> Vec<f32> {
    speaker_at(0.4, configure, via_listener)
}

/// One speaker playing a tone with peak `amplitude`.
fn speaker_at(amplitude: f32, configure: impl FnOnce(&mut Mixer), via_listener: bool) -> Vec<f32> {
    let packets = tone_packets_at(amplitude, 50, 0);
    let mut mixer = Mixer::new().expect("mixer");
    configure(&mut mixer);
    let speech = [Speech {
        session: 7,
        packets: &packets,
        start_ms: 0,
        via_listener,
    }];
    run(&mut mixer, &speech, None, 1_200).0
}

/// Loudness change of the steady tone in dB relative to `normal`.
fn db(level: f32, normal: f32) -> f32 {
    20.0 * (level / normal).log10()
}

#[test]
fn volume_follows_a_perceptual_curve_and_mute_silences() {
    let normal = steady_rms(&one_speaker(|_| {}, false));
    let full = steady_rms(&one_speaker(|m| assert!(m.set_volume(7, 1.0)), false));
    let half = steady_rms(&one_speaker(|m| assert!(m.set_volume(7, 0.5)), false));
    let quarter = steady_rms(&one_speaker(|m| assert!(m.set_volume(7, 0.25)), false));
    let muted = steady_rms(&one_speaker(|m| assert!(m.set_muted(7, true)), false));
    let zero = steady_rms(&one_speaker(|m| assert!(m.set_volume(7, 0.0)), false));
    assert!(normal > 0.2, "tone audible: {normal}");
    assert!(db(full, normal).abs() < 0.1, "100 % is unity");
    // A linear slider only takes 6 dB off at 50 %, which is barely audible.
    assert!(
        (db(half, normal) + 18.0).abs() < 0.5,
        "50 %: {} dB",
        db(half, normal)
    );
    assert!(
        (db(quarter, normal) + 36.0).abs() < 1.0,
        "25 %: {} dB",
        db(quarter, normal)
    );
    assert_eq!(muted, 0.0);
    assert_eq!(zero, 0.0);
}

#[test]
fn volume_boosts_a_quiet_speaker_by_18_db_at_200_percent() {
    let normal = steady_rms(&speaker_at(0.04, |_| {}, false));
    let double = steady_rms(&speaker_at(0.04, |m| assert!(m.set_volume(7, 2.0)), false));
    let over = steady_rms(&speaker_at(0.04, |m| assert!(m.set_volume(7, 9.0)), false));
    assert!(
        (db(double, normal) - 18.0).abs() < 0.5,
        "200 %: {} dB",
        db(double, normal)
    );
    assert!((db(over, normal) - 18.0).abs() < 0.5, "clamped to 200 %");
}

#[test]
fn listener_audio_is_discarded_unless_switched_off() {
    assert_eq!(steady_rms(&one_speaker(|_| {}, true)), 0.0);
    let heard = steady_rms(&one_speaker(|m| m.set_discard_listener_audio(false), true));
    assert!(heard > 0.2);
}

#[test]
fn limiter_keeps_loud_mix_below_full_scale() {
    let packets = tone_packets(50, 0);
    let mut mixer = Mixer::new().expect("mixer");
    let speech: Vec<_> = (1..=4)
        .map(|session| {
            assert!(mixer.set_volume(session, 2.0));
            Speech {
                session,
                packets: &packets,
                start_ms: 0,
                via_listener: false,
            }
        })
        .collect();
    let loud_mix_in = [1.5; FRAME_SAMPLES];
    let (out, _) = run(&mut mixer, &speech, Some(&loud_mix_in), 1_200);
    let peak = out.iter().fold(0.0f32, |p, s| p.max(s.abs()));
    assert!(peak <= 1.0, "peak {peak}");
    assert!(steady_rms(&out) > 0.5, "still loud after limiting");
}

#[test]
fn mix_in_is_added_without_speakers() {
    let mut mixer = Mixer::new().expect("mixer");
    let beep = [0.25; FRAME_SAMPLES];
    let mut out = [0.0; FRAME_SAMPLES];
    let mut right = [0.0; FRAME_SAMPLES];
    mixer.mix(&mut out, &mut right, Some(&beep), 0, &mut |_| {});
    assert!(out.iter().all(|s| (s - 0.25).abs() < 1e-6));
    assert_eq!(out, right);
}

#[test]
fn talk_events_follow_bursts_within_budget() {
    let packets = tone_packets(25, 0);
    let mut mixer = Mixer::new().expect("mixer");
    assert!(mixer.set_muted(3, true));
    let speech = [
        Speech {
            session: 3,
            packets: &packets,
            start_ms: 100,
            via_listener: false,
        },
        Speech {
            session: 4,
            packets: &packets,
            start_ms: 300,
            via_listener: true,
        },
    ];
    let (_, events) = run(&mut mixer, &speech, None, 1_500);
    let at = |session, talking| {
        events
            .iter()
            .find(|(_, e)| *e == TalkEvent { session, talking })
            .map(|(t, _)| *t)
            .unwrap_or_else(|| panic!("no event {session} {talking}"))
    };
    // Muted and discarded speakers still show as talking (FR-1.15 indicators).
    assert!(
        at(3, true) - 100 <= 200,
        "start latency {}",
        at(3, true) - 100
    );
    assert!(at(4, true) - 300 <= 200);
    // 500 ms of audio each: stop shortly after the last frame played.
    let stop_latency = at(3, false) - (100 + 500);
    assert!(stop_latency <= 200, "stop latency {stop_latency}");
    assert!(at(4, false) > at(4, true));
    assert_eq!(events.len(), 4);
}

#[test]
fn forget_speaker_frees_slot_with_stop_event() {
    let packets = tone_packets(50, 0);
    let mut mixer = Mixer::new().expect("mixer");
    let mut frame = [0.0; FRAME_SAMPLES];
    let mut right = [0.0; FRAME_SAMPLES];
    let mut events = Vec::new();
    for (i, p) in packets.iter().take(3).enumerate() {
        mixer.push_packet(&packet(9, p, false), i as u64 * 20);
    }
    mixer.mix(&mut frame, &mut right, None, 10, &mut |e| events.push(e));
    mixer.forget_speaker(9);
    mixer.mix(&mut frame, &mut right, None, 20, &mut |e| events.push(e));
    assert_eq!(
        events,
        [
            TalkEvent {
                session: 9,
                talking: true
            },
            TalkEvent {
                session: 9,
                talking: false
            },
        ]
    );
    assert!(mixer.speakers.iter().all(|s| s.session.is_none()));
}

#[test]
fn positional_voices_are_placed_around_the_listener() {
    let packets = tone_packets(50, 0);
    let speech = |session| Speech {
        session,
        packets: &packets,
        start_ms: 0,
        via_listener: false,
    };
    let listener = Arc::new(crate::spatial::Listener::default());
    listener.set_pose([0.0; 3], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]);
    let side = |session: u32, position: [f32; 3], listener: &Arc<_>| {
        let mut mixer = Mixer::new().expect("mixer");
        mixer.set_listener(Arc::clone(listener));
        let (left, right, _) = run_stereo(
            &mut mixer,
            &[speech(session)],
            &[(session, position)],
            None,
            1_200,
        );
        (steady_rms(&left), steady_rms(&right))
    };

    let (l, r) = side(1, [-3.0, 0.0, 0.0], &listener);
    assert!(l > 2.0 * r, "left voice: {l} {r}");
    let (l, r) = side(2, [3.0, 0.0, 0.0], &listener);
    assert!(r > 2.0 * l, "right voice: {l} {r}");
    let (near, _) = side(3, [-2.0, 0.0, 0.0], &listener);
    let (far, _) = side(3, [-10.0, 0.0, 0.0], &listener);
    assert!(near > 2.0 * far, "distance: {near} {far}");

    // Without a listener pose (no game), positions are ignored.
    listener.clear();
    let (l, r) = side(1, [-3.0, 0.0, 0.0], &listener);
    assert!((l - r).abs() < 1e-6, "unplaced: {l} {r}");
}

#[test]
fn speakers_beyond_capacity_are_counted() {
    let packets = tone_packets(5, 0);
    let mut mixer = Mixer::new().expect("mixer");
    for session in 0..MAX_SPEAKERS as u32 + 3 {
        mixer.push_packet(&packet(session, &packets[0], false), 0);
    }
    assert_eq!(mixer.stats().packets_no_slot, 3);
}

/// Consumes queued audio in real time, like a device.
struct RealTimeSink {
    started: Instant,
    written: usize,
    frames: Arc<Mutex<Vec<Frame>>>,
}

impl PlaybackSink for RealTimeSink {
    fn write_frame(&mut self, frame: &Frame) -> bool {
        self.written += FRAME_SAMPLES;
        if let Ok(mut frames) = self.frames.lock() {
            frames.push(*frame);
        }
        true
    }

    fn queued_samples(&mut self) -> usize {
        let played = self.started.elapsed().as_micros() as usize * 48 / 1000;
        self.written.saturating_sub(played)
    }
}

#[test]
fn playback_thread_mixes_paces_and_taps() {
    let packets = tone_packets(20, 0);
    let frames = Arc::new(Mutex::new(Vec::new()));
    let sink_started = Instant::now();
    let sink = RealTimeSink {
        started: sink_started,
        written: 0,
        frames: Arc::clone(&frames),
    };
    let tapped = Arc::new(Mutex::new(0usize));
    let tap_count = Arc::clone(&tapped);
    let config = PlaybackConfig {
        far_end_tap: Some(Box::new(move |_| {
            if let Ok(mut n) = tap_count.lock() {
                *n += 1;
            }
        })),
        ..PlaybackConfig::default()
    };
    let (playback, mut ports) = Playback::start(sink, config).expect("playback");

    // Packets are sent on a 20 ms schedule but kept up to 200 ms ahead of
    // it (well inside the jitter buffer's slots), so a stall of this thread
    // on a loaded machine does not starve the playback thread.
    const LEAD: Duration = Duration::from_millis(200);
    let started = Instant::now();
    for (i, p) in packets.iter().enumerate() {
        let due = started + Duration::from_millis(20 * i as u64);
        if let Some(wait) = due.checked_duration_since(Instant::now() + LEAD) {
            std::thread::sleep(wait);
        }
        let pkt = VoicePacket::new(5, p.seq, &p.payload, p.is_last, false, Instant::now())
            .expect("payload fits");
        assert!(ports.packets.push(pkt));
    }

    // The burst ends (talking: false) once all of it has been played.
    let mut events = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    while events.len() < 2 {
        assert!(Instant::now() < deadline, "burst never ended: {events:?}");
        while let Ok(e) = ports.talk_events.pop() {
            events.push(e);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    // Keeps up with real time: about one frame per 10 ms since the sink
    // started. Re-evaluated until the thread has caught up after a stall.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let expected = sink_started.elapsed().as_millis() as usize / 10;
        let played = frames.lock().expect("frames").len();
        if played + 5 >= expected {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "{played} frames, expected {expected}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    drop(playback);
    let elapsed_ms = sink_started.elapsed().as_millis() as usize;

    let frames = frames.lock().expect("frames");
    let tapped = *tapped.lock().expect("tap");
    assert_eq!(tapped, frames.len(), "tap sees every played frame");
    // Never ahead of real time by more than the 25 ms queue (plus slack).
    assert!(
        frames.len() <= elapsed_ms / 10 + 10,
        "{} frames in {elapsed_ms} ms",
        frames.len()
    );
    let audible = frames.iter().filter(|f| rms(&f[..]) > 0.1).count();
    assert!(audible >= 30, "{audible} audible frames");

    while let Ok(e) = ports.talk_events.pop() {
        events.push(e);
    }
    assert_eq!(
        events,
        [
            TalkEvent {
                session: 5,
                talking: true
            },
            TalkEvent {
                session: 5,
                talking: false
            },
        ]
    );
}

#[test]
fn playback_thread_plays_share_sound_at_its_level() {
    use super::share::{ShareStats, share_channel, share_encoder};

    const AMPLITUDE: f32 = 0.25;
    let frames = Arc::new(Mutex::new(Vec::new()));
    let sink = RealTimeSink {
        started: Instant::now(),
        written: 0,
        frames: Arc::clone(&frames),
    };
    let (playback, mut ports) = Playback::start(sink, PlaybackConfig::default()).expect("playback");
    let stats = Arc::new(ShareStats::default());
    let (mut writer, source) = share_channel(stats.clone()).expect("channel");
    assert!(ports.shares.add(source).is_ok());

    let mut encoder = share_encoder().expect("encoder");
    let mut out = [0u8; crate::codec::MAX_PACKET_BYTES];
    let started = Instant::now();
    for n in 0..100 {
        let mut frame = [0.0; FRAME_SAMPLES];
        for (i, s) in frame.iter_mut().enumerate() {
            let t = (n * FRAME_SAMPLES + i) as f32 / 48_000.0;
            *s = AMPLITUDE * (std::f32::consts::TAU * 440.0 * t).sin();
        }
        if let Some(len) = encoder.push_frame(&frame, &mut out).expect("encode") {
            writer.push_packet(&out[..len]).expect("decode");
        }
        let due = started + Duration::from_millis(10 * (n as u64 + 1));
        if let Some(wait) = due.checked_duration_since(Instant::now()) {
            std::thread::sleep(wait);
        }
    }
    drop(writer);
    let deadline = Instant::now() + Duration::from_secs(5);
    while stats.snapshot().played_samples < 90 * FRAME_SAMPLES as u64 {
        assert!(Instant::now() < deadline, "{:?}", stats.snapshot());
        std::thread::sleep(Duration::from_millis(10));
    }
    std::thread::sleep(Duration::from_millis(100));
    // The ring was played out and handed back for freeing.
    ports.shares.collect();
    drop(playback);

    let frames = frames.lock().expect("frames");
    let expected = AMPLITUDE / std::f32::consts::SQRT_2;
    let audible: Vec<f32> = frames
        .iter()
        .map(|f| rms(&f[..]))
        .filter(|level| *level > 0.05)
        .collect();
    assert!(audible.len() >= 80, "{} audible frames", audible.len());
    let steady = &audible[10..audible.len() - 5];
    let level = steady.iter().sum::<f32>() / steady.len() as f32;
    assert!(
        (level - expected).abs() < expected * 0.1,
        "level {level}, expected {expected}"
    );
}

#[test]
fn jitter_delay_is_reported_while_someone_talks_and_kept_afterwards() {
    let packets = tone_packets(40, 0);
    let mut mixer = Mixer::new().expect("mixer");
    let (mut left, mut right) = ([0.0; FRAME_SAMPLES], [0.0; FRAME_SAMPLES]);
    assert_eq!(mixer.stats().jitter_delay_ms, 0, "nobody talked yet");
    let mut talking_delay = 0;
    for now in (0..1_500).step_by(10) {
        if let Some(p) = packets.get(now as usize / 20).filter(|_| now % 20 == 0) {
            // Every fourth packet arrives 30 ms late: jitter.
            let delay = if (now / 20) % 4 == 3 { 30 } else { 0 };
            mixer.push_packet(&packet(3, p, false), now + 1 + delay);
        }
        mixer.mix(&mut left, &mut right, None, now + 1, &mut |_| {});
        if now == 600 {
            talking_delay = mixer.stats().jitter_delay_ms;
        }
    }
    assert!(
        talking_delay > 20,
        "more than one packet under jitter: {talking_delay}"
    );
    assert!(
        mixer.stats().jitter_delay_ms > 0,
        "the last stream's buffer is still shown between bursts"
    );
}

#[test]
fn idle_silence_and_stream_ends_are_not_concealment() {
    let mut mixer = Mixer::new().expect("mixer");
    let (mut left, mut right) = ([0.0; FRAME_SAMPLES], [0.0; FRAME_SAMPLES]);
    for now in (0..5_000).step_by(10) {
        assert!(!mixer.mix(&mut left, &mut right, None, now, &mut |_| {}));
    }
    assert_eq!(mixer.stats().concealed_frames, 0, "nobody talked");

    // A stream that ends without the last-frame flag (lost, or a client that never sets it),
    // followed by a long silence.
    let mut packets = tone_packets(20, 0);
    if let Some(last) = packets.last_mut() {
        last.is_last = false;
    }
    let (_, events) = run(
        &mut mixer,
        &[Speech {
            session: 4,
            packets: &packets,
            start_ms: 0,
            via_listener: false,
        }],
        None,
        5_000,
    );
    assert_eq!(events.len(), 2, "the stream started and ended: {events:?}");
    let stats = mixer.stats();
    assert_eq!(stats.concealed_frames, 0, "{stats:?}");
    assert_eq!(stats.received_frames, 40);
}

#[test]
fn a_gap_inside_a_stream_is_concealment() {
    let packets = tone_packets(40, 0);
    // Packets 10–12 (60 ms) are lost; the stream goes on afterwards.
    let kept: Vec<_> = packets
        .into_iter()
        .enumerate()
        .filter(|(i, _)| !(10..13).contains(i))
        .map(|(_, p)| p)
        .collect();
    let mut mixer = Mixer::new().expect("mixer");
    let (mut left, mut right) = ([0.0; FRAME_SAMPLES], [0.0; FRAME_SAMPLES]);
    for now in (0..1_500).step_by(10) {
        for p in kept.iter().filter(|p| p.seq * FRAME_MS == now) {
            mixer.push_packet(&packet(5, p, false), now + 1);
        }
        mixer.mix(&mut left, &mut right, None, now + 1, &mut |_| {});
    }
    let stats = mixer.stats();
    assert!(stats.concealed_frames >= 4, "{stats:?}");
    assert_eq!(stats.received_frames, 74);
}

/// A device queue the test drains at will, like a device whose feeder thread ran late.
struct DrainableSink {
    queued: Arc<std::sync::atomic::AtomicUsize>,
}

impl PlaybackSink for DrainableSink {
    fn write_frame(&mut self, _: &Frame) -> bool {
        self.queued
            .fetch_add(FRAME_SAMPLES, std::sync::atomic::Ordering::Relaxed);
        true
    }

    fn queued_samples(&mut self) -> usize {
        self.queued.load(std::sync::atomic::Ordering::Relaxed)
    }
}

struct DropFlag(Arc<std::sync::atomic::AtomicBool>);

impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

#[test]
fn playback_thread_counts_late_wakeups_and_keeps_its_setup() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    let queued = Arc::new(AtomicUsize::new(0));
    let setup_dropped = Arc::new(AtomicBool::new(false));
    let setup_thread = Arc::new(Mutex::new(None));
    let config = PlaybackConfig {
        on_thread_start: Some({
            let (dropped, thread) = (Arc::clone(&setup_dropped), Arc::clone(&setup_thread));
            Box::new(move || {
                *thread.lock().expect("thread") = std::thread::current().name().map(str::to_owned);
                Box::new(DropFlag(dropped))
            })
        }),
        ..PlaybackConfig::default()
    };
    let sink = DrainableSink {
        queued: Arc::clone(&queued),
    };
    let (playback, mut ports) = Playback::start(sink, config).expect("playback");
    let wait_until = |what: &str, done: &dyn Fn() -> bool| {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !done() {
            assert!(Instant::now() < deadline, "timed out: {what}");
            std::thread::sleep(Duration::from_millis(2));
        }
    };
    let filled = || queued.load(Ordering::Relaxed) >= 1200;
    wait_until("queue filled", &filled);
    assert_eq!(playback.stats().late_wakeups, 0, "a full queue is on time");
    // The device played all the queued silence before the thread woke up again: not heard.
    queued.store(0, Ordering::Relaxed);
    wait_until("queue refilled", &filled);
    assert_eq!(
        playback.stats().late_wakeups,
        0,
        "running late over silence is not heard"
    );
    // Now with sound queued, the device runs dry part-way through it.
    for _ in 0..4_800 {
        ports.mix_in.push(0.5).expect("mix-in");
    }
    queued.store(0, Ordering::Relaxed);
    wait_until("sound queued", &filled);
    queued.store(0, Ordering::Relaxed);
    wait_until("late wake-up counted", &|| {
        playback.stats().late_wakeups >= 1
    });
    assert!(playback.stats().mix_time_max_us >= playback.stats().mix_time_avg_us);
    assert_eq!(
        setup_thread.lock().expect("thread").as_deref(),
        Some("mumcord-playback")
    );
    assert!(
        !setup_dropped.load(Ordering::Relaxed),
        "setup lives with the thread"
    );
    drop(playback);
    assert!(
        setup_dropped.load(Ordering::Relaxed),
        "setup ends with the thread"
    );
}

#[test]
fn low_latency_reaches_every_jitter_buffer() {
    let mut mixer = Mixer::new().expect("mixer");
    mixer.set_low_latency(true);
    assert!(mixer.speakers.iter().all(|s| s.jitter.low_latency()));
    mixer.set_low_latency(false);
    assert!(mixer.speakers.iter().all(|s| !s.jitter.low_latency()));
}

/// Records the queue level the playback thread saw before each write (issue #156).
struct QueueProbe {
    started: Instant,
    written: usize,
    levels: Arc<Mutex<Vec<usize>>>,
}

impl PlaybackSink for QueueProbe {
    fn write_frame(&mut self, _frame: &Frame) -> bool {
        let level = self.queued_samples();
        if let Ok(mut levels) = self.levels.lock() {
            levels.push(level);
        }
        self.written += FRAME_SAMPLES;
        true
    }

    fn queued_samples(&mut self) -> usize {
        let played = self.started.elapsed().as_micros() as usize * 48 / 1000;
        self.written.saturating_sub(played)
    }
}

#[test]
fn queue_target_can_be_lowered_while_playing() {
    let levels = Arc::new(Mutex::new(Vec::new()));
    let sink = QueueProbe {
        started: Instant::now(),
        written: 0,
        levels: Arc::clone(&levels),
    };
    let (mut playback, _ports) = Playback::start(sink, PlaybackConfig::default()).expect("start");
    std::thread::sleep(Duration::from_millis(300));
    let before = std::mem::take(&mut *levels.lock().expect("levels"));
    assert!(playback.set_queue_target_ms(LOW_LATENCY_QUEUE_MS));
    // Let the command land and the queue drain to the new target.
    std::thread::sleep(Duration::from_millis(100));
    levels.lock().expect("levels").clear();
    std::thread::sleep(Duration::from_millis(300));
    let after = levels.lock().expect("levels").clone();
    drop(playback);

    let samples = |ms: u32| ms as usize * 48;
    assert!(
        before.iter().any(|&l| l > samples(LOW_LATENCY_QUEUE_MS)),
        "{before:?}"
    );
    assert!(!after.is_empty());
    // A frame is only written while the queue is below the target.
    assert!(
        after.iter().all(|&l| l < samples(LOW_LATENCY_QUEUE_MS)),
        "{after:?}"
    );
}
