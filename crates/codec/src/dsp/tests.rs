use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::f32::consts::PI;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::gate::{PTT_TAIL_FRAMES, PttGate, VAD_HANGOVER_FRAMES, VoiceDetector};
use super::*;

/// Counts allocations per thread so the no-allocation check is not
/// disturbed by tests running in parallel.
struct CountingAlloc;

thread_local! {
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

// SAFETY: delegates to the system allocator; the counter is a const-initialised
// thread-local `Cell`, which never allocates.
unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.with(|a| a.set(a.get() + 1));
        // SAFETY: same contract as the caller's.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: same contract as the caller's.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.with(|a| a.set(a.get() + 1));
        // SAFETY: same contract as the caller's.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: CountingAlloc = CountingAlloc;

fn allocations() -> usize {
    ALLOCATIONS.with(Cell::get)
}

/// Deterministic white noise in [-1, 1].
struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.0 >> 8) as f32 / (1 << 23) as f32 * 2.0 - 1.0
    }
}

/// Voiced, syllable-like signal: a 140 Hz harmonic series with a falling
/// spectrum, gliding pitch and 4 Hz on/off envelope.
fn speech_like(len: usize, amplitude: f32) -> Vec<f32> {
    let mut phase = 0.0f32;
    (0..len)
        .map(|i| {
            let t = i as f32 / SAMPLE_RATE as f32;
            let f0 = 140.0 + 20.0 * (2.0 * PI * 0.7 * t).sin();
            phase += 2.0 * PI * f0 / SAMPLE_RATE as f32;
            let voiced: f32 = (1..=20)
                .map(|h| (h as f32 * phase).sin() / h as f32)
                .sum::<f32>()
                / 2.0;
            let envelope = (2.0 * PI * 4.0 * t).sin().max(0.0).sqrt();
            amplitude * envelope * voiced
        })
        .collect()
}

/// Fan-like noise: low-passed white noise plus a little mains hum.
fn fan_noise(len: usize, amplitude: f32) -> Vec<f32> {
    let mut noise = Noise(7);
    let mut lp = 0.0f32;
    (0..len)
        .map(|i| {
            lp += 0.2 * (noise.next() - lp);
            let hum = 0.2 * (2.0 * PI * 100.0 * i as f32 / SAMPLE_RATE as f32).sin();
            amplitude * (2.0 * lp + hum)
        })
        .collect()
}

fn energy(samples: &[f32]) -> f32 {
    samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32
}

fn db(ratio: f32) -> f32 {
    10.0 * ratio.log10()
}

fn chain_with(aec: bool, ns: bool, agc: bool) -> (DspChain, EchoRefProducer) {
    let controls = Arc::new(DspControls::default());
    controls.set_echo_cancellation(aec);
    controls.set_noise_suppression(ns);
    controls.set_auto_gain(agc);
    let (producer, consumer) = echo_reference();
    (DspChain::new(controls, consumer), producer)
}

/// Runs `input` through the chain frame by frame and returns the output.
fn run(chain: &mut DspChain, input: &[f32]) -> Vec<f32> {
    let mut out = Vec::with_capacity(input.len());
    let mut frame_out = [0.0; FRAME_SAMPLES];
    for frame in input.as_chunks::<FRAME_SAMPLES>().0 {
        chain.process(frame, &mut frame_out);
        out.extend_from_slice(&frame_out);
    }
    out
}

fn last_seconds(samples: &[f32], seconds: f32) -> &[f32] {
    &samples[samples.len() - (seconds * SAMPLE_RATE as f32) as usize..]
}

