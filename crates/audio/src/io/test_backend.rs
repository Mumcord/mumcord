//! Device-free backend for headless tests and E2E (feature `test-backend`).
//! Threads stand in for the device callbacks and keep real-time pace, so the
//! rest of the pipeline behaves exactly as with hardware.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use rtrb::{Consumer, Producer};

use super::controller::{Backend, Opened, StreamGuard};
use super::ring::{self, Counters};
use super::{DeviceInfo, Direction, IoError};
use crate::SAMPLE_RATE;

const TICK: Duration = Duration::from_millis(10);
/// Refresh the WAV header once per second so a killed process leaves a valid file.
const FLUSH_EVERY_TICKS: u32 = 100;

static INPUT_OPENS: AtomicUsize = AtomicUsize::new(0);

/// How many input streams the test backend opened in this process, for tests that check the
/// microphone (and with it the OS permission prompt) is opened only once.
pub fn input_opens() -> usize {
    INPUT_OPENS.load(Ordering::Relaxed)
}

pub(crate) enum TestBackend {
    Null,
    File {
        input: PathBuf,
        output: PathBuf,
        /// Record playback as stereo instead of the mono mix.
        stereo: bool,
    },
}

impl TestBackend {
    fn device(&self, direction: Direction) -> DeviceInfo {
        let id = match (self, direction) {
            (TestBackend::Null, _) => "null".to_owned(),
            (TestBackend::File { input, .. }, Direction::Input) => {
                format!("file:{}", input.display())
            }
            (TestBackend::File { output, .. }, Direction::Output) => {
                format!("file:{}", output.display())
            }
        };
        DeviceInfo {
            name: id.clone(),
            id,
            is_default: true,
        }
    }
}

impl Backend for TestBackend {
    fn devices(&self, direction: Direction) -> Result<Vec<DeviceInfo>, IoError> {
        Ok(vec![self.device(direction)])
    }

    fn polls_devices(&self) -> bool {
        false
    }

    fn open_input(
        &self,
        _id: Option<&str>,
        counters: &Arc<Counters>,
    ) -> Result<Opened<Consumer<f32>>, IoError> {
        INPUT_OPENS.fetch_add(1, Ordering::Relaxed);
        // The WAV is streamed from the capture thread instead of being decoded here: a long
        // input file (E2E runs use minutes of silence) would otherwise hold up the control
        // thread for seconds and with it the output stream, which opens right after this one.
        let mut wav = match self {
            TestBackend::Null => None,
            TestBackend::File { input, .. } => Some(WavInput::open(input)?),
        };
        let rate = wav.as_ref().map_or(SAMPLE_RATE, WavInput::rate);
        let (producer, consumer) = ring::ring(rate);
        let counters = counters.clone();
        let guard = TickThread::spawn("audio-test-capture", move |_| {
            let mut producer = producer;
            let mut block = vec![0.0f32; rate as usize / 100];
            move || {
                let filled = wav.as_mut().map_or(0, |w| w.read(&mut block));
                block[filled..].fill(0.0);
                ring::capture(&block, 1, &mut producer, &counters);
                Ok(())
            }
        })?;
        Ok(Opened {
            guard: Box::new(guard),
            ring: consumer,
            rate,
            device: self.device(Direction::Input),
        })
    }

    fn open_output(
        &self,
        _id: Option<&str>,
        counters: &Arc<Counters>,
    ) -> Result<Opened<Producer<f32>>, IoError> {
        let (mut writer, channels) = match self {
            TestBackend::Null => (None, 1),
            TestBackend::File { output, stereo, .. } => {
                let channels = if *stereo { 2 } else { 1 };
                (Some(open_output_wav(output, channels)?), channels)
            }
        };
        let (producer, consumer) = ring::stereo_ring(SAMPLE_RATE);
        let counters = counters.clone();
        let guard = TickThread::spawn("audio-test-playback", move |stop| {
            let mut consumer = consumer;
            let mut block = vec![0.0f32; usize::from(channels) * SAMPLE_RATE as usize / 100];
            let mut ticks = 0u32;
            move || {
                ring::playback(&mut block, usize::from(channels), &mut consumer, &counters);
                if let Some(w) = writer.as_mut() {
                    for &s in &block {
                        w.write_sample(to_i16(s)).map_err(wav_err)?;
                    }
                    ticks += 1;
                    if ticks.is_multiple_of(FLUSH_EVERY_TICKS) || stop.load(Ordering::Relaxed) {
                        w.flush().map_err(wav_err)?;
                    }
                }
                Ok(())
            }
        })?;
        Ok(Opened {
            guard: Box::new(guard),
            ring: producer,
            rate: SAMPLE_RATE,
            device: self.device(Direction::Output),
        })
    }
}

