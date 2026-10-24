use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::*;
use crate::jitter::tests::{TestPacket, tone_packets};

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
/// `end_ms`. Returns the mixed output and the talk events with their time.
fn run(
    mixer: &mut Mixer,
    speech: &[Speech<'_>],
    mix_in: Option<&Frame>,
    end_ms: u64,
) -> (Vec<f32>, Vec<(u64, TalkEvent)>) {
    let mut out = Vec::new();
    let mut events = Vec::new();
    let mut frame = [0.0; FRAME_SAMPLES];
    let mut now = 0;
    while now < end_ms {
        for s in speech {
            let first_seq = s.packets[0].seq;
            for p in s.packets {
                let arrival = s.start_ms + (p.seq - first_seq) * FRAME_MS + 1;
                if arrival + FRAME_MS > now && arrival <= now {
                    mixer.push_packet(&packet(s.session, p, s.via_listener), arrival);
                }
            }
        }
        mixer.mix(&mut frame, mix_in, now, &mut |e| events.push((now, e)));
        out.extend_from_slice(&frame);
        now += FRAME_MS;
    }
    (out, events)
}

fn rms(samples: &[f32]) -> f32 {
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

/// RMS of the steady middle of a one-second burst starting at 0 ms.
fn steady_rms(out: &[f32]) -> f32 {
    rms(&out[200 * 48..800 * 48])
}

fn one_speaker(configure: impl FnOnce(&mut Mixer), via_listener: bool) -> Vec<f32> {
    let packets = tone_packets(50, 0);
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

#[test]
fn volume_scales_and_mute_silences() {
    let normal = steady_rms(&one_speaker(|_| {}, false));
    let double = steady_rms(&one_speaker(|m| assert!(m.set_volume(7, 2.0)), false));
    let half = steady_rms(&one_speaker(|m| assert!(m.set_volume(7, 0.5)), false));
    let muted = steady_rms(&one_speaker(|m| assert!(m.set_muted(7, true)), false));
    let zero = steady_rms(&one_speaker(|m| assert!(m.set_volume(7, 0.0)), false));
    assert!(normal > 0.2, "tone audible: {normal}");
    assert!(
        (double / normal - 2.0).abs() < 0.05,
        "200 %: {}",
        double / normal
    );
    assert!(
        (half / normal - 0.5).abs() < 0.02,
        "50 %: {}",
        half / normal
    );
    assert_eq!(muted, 0.0);
    assert_eq!(zero, 0.0);
}

#[test]
fn volume_is_clamped_to_200_percent() {
    let normal = steady_rms(&one_speaker(|_| {}, false));
    let over = steady_rms(&one_speaker(|m| assert!(m.set_volume(7, 9.0)), false));
    assert!((over / normal - 2.0).abs() < 0.05);
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
    mixer.mix(&mut out, Some(&beep), 0, &mut |_| {});
    assert!(out.iter().all(|s| (s - 0.25).abs() < 1e-6));
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
    let mut events = Vec::new();
    for (i, p) in packets.iter().take(3).enumerate() {
        mixer.push_packet(&packet(9, p, false), i as u64 * 20);
    }
    mixer.mix(&mut frame, None, 10, &mut |e| events.push(e));
    mixer.forget_speaker(9);
    mixer.mix(&mut frame, None, 20, &mut |e| events.push(e));
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
    let frames = Arc::new(Mutex::new(Vec::new()));
    let sink = RealTimeSink {
        started: Instant::now(),
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

    let started = Instant::now();
    for p in tone_packets(20, 0) {
        let pkt = VoicePacket::new(5, p.seq, &p.payload, p.is_last, false, Instant::now())
            .expect("payload fits");
        assert!(ports.packets.push(pkt));
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(Duration::from_millis(300));
    let elapsed_ms = started.elapsed().as_millis() as usize;
    drop(playback);

    let frames = frames.lock().expect("frames");
    let tapped = *tapped.lock().expect("tap");
    assert_eq!(tapped, frames.len(), "tap sees every played frame");
    // Paced in real time: about one frame per 10 ms plus the 25 ms queue.
    let expected = elapsed_ms / 10;
    assert!(
        frames.len() + 5 >= expected && frames.len() <= expected + 10,
        "{} frames in {elapsed_ms} ms",
        frames.len()
    );
    let audible = frames.iter().filter(|f| rms(&f[..]) > 0.1).count();
    assert!(audible >= 30, "{audible} audible frames");

    let mut events = Vec::new();
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