#[test]
fn echo_cancellation_reduces_synthetic_echo() {
    let (mut chain, mut reference) = chain_with(true, false, false);
    let frames = 600;
    let delay = 2 * FRAME_SAMPLES + 77; // ≈ 21.6 ms acoustic + device delay
    let mut noise = Noise(42);
    let far: Vec<f32> = (0..frames * FRAME_SAMPLES)
        .map(|_| 0.3 * noise.next())
        .collect();
    let mic: Vec<f32> = (0..far.len())
        .map(|i| {
            if i >= delay {
                0.5 * far[i - delay]
            } else {
                0.0
            }
        })
        .collect();

    let mut out = Vec::with_capacity(mic.len());
    let mut frame_out = [0.0; FRAME_SAMPLES];
    for (far, mic) in far
        .as_chunks::<FRAME_SAMPLES>()
        .0
        .iter()
        .zip(mic.as_chunks::<FRAME_SAMPLES>().0)
    {
        assert!(reference.push(far));
        chain.process(mic, &mut frame_out);
        out.extend_from_slice(&frame_out);
    }
    let reduction = db(energy(last_seconds(&mic, 1.0)) / energy(last_seconds(&out, 1.0)));
    println!("echo reduction after 5 s: {reduction:.1} dB");
    assert!(reduction >= 10.0, "echo only reduced by {reduction:.1} dB");
}