/// A thread that calls a tick function once per 10 ms of elapsed wall-clock
/// time, like a device callback. Sleeps may overshoot (Windows timers tick at
/// ~15.6 ms, loaded machines preempt), so every wake-up runs all ticks that
/// came due meanwhile instead of assuming one tick per sleep.
struct TickThread {
    stop: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl TickThread {
    fn spawn<F, T>(name: &str, make_tick: F) -> Result<Self, IoError>
    where
        F: FnOnce(Arc<AtomicBool>) -> T + Send + 'static,
        T: FnMut() -> Result<(), IoError>,
    {
        let stop = Arc::new(AtomicBool::new(false));
        let failed = Arc::new(AtomicBool::new(false));
        let (thread_stop, thread_failed) = (stop.clone(), failed.clone());
        let handle = std::thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || {
                // Real device threads get real-time scheduling from cpal; so does the stand-in.
                let _realtime = crate::rt::promote_current_thread().ok();
                let mut tick = make_tick(thread_stop.clone());
                let mut schedule = Schedule::new(Instant::now());
                loop {
                    // One last tick after the stop request lets it flush.
                    let stopping = thread_stop.load(Ordering::Relaxed);
                    let due = if stopping {
                        1
                    } else {
                        schedule.due(Instant::now())
                    };
                    for _ in 0..due {
                        if let Err(e) = tick() {
                            tracing::error!("test audio backend failed: {e}");
                            thread_failed.store(true, Ordering::Relaxed);
                            return;
                        }
                    }
                    if stopping {
                        return;
                    }
                    schedule.complete(due);
                    if let Some(wait) = schedule.next_at().checked_duration_since(Instant::now()) {
                        std::thread::sleep(wait);
                    }
                }
            })
            .map_err(|e| IoError::Backend(e.to_string()))?;
        Ok(Self {
            stop,
            failed,
            handle: Some(handle),
        })
    }
}

/// Fixed 10 ms tick grid anchored at the start time; tick `n` is due at
/// `start + n * TICK`.
struct Schedule {
    start: Instant,
    done: u64,
}

impl Schedule {
    fn new(start: Instant) -> Self {
        Self { start, done: 0 }
    }

    /// Ticks due by `now` that have not run yet (0 after an early wake-up).
    fn due(&self, now: Instant) -> u64 {
        let elapsed = now.saturating_duration_since(self.start).as_nanos() / TICK.as_nanos();
        u64::try_from(elapsed + 1)
            .unwrap_or(u64::MAX)
            .saturating_sub(self.done)
    }

    fn complete(&mut self, ticks: u64) {
        self.done += ticks;
    }

    /// When the next tick is due.
    fn next_at(&self) -> Instant {
        let nanos = u64::try_from(TICK.as_nanos())
            .unwrap_or(u64::MAX)
            .saturating_mul(self.done);
        self.start + Duration::from_nanos(nanos)
    }
}

impl StreamGuard for TickThread {
    fn failed(&self) -> bool {
        self.failed.load(Ordering::Relaxed)
    }
}

impl Drop for TickThread {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take()
            && handle.join().is_err()
        {
            tracing::error!("test audio backend thread panicked");
        }
    }
}

type WavFile = std::io::BufReader<std::fs::File>;

enum Samples {
    /// Integer samples and the factor that maps them to -1.0..1.0.
    Int(hound::WavIntoSamples<WavFile, i32>, f32),
    Float(hound::WavIntoSamples<WavFile, f32>),
}

/// A WAV file read block by block as mono `f32` at its own sample rate.
struct WavInput {
    samples: Samples,
    channels: usize,
    rate: u32,
    done: bool,
}

impl WavInput {
    fn open(path: &Path) -> Result<Self, IoError> {
        let reader = hound::WavReader::open(path).map_err(wav_err)?;
        let spec = reader.spec();
        let samples = match spec.sample_format {
            hound::SampleFormat::Float => Samples::Float(reader.into_samples()),
            hound::SampleFormat::Int => {
                let scale = 1.0 / (1u64 << (spec.bits_per_sample - 1)) as f32;
                Samples::Int(reader.into_samples(), scale)
            }
        };
        Ok(Self {
            samples,
            channels: usize::from(spec.channels.max(1)),
            rate: spec.sample_rate,
            done: false,
        })
    }

    fn rate(&self) -> u32 {
        self.rate
    }

