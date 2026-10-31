use super::*;
use crate::codec::{OpusEncoder, PacketDuration};

/// A 20 ms Opus packet as a speaker would send it.
pub(crate) struct TestPacket {
    pub seq: u64,
    pub payload: Vec<u8>,
    pub is_last: bool,
}

/// `count` 20 ms packets of a 440 Hz tone, sequence numbers in 10 ms frames.
pub(crate) fn tone_packets(count: usize, first_seq: u64) -> Vec<TestPacket> {
    tone_packets_at(0.4, count, first_seq)
}

/// [`tone_packets`] with the tone's peak `amplitude`.
pub(crate) fn tone_packets_at(amplitude: f32, count: usize, first_seq: u64) -> Vec<TestPacket> {
    let mut encoder = OpusEncoder::new(32_000, PacketDuration::Ms20).expect("encoder");
    encoder.set_packet_loss_percent(10).expect("loss hint");
    let mut out = [0u8; MAX_PACKET_BYTES];
    let mut packets = Vec::new();
    let mut frame = [0.0; FRAME_SAMPLES];
    let mut t = 0usize;
    while packets.len() < count {
        for s in frame.iter_mut() {
            *s = amplitude
                * (2.0 * std::f32::consts::PI * 440.0 * t as f32 / SAMPLE_RATE as f32).sin();
            t += 1;
        }
        if let Some(len) = encoder.push_frame(&frame, &mut out).expect("encode") {
            packets.push(TestPacket {
                seq: first_seq + 2 * packets.len() as u64,
                payload: out[..len].to_vec(),
                is_last: false,
            });
        }
    }
    if let Some(last) = packets.last_mut() {
        last.is_last = true;
    }
    packets
}

/// Deterministic pseudo-random numbers in `0..bound`.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self, bound: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        (self.0 >> 33) % bound
    }
}

#[derive(Default)]
struct Run {
    pulls: Vec<Pull>,
    /// Target delay sampled every pull.
    targets: Vec<u64>,
    /// Buffered audio sampled every pull.
    buffered: Vec<u64>,
}

impl Run {
    fn first_audio_ms(&self) -> Option<u64> {
        self.pulls
            .iter()
            .position(|p| *p == Pull::Audio)
            .map(|i| i as u64 * FRAME_MS)
    }

    fn count(&self, kind: Pull) -> usize {
        self.pulls.iter().filter(|p| **p == kind).count()
    }
}

/// Feeds packets at their arrival times (ms) and pulls a frame every 10 ms
/// until `end_ms`. Packets with arrival `None` are lost.
fn simulate(jb: &mut JitterBuffer, packets: &[(Option<u64>, &TestPacket)], end_ms: u64) -> Run {
    let mut order: Vec<_> = packets
        .iter()
        .filter_map(|(arrival, p)| arrival.map(|a| (a, *p)))
        .collect();
    order.sort_by_key(|(a, _)| *a);
    let mut next = 0;
    let mut run = Run::default();
    let mut frame = [0.0; FRAME_SAMPLES];
    let mut now = 0;
    while now < end_ms {
        while next < order.len() && order[next].0 <= now {
            let (arrival, p) = order[next];
            jb.push(p.seq, &p.payload, p.is_last, arrival);
            next += 1;
        }
        run.pulls.push(jb.pull(&mut frame, now));
        run.targets.push(jb.target_delay_ms());
        run.buffered.push(jb.buffered_ms());
        now += FRAME_MS;
    }
    run
}

fn clean_schedule(packets: &[TestPacket], offset_ms: u64) -> Vec<(Option<u64>, &TestPacket)> {
    packets
        .iter()
        .map(|p| (Some(p.seq * FRAME_MS + offset_ms), p))
        .collect()
}