#[test]
fn noise_suppression_reduces_noise_wav_and_keeps_speech() {
    let dir = std::env::temp_dir().join(format!("mumcord-dsp-ns-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("noise.wav");
    write_wav(&path, &fan_noise(3 * SAMPLE_RATE as usize, 0.1));
    let noise = read_wav(&path);
    std::fs::remove_dir_all(&dir).expect("cleanup");

    let (mut chain, _) = chain_with(false, true, false);
    let out = run(&mut chain, &noise);
    let noise_reduction = db(energy(last_seconds(&noise, 2.0)) / energy(last_seconds(&out, 2.0)));

    let speech = speech_like(3 * SAMPLE_RATE as usize, 0.3);
    let (mut chain, _) = chain_with(false, true, false);
    let out = run(&mut chain, &speech);
    let speech_loss = db(energy(last_seconds(&speech, 2.0)) / energy(last_seconds(&out, 2.0)));

    println!("noise reduced by {noise_reduction:.1} dB, speech-like signal by {speech_loss:.1} dB");
    assert!(
        noise_reduction >= 10.0,
        "noise only reduced by {noise_reduction:.1} dB"
    );
    assert!(speech_loss <= 6.0, "speech reduced by {speech_loss:.1} dB");
}

#[test]
fn auto_gain_raises_quiet_signal() {
    let quiet = speech_like(8 * SAMPLE_RATE as usize, 0.01);
    let (mut chain, _) = chain_with(false, false, true);
    let out = run(&mut chain, &quiet);
    let gain = db(energy(last_seconds(&out, 2.0)) / energy(last_seconds(&quiet, 2.0)));
    println!("auto gain after 8 s: {gain:.1} dB");
    assert!(gain >= 10.0, "quiet signal only raised by {gain:.1} dB");
}

#[test]
fn all_stages_bypassed_is_bit_exact() {
    let mut input = speech_like(SAMPLE_RATE as usize, 0.5);
    let mut noise = Noise(3);
    for s in &mut input {
        *s += 0.05 * noise.next();
    }
    let (mut chain, mut reference) = chain_with(false, false, false);
    reference.push(&[0.25; FRAME_SAMPLES]);
    assert_eq!(run(&mut chain, &input), input);
}

#[test]
fn each_stage_switches_off_at_runtime() {
    let input = speech_like(SAMPLE_RATE as usize, 0.3);
    let (mut chain, _) = chain_with(true, true, true);
    run(&mut chain, &input);
    let controls = chain.controls().clone();
    for stage in 0..3 {
        // Turn off one stage after another; once all are off the chain must
        // pass audio through untouched even though it ran before.
        match stage {
            0 => controls.set_echo_cancellation(false),
            1 => controls.set_noise_suppression(false),
            _ => controls.set_auto_gain(false),
        }
        let out = run(&mut chain, &input);
        assert_eq!(
            out == input,
            stage == 2,
            "after switching off stage {stage}"
        );
    }
}

#[test]
fn voice_detector_opens_holds_and_hangs_over() {
    let mut vad = VoiceDetector::default();
    let loud = 0.1;
    assert!(!vad.update(0.5, loud), "below open threshold");
    assert!(!vad.update(0.9, 0.0005), "too quiet to be speech");
    assert!(vad.update(0.9, loud), "speech opens");
    assert!(vad.update(0.4, loud), "hold threshold keeps it open");
    for frame in 0..VAD_HANGOVER_FRAMES {
        assert!(vad.update(0.0, 0.0), "hangover frame {frame}");
    }
    assert!(!vad.update(0.0, 0.0), "closes after hangover");
    assert!(!vad.update(0.4, loud), "hold threshold does not reopen");
}

#[test]
fn ptt_gate_follows_flag_with_tail() {
    let mut ptt = PttGate::default();
    assert!(!ptt.update(false));
    assert!(ptt.update(true));
    for frame in 0..PTT_TAIL_FRAMES {
        assert!(ptt.update(false), "tail frame {frame}");
    }
    assert!(!ptt.update(false));
}

#[test]
fn chain_gates_by_mode() {
    let (mut chain, _) = chain_with(false, false, false);
    let controls = chain.controls().clone();
    let speech = speech_like(SAMPLE_RATE as usize, 0.3);
    let frame: &Frame = speech[2400..2880].try_into().expect("frame");
    let mut out = [0.0; FRAME_SAMPLES];

    controls.set_mode(TalkMode::PushToTalk);
    assert_eq!(chain.process(frame, &mut out), Gate::Closed);
    assert!(!controls.transmitting());
    controls.set_ptt_pressed(true);
    assert_eq!(chain.process(frame, &mut out), Gate::Open);
    assert!(controls.transmitting());
    controls.set_ptt_pressed(false);
    for _ in 0..PTT_TAIL_FRAMES {
        assert_eq!(chain.process(frame, &mut out), Gate::Open);
    }
    assert_eq!(chain.process(frame, &mut out), Gate::Closed);

    // Voice activation on silence stays closed; the meter reads the floor.
    controls.set_mode(TalkMode::VoiceActivation);
    // The speech frames above opened the detector; silence closes it once
    // the hangover has run out.
    let mut gate = Gate::Open;
    for _ in 0..=VAD_HANGOVER_FRAMES {
        gate = chain.process(&[0.0; FRAME_SAMPLES], &mut out);
    }
    assert_eq!(gate, Gate::Closed);
    assert_eq!(controls.level().peak_dbfs, Level::FLOOR_DBFS);
}

#[test]
fn voice_activation_opens_on_speech_and_closes_after_hangover() {
    let (mut chain, _) = chain_with(false, true, false);
    let controls = chain.controls().clone();
    let mut noise = Noise(9);
    let speech = speech_like(2 * SAMPLE_RATE as usize, 0.3);
    let mut out = [0.0; FRAME_SAMPLES];
    let mut opened = 0;
    for frame in speech.as_chunks::<FRAME_SAMPLES>().0 {
        if chain.process(frame, &mut out) == Gate::Open {
            opened += 1;
        }
    }
    println!("VAD open for {opened} of 200 speech-like frames");
    assert!(opened >= 100, "VAD open only {opened} of 200 frames");

    let mut closed_after = None;
    for frame in 0..200 {
        let quiet: Frame = std::array::from_fn(|_| 0.0005 * noise.next());
        if chain.process(&quiet, &mut out) == Gate::Closed {
            closed_after = Some(frame);
            break;
        }
    }
    let closed_after = closed_after.expect("gate never closed");
    assert!(closed_after <= VAD_HANGOVER_FRAMES as usize + 1);
    assert!(!controls.transmitting());
}

#[test]
fn level_meter_reports_peak_and_rms() {
    let (mut chain, _) = chain_with(false, false, false);
    let frame: Frame = std::array::from_fn(|i| if i % 2 == 0 { 0.5 } else { -0.5 });
    chain.process(&frame, &mut [0.0; FRAME_SAMPLES]);
    let level = chain.controls().level();
    assert!((level.peak_dbfs - -6.02).abs() < 0.01, "{level:?}");
    assert!((level.rms_dbfs - -6.02).abs() < 0.01, "{level:?}");
}

/// Steady-state allocations per frame with the given stages on.
fn allocations_per_frame(aec: bool, ns: bool, agc: bool) -> f32 {
    let (mut chain, mut reference) = chain_with(aec, ns, agc);
    let speech = speech_like(4 * SAMPLE_RATE as usize, 0.3);
    let mut out = [0.0; FRAME_SAMPLES];
    let mut before = 0;
    for (index, frame) in speech.as_chunks::<FRAME_SAMPLES>().0.iter().enumerate() {
        // The first second is warm-up: lazily sized buffers settle.
        if index == 100 {
            before = allocations();
        }
        reference.push(frame);
        chain.process(frame, &mut out);
    }
    (allocations() - before) as f32 / 300.0
}

/// Mumcord's own per-frame code (reference drain, noise suppression, VAD,
/// gate, meter) must not allocate. `sonora`'s echo canceller and gain
/// controller do allocate internally (small temporaries every frame); that
/// is measured and printed here as a known library limitation.
#[test]
fn processing_does_not_allocate() {
    assert_eq!(allocations_per_frame(false, false, false), 0.0);
    assert_eq!(allocations_per_frame(false, true, false), 0.0);
    println!(
        "sonora allocations per frame: echo cancellation {:.1}, auto gain {:.1}",
        allocations_per_frame(true, false, false),
        allocations_per_frame(false, false, true)
    );
}

/// Algorithmic delay of the full chain: lag that best aligns output with
/// input. Must stay within the ≤ 30 ms processing budget (spec §9).
#[test]
fn processing_latency_within_budget() {
    let input = speech_like(2 * SAMPLE_RATE as usize, 0.3);
    let (mut chain, _) = chain_with(true, true, true);
    let out = run(&mut chain, &input);
    let window = &input[SAMPLE_RATE as usize..SAMPLE_RATE as usize + 9600];
    let best_lag = (0..2400)
        .max_by(|&a, &b| {
            let corr = |lag: usize| -> f32 {
                let shifted = &out[SAMPLE_RATE as usize + lag..SAMPLE_RATE as usize + lag + 9600];
                window.iter().zip(shifted).map(|(x, y)| x * y).sum()
            };
            corr(a).total_cmp(&corr(b))
        })
        .expect("lag");
    let ms = best_lag as f32 * 1000.0 / SAMPLE_RATE as f32;
    println!("processing delay: {ms:.1} ms ({best_lag} samples)");
    assert!(ms <= 30.0, "processing delay {ms:.1} ms");
}

/// Benchmark: per-frame cost of the full chain on speech plus noise.
/// Ignored by default because wall-clock timing depends on the machine and
/// its load (debug builds on shared CI runners take ~7 ms per frame). Run it
/// in release:
/// `cargo test -p mumcord-audio --all-features --release -- --ignored --nocapture per_frame_processing_time`
#[test]
#[ignore = "benchmark; run in release with --ignored"]
fn per_frame_processing_time() {
    let mut input = speech_like(10 * SAMPLE_RATE as usize, 0.3);
    let noise = fan_noise(input.len(), 0.05);
    for (s, n) in input.iter_mut().zip(noise) {
        *s += n;
    }
    let (mut chain, mut reference) = chain_with(true, true, true);
    let far = fan_noise(input.len(), 0.1);
    let mut out = [0.0; FRAME_SAMPLES];
    let mut times: Vec<Duration> = Vec::with_capacity(input.len() / FRAME_SAMPLES);
    for (chunk, far) in input
        .as_chunks::<FRAME_SAMPLES>()
        .0
        .iter()
        .zip(far.as_chunks::<FRAME_SAMPLES>().0)
    {
        reference.push(far);
        let start = Instant::now();
        chain.process(chunk, &mut out);
        times.push(start.elapsed());
    }
    times.sort();
    let mean = times.iter().sum::<Duration>() / times.len() as u32;
    let p99 = times[times.len() * 99 / 100];
    let max = times[times.len() - 1];
    let cpu = mean.as_secs_f64() / 0.010 * 100.0;
    println!("per frame: mean {mean:?}, p99 {p99:?}, max {max:?} → {cpu:.2} % of one core");
    // A 10 ms frame must never take anywhere near 10 ms; in release the
    // chain needs well under 1 ms. The bound only holds for optimized builds.
    if !cfg!(debug_assertions) {
        assert!(mean < Duration::from_millis(5), "mean {mean:?}");
    }
}

fn write_wav(path: &Path, samples: &[f32]) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).expect("create wav");
    for s in samples {
        writer
            .write_sample((s.clamp(-1.0, 1.0) * 32767.0) as i16)
            .expect("write");
    }
    writer.finalize().expect("finalize");
}