    fn next_sample(&mut self) -> Option<f32> {
        let next = match &mut self.samples {
            Samples::Float(samples) => samples.next(),
            Samples::Int(samples, scale) => samples.next().map(|s| s.map(|s| s as f32 * *scale)),
        };
        match next {
            Some(Ok(sample)) => Some(sample),
            Some(Err(e)) => {
                tracing::warn!("test audio input stopped: {}", wav_err(e));
                None
            }
            None => None,
        }
    }

    /// Fills `out` with downmixed frames; returns how many were filled (fewer at the end of
    /// the file, which then plays as silence).
    fn read(&mut self, out: &mut [f32]) -> usize {
        if self.done {
            return 0;
        }
        for (filled, slot) in out.iter_mut().enumerate() {
            let mut sum = 0.0;
            for _ in 0..self.channels {
                let Some(sample) = self.next_sample() else {
                    self.done = true;
                    return filled;
                };
                sum += sample;
            }
            *slot = sum / self.channels as f32;
        }
        out.len()
    }
}

type WavWriter = hound::WavWriter<std::io::BufWriter<std::fs::File>>;

/// Output files this process has already recorded to.
static RECORDED: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());

/// Opens the output recording. The first open in a process starts a new file; later opens
/// (the engine closes and reopens the devices between mic test, connections and device
/// listings, the controller reopens after errors or device switches) continue it, so
/// everything played during one app run stays in one recording.
fn open_output_wav(path: &Path, channels: u16) -> Result<WavWriter, IoError> {
    let mut recorded = RECORDED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if recorded.iter().any(|p| p == path) {
        match hound::WavWriter::append(path) {
            Ok(writer) if writer.spec() == output_spec(channels) => return Ok(writer),
            Ok(_) => tracing::warn!("test audio output has another format; starting it anew"),
            Err(e) => {
                tracing::warn!("continuing the test audio output failed ({e}); starting it anew")
            }
        }
    } else {
        recorded.push(path.to_owned());
    }
    hound::WavWriter::create(path, output_spec(channels)).map_err(wav_err)
}

fn output_spec(channels: u16) -> hound::WavSpec {
    hound::WavSpec {
        channels,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    }
}

fn to_i16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16
}

fn wav_err(err: hound::Error) -> IoError {
    IoError::Backend(format!("wav: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Windows' default timer resolution: sleeps end on a 15.625 ms grid.
    const GRANULARITY: Duration = Duration::from_micros(15_625);

    /// Wakes at the first timer interrupt at or after `deadline`.
    fn coarse_wake(start: Instant, deadline: Instant) -> Instant {
        let wanted = deadline.saturating_duration_since(start).as_nanos();
        let step = GRANULARITY.as_nanos();
        let steps = wanted.div_ceil(step) + 1;
        start + Duration::from_nanos(u64::try_from(steps * step).expect("fits"))
    }

    #[test]
    fn schedule_keeps_real_time_pace_with_coarse_sleeps() {
        let start = Instant::now();
        let mut schedule = Schedule::new(start);
        // A producer queues one 10 ms block per 10 ms; the tick drains one
        // block per due tick, as the playback thread does.
        let mut produced = 0u64;
        let mut drained = 0u64;
        let mut max_queued = 0u64;
        let mut now = start;
        while now < start + Duration::from_secs(5) {
            let due = schedule.due(now);
            produced = (now - start).as_millis() as u64 / 10 + 1;
            drained = (drained + due).min(produced);
            schedule.complete(due);
            max_queued = max_queued.max(produced - drained);
            now = coarse_wake(start, schedule.next_at());
            // Production that happened while asleep, before the drain.
            let arrived = (now - start).as_millis() as u64 / 10 + 1;
            max_queued = max_queued.max(arrived - drained);
        }
        // One wake-up is at most ~2 timer periods, so the queue never holds
        // more than a few blocks and never grows over time.
        assert!(max_queued <= 4, "queue reached {max_queued} blocks");
        assert!(
            produced - drained <= 1,
            "lagging {} blocks",
            produced - drained
        );
    }

    #[test]
    fn schedule_catches_up_after_a_long_stall_and_ignores_early_wakes() {
        let start = Instant::now();
        let mut schedule = Schedule::new(start);
        assert_eq!(schedule.due(start), 1);
        schedule.complete(1);
        // Woken before the next tick: nothing to do yet.
        assert_eq!(schedule.due(start + Duration::from_millis(9)), 0);
        assert_eq!(schedule.next_at(), start + TICK);
        // Descheduled for a second: all 100 missed ticks run at once.
        let late = start + Duration::from_millis(1_000);
        assert_eq!(schedule.due(late), 100);
        schedule.complete(100);
        assert_eq!(schedule.due(late), 0);
        assert_eq!(schedule.next_at(), start + Duration::from_millis(1_010));
    }
}