#[test]
fn clean_stream_plays_with_one_packet_of_delay() {
    let packets = tone_packets(100, 0);
    let mut jb = JitterBuffer::new().expect("jitter buffer");
    let run = simulate(&mut jb, &clean_schedule(&packets, 3), 2_500);

    // First packet arrives at 3 ms and plays on the very next tick.
    assert_eq!(run.first_audio_ms(), Some(10));
    assert_eq!(run.targets.iter().max(), Some(&20));
    assert_eq!(run.count(Pull::Concealed), 0);
    assert_eq!(run.count(Pull::Audio), 200);
    assert_eq!(jb.stats().late_packets, 0);
    assert!(jb.is_idle(), "last-frame flag ends the burst");
}

#[test]
fn delay_grows_under_jitter_and_shrinks_back() {
    let packets = tone_packets(1_000, 0);
    let mut rng = Lcg(7);
    // 8 s of up to 40 ms jitter (with reordering), then 12 s clean.
    let schedule: Vec<_> = packets
        .iter()
        .map(|p| {
            let base = p.seq * FRAME_MS + 3;
            let jitter = if base < 8_000 { rng.next(41) } else { 0 };
            (Some(base + jitter), p)
        })
        .collect();
    let mut jb = JitterBuffer::new().expect("jitter buffer");
    let run = simulate(&mut jb, &schedule, 20_100);

    let jittery_target = run.targets[700];
    assert!(
        (50..=80).contains(&jittery_target),
        "target {jittery_target} ms under 40 ms jitter"
    );
    let after_warmup = &run.pulls[100..800];
    let concealed = after_warmup
        .iter()
        .filter(|p| **p == Pull::Concealed)
        .count();
    assert!(concealed <= 7, "{concealed} concealed frames after warm-up");
    assert_eq!(run.targets[1_990], 20, "target shrinks back once clean");
    assert!(
        jb.buffered_ms() <= 40,
        "buffer shrank to {} ms",
        jb.buffered_ms()
    );
    assert!(jb.stats().dropped_frames > 0);
}

#[test]
fn reordered_packets_are_all_played() {
    let packets = tone_packets(100, 1_000);
    // Every pair arrives swapped: packet n+1 15 ms before packet n.
    let schedule: Vec<_> = packets
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let base = (p.seq - 1_000) * FRAME_MS + 2;
            let arrival = if i % 2 == 0 { base + 25 } else { base };
            (Some(arrival), p)
        })
        .collect();
    let mut jb = JitterBuffer::new().expect("jitter buffer");
    let run = simulate(&mut jb, &schedule, 3_000);

    assert_eq!(jb.stats().late_packets, 0);
    assert_eq!(jb.stats().concealed_frames + jb.stats().fec_frames, 0);
    assert_eq!(run.count(Pull::Audio), 200);
}

#[test]
fn lost_packets_are_recovered_or_concealed() {
    let packets = tone_packets(200, 0);
    let schedule: Vec<_> = clean_schedule(&packets, 3)
        .into_iter()
        .enumerate()
        .map(|(i, (a, p))| (if i % 10 == 5 { None } else { a }, p))
        .collect();
    let mut jb = JitterBuffer::new().expect("jitter buffer");
    let run = simulate(&mut jb, &schedule, 4_500);

    let stats = jb.stats();
    // The first loss is bridged by concealment while it could still be a late
    // packet (growing the buffer by 20 ms); with that headroom every later
    // loss is rebuilt from the next packet's FEC.
    assert!(stats.concealed_frames <= 2, "{stats:?}");
    assert!(stats.fec_frames >= 38, "FEC used: {stats:?}");
    // Every 10 ms of the burst is still played: no gaps.
    let played = run.count(Pull::Audio) + run.count(Pull::Concealed);
    assert_eq!(
        played as u64,
        400 + stats.concealed_frames - stats.dropped_frames
    );
}