fn read_wav(path: &Path) -> Vec<f32> {
    hound::WavReader::open(path)
        .expect("open wav")
        .samples::<i16>()
        .map(|s| f32::from(s.expect("sample")) / 32768.0)
        .collect()
}

#[test]
fn manual_threshold_gates_quiet_and_loud_tones_and_ptt_ignores_it() {
    let (mut chain, _) = chain_with(false, false, false);
    let controls = chain.controls().clone();
    controls.set_manual_threshold(true);
    controls.set_threshold_dbfs(-30.0);
    let tone = |amplitude: f32| -> Vec<f32> {
        (0..SAMPLE_RATE as usize / 2)
            .map(|i| amplitude * (2.0 * PI * 440.0 * i as f32 / SAMPLE_RATE as f32).sin())
            .collect()
    };
    let open_frames = |chain: &mut DspChain, samples: &[f32]| {
        let mut out = [0.0; FRAME_SAMPLES];
        samples
            .as_chunks::<FRAME_SAMPLES>()
            .0
            .iter()
            .filter(|frame| chain.process(frame, &mut out) == Gate::Open)
            .count()
    };
    // -46 dBFS peak is below the -30 dBFS threshold, -12 dBFS above it.
    assert_eq!(open_frames(&mut chain, &tone(0.005)), 0);
    assert!(open_frames(&mut chain, &tone(0.25)) >= 49);
    assert!(controls.voice_active());

    // Push-to-talk ignores the threshold.
    controls.set_mode(TalkMode::PushToTalk);
    controls.set_ptt_pressed(false);
    let mut out = [0.0; FRAME_SAMPLES];
    for _ in 0..VAD_HANGOVER_FRAMES + 10 {
        chain.process(&[0.0; FRAME_SAMPLES], &mut out);
    }
    assert_eq!(open_frames(&mut chain, &tone(0.25)), 0);
}

/// Sustained three-note chord with a slow swell, standing in for music.
fn music_like(len: usize, amplitude: f32) -> Vec<f32> {
    (0..len)
        .map(|i| {
            let t = i as f32 / SAMPLE_RATE as f32;
            let chord: f32 = [220.0, 277.2, 329.6]
                .iter()
                .map(|f| (2.0 * PI * f * t).sin())
                .sum::<f32>()
                / 3.0;
            amplitude * (0.75 + 0.25 * (2.0 * PI * 0.3 * t).sin()) * chord
        })
        .collect()
}

fn level_dbfs(samples: &[f32]) -> f32 {
    db(energy(samples))
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |p, s| p.max(s.abs()))
}

/// Output level of `input` after 10 s with auto gain on and music quality as given.
fn sent_level(input: &[f32], music: bool) -> (f32, f32) {
    let (mut chain, _) = chain_with(false, false, true);
    chain.controls().set_music_quality(music);
    let out = run(&mut chain, input);
    let tail = last_seconds(&out, 3.0);
    (level_dbfs(tail), peak(tail))
}

/// F14: with "Music quality" on, the sender skipped automatic gain and nothing replaced it,
/// so listeners heard music clients far quieter than speech clients.
#[test]
fn music_quality_sends_as_loud_as_speech() {
    let len = 10 * SAMPLE_RATE as usize;
    for amplitude in [0.02, 0.3] {
        let (speech, _) = sent_level(&speech_like(len, amplitude), false);
        let (music_mode, music_peak) = sent_level(&speech_like(len, amplitude), true);
        println!(
            "speech-like {amplitude}: voice {speech:.1} dBFS, music quality {music_mode:.1} dBFS"
        );
        assert!(
            (music_mode - speech).abs() <= 3.0,
            "music quality {music_mode:.1} vs voice {speech:.1} dBFS"
        );
        assert!(
            music_peak < 0.99,
            "music quality clips: peak {music_peak:.2}"
        );
    }

    // Quiet music is lifted to the level quiet speech is sent at.
    let (speech_quiet, _) = sent_level(&speech_like(len, 0.02), false);
    let (music_quiet, _) = sent_level(&music_like(len, 0.02), true);
    println!("quiet music {music_quiet:.1} dBFS, quiet speech {speech_quiet:.1} dBFS");
    assert!(
        (music_quiet - speech_quiet).abs() <= 3.0,
        "quiet music {music_quiet:.1} vs speech {speech_quiet:.1} dBFS"
    );

    // Loud music passes as it is, like loud speech: no compression and no clipping.
    let loud = music_like(len, 0.9);
    let (music_loud, loud_peak) = sent_level(&loud, true);
    let input_level = level_dbfs(last_seconds(&loud, 3.0));
    println!("loud music {music_loud:.1} dBFS (input {input_level:.1}), peak {loud_peak:.2}");
    assert!((music_loud - input_level).abs() <= 0.5);
    assert!(loud_peak < 0.99, "loud music clips: peak {loud_peak:.2}");
}