#[test]
fn burst_ends_by_timeout_when_last_packet_is_lost() {
    let mut packets = tone_packets(20, 0);
    packets.pop();
    let mut jb = JitterBuffer::new().expect("jitter buffer");
    let run = simulate(&mut jb, &clean_schedule(&packets, 3), 1_000);

    assert!(jb.is_idle());
    let last_active = run.pulls.iter().rposition(|p| *p != Pull::Silent);
    let last_active_ms = last_active.map(|i| i as u64 * FRAME_MS).unwrap_or_default();
    // Audio ends at 380 ms; the burst must end within the timeout after that.
    assert!(
        (380..=380 + 20 + END_TIMEOUT_MS + FRAME_MS).contains(&last_active_ms),
        "burst ended at {last_active_ms} ms"
    );
}

#[test]
fn late_packet_is_dropped_and_counted() {
    let packets = tone_packets(40, 0);
    let mut jb = JitterBuffer::new().expect("jitter buffer");
    let mut schedule = clean_schedule(&packets, 3);
    // Packet 2 arrives 200 ms late, after its slot was played.
    schedule[2].0 = Some(240);
    simulate(&mut jb, &schedule, 1_000);
    assert_eq!(jb.stats().late_packets, 1);
}

#[test]
fn next_burst_starts_fresh_after_last_frame() {
    let first = tone_packets(10, 0);
    let second = tone_packets(10, 100);
    let mut schedule = clean_schedule(&first, 3);
    schedule.extend(clean_schedule(&second, 3));
    let mut jb = JitterBuffer::new().expect("jitter buffer");
    let run = simulate(&mut jb, &schedule, 1_500);
    assert_eq!(run.count(Pull::Audio), 40);
    assert_eq!(run.count(Pull::Concealed), 0);
}

#[test]
fn invalid_packets_are_ignored() {
    let mut jb = JitterBuffer::new().expect("jitter buffer");
    // TOC byte with code 3 but no frame count byte.
    jb.push(0, &[0xFF], false, 0);
    jb.push(1, &[0; MAX_PACKET_BYTES + 1], false, 0);
    assert_eq!(jb.stats().invalid_packets, 2);
    assert!(jb.is_idle());
}

#[test]
fn stream_end_without_last_flag_is_not_concealment() {
    let mut packets = tone_packets(20, 0);
    if let Some(last) = packets.last_mut() {
        last.is_last = false;
    }
    let mut jb = JitterBuffer::new().expect("jitter buffer");
    simulate(&mut jb, &clean_schedule(&packets, 3), 1_000);
    assert!(jb.is_idle());
    let stats = jb.stats();
    assert_eq!(
        (stats.concealed_frames, stats.fec_frames),
        (0, 0),
        "{stats:?}"
    );
    assert_eq!(stats.decoded_frames, 40);
}

#[test]
fn late_packet_after_a_bridge_counts_the_bridge() {
    let packets = tone_packets(40, 0);
    let mut schedule = clean_schedule(&packets, 3);
    // Packet 10 arrives 40 ms late but before the burst times out: the bridge covered a gap.
    schedule[10].0 = schedule[10].0.map(|a| a + 40);
    for entry in schedule.iter_mut().skip(11) {
        entry.0 = entry.0.map(|a| a + 40);
    }
    let mut jb = JitterBuffer::new().expect("jitter buffer");
    simulate(&mut jb, &schedule, 1_500);
    assert!(jb.stats().concealed_frames >= 1, "{:?}", jb.stats());
}

/// `count` 10 ms low-delay packets of a 440 Hz tone, as low-latency mode sends them (issue #156).
fn low_delay_tone_packets(count: usize) -> Vec<TestPacket> {
    let mut encoder = OpusEncoder::new_low_delay(64_000, PacketDuration::Ms10).expect("encoder");
    let mut out = [0u8; MAX_PACKET_BYTES];
    let mut frame = [0.0; FRAME_SAMPLES];
    let mut packets: Vec<TestPacket> = (0..count as u64)
        .map(|seq| {
            for (i, s) in frame.iter_mut().enumerate() {
                let t = seq as usize * FRAME_SAMPLES + i;
                *s = 0.4
                    * (2.0 * std::f32::consts::PI * 440.0 * t as f32 / SAMPLE_RATE as f32).sin();
            }
            let len = encoder
                .push_frame(&frame, &mut out)
                .expect("encode")
                .expect("one packet per frame");
            TestPacket {
                seq,
                payload: out[..len].to_vec(),
                is_last: false,
            }
        })
        .collect();
    if let Some(last) = packets.last_mut() {
        last.is_last = true;
    }
    packets
}