/// A sudden loud passage after a quiet one must not clip while the gain is still lifted.
#[test]
fn music_leveler_never_clips_on_a_sudden_peak() {
    let mut input = music_like(6 * SAMPLE_RATE as usize, 0.01);
    input.extend(music_like(2 * SAMPLE_RATE as usize, 0.8));
    let (mut chain, _) = chain_with(false, false, true);
    chain.controls().set_music_quality(true);
    let out = run(&mut chain, &input);
    let loud_peak = peak(last_seconds(&out, 2.0));
    println!("peak after sudden loud passage: {loud_peak:.3}");
    assert!(loud_peak <= 0.9, "peak {loud_peak:.3}");
}

/// Issue #156: low-latency mode sends the microphone signal untouched, even with echo
/// cancellation, noise suppression and automatic gain switched on.
#[test]
fn low_latency_bypasses_echo_cancellation_noise_suppression_and_gain() {
    let input = music_like(48_000 * 2, 0.05);
    let (mut chain, _echo) = chain_with(true, true, true);
    chain.controls().set_low_latency(true);
    let out = run(&mut chain, &input);
    assert_eq!(out.len(), input.len());
    assert!(
        out.iter().zip(&input).all(|(o, i)| o == i),
        "low-latency output differs from the input"
    );

    chain.controls().set_low_latency(false);
    let out = run(&mut chain, &input);
    assert!(
        out.iter().zip(&input).any(|(o, i)| (o - i).abs() > 1e-3),
        "the stages are back once the mode is off"
    );
}