#[test]
fn low_latency_clean_stream_waits_one_10ms_packet() {
    let packets = low_delay_tone_packets(200);
    let mut jb = JitterBuffer::new().expect("jitter buffer");
    jb.set_low_latency(true);
    let run = simulate(&mut jb, &clean_schedule(&packets, 3), 2_500);

    assert_eq!(run.first_audio_ms(), Some(10));
    // Before the first packet the buffer assumes 20 ms packets.
    assert_eq!(run.targets[1..].iter().max(), Some(&10));
    assert_eq!(run.count(Pull::Concealed), 0);
    assert_eq!(run.count(Pull::Audio), 200);
}

#[test]
fn low_latency_caps_the_target_and_recovers_from_a_stall() {
    let packets = low_delay_tone_packets(400);
    // A clean stream with one 80 ms network stall: packets 100–108 all arrive at 1090 ms.
    let schedule: Vec<_> = packets
        .iter()
        .map(|p| {
            let base = p.seq * FRAME_MS + 3;
            let arrival = if (100..=108).contains(&p.seq) {
                1_090
            } else {
                base
            };
            (Some(arrival), p)
        })
        .collect();
    let mut normal = JitterBuffer::new().expect("jitter buffer");
    let normal_run = simulate(&mut normal, &schedule, 4_000);
    let mut low = JitterBuffer::new().expect("jitter buffer");
    low.set_low_latency(true);
    let low_run = simulate(&mut low, &schedule, 4_000);

    assert!(
        normal_run.targets[150] >= 80,
        "normal mode grows with the stall"
    );
    assert!(
        low_run.targets[1..]
            .iter()
            .all(|&t| t <= 10 + LOW_LATENCY_MAX_EXTRA_MS),
        "low-latency target stays at one packet plus the cap"
    );
    // One second after the stall normal mode still plays the music late; low-latency mode has
    // dropped frames to get back to its target.
    let (normal_ms, low_ms) = (normal_run.buffered[210], low_run.buffered[210]);
    assert!(normal_ms >= 60, "normal mode buffers {normal_ms} ms");
    assert!(low_ms <= 20, "low-latency mode buffers {low_ms} ms");
    assert!(low.stats().dropped_frames > normal.stats().dropped_frames);
    // Concealment stays on and the stream keeps playing.
    assert!(low.stats().concealed_frames > 0);
    assert!(low_run.count(Pull::Audio) > 350);
}

#[test]
fn switching_low_latency_on_applies_to_a_running_stream() {
    let packets = tone_packets(400, 0);
    let mut rng = Lcg(3);
    let schedule: Vec<_> = packets
        .iter()
        .map(|p| (Some(p.seq * FRAME_MS + 3 + rng.next(31)), p))
        .collect();
    let mut jb = JitterBuffer::new().expect("jitter buffer");
    let first = simulate(&mut jb, &schedule[..100], 2_000);
    assert!(first.targets.iter().any(|&t| t > 30));
    jb.set_low_latency(true);
    assert!(jb.low_latency());
    // 20 ms packets from a normal-mode sender: one packet plus the cap.
    assert_eq!(jb.target_delay_ms(), 20 + LOW_LATENCY_MAX_EXTRA_MS);
    jb.reset();
    assert!(
        jb.low_latency(),
        "a mode survives handing the buffer to another speaker"
    );
}
