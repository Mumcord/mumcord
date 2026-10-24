//! Device-free backend for headless tests and E2E (feature `test-backend`).
//! Threads stand in for the device callbacks and keep real-time pace, so the
//! rest of the pipeline behaves exactly as with hardware.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
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

pub(crate) enum TestBackend {
    Null,
    File { input: PathBuf, output: PathBuf },
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
        let (samples, rate) = match self {
            TestBackend::Null => (Vec::new(), SAMPLE_RATE),
            TestBackend::File { input, .. } => read_wav_mono(input)?,
        };
        let (producer, consumer) = ring::ring(rate);
        let counters = counters.clone();
        let guard = TickThread::spawn("audio-test-capture", move |_| {
            let mut producer = producer;
            let mut position = 0;
            let mut silence = vec![0.0f32; rate as usize / 100];
            move || {
                let block = samples
                    .get(position..)
                    .map_or(&[][..], |rest| &rest[..rest.len().min(silence.len())]);
                position += block.len();
                silence[..block.len()].copy_from_slice(block);
                silence[block.len()..].fill(0.0);
                ring::capture(&silence, 1, &mut producer, &counters);
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
        let mut writer = match self {
            TestBackend::Null => None,
            TestBackend::File { output, .. } => Some(create_wav(output)?),
        };
        let (producer, consumer) = ring::ring(SAMPLE_RATE);
        let counters = counters.clone();
        let guard = TickThread::spawn("audio-test-playback", move |stop| {
            let mut consumer = consumer;
            let mut block = [0.0f32; SAMPLE_RATE as usize / 100];
            let mut ticks = 0u32;
            move || {
                ring::playback(&mut block, 1, &mut consumer, &counters);
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

/// A thread that calls a tick function every 10 ms on a fixed schedule,
/// catching up after oversleeping like a device would.
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
                let mut tick = make_tick(thread_stop.clone());
                let mut next = Instant::now();
                loop {
                    let stopping = thread_stop.load(Ordering::Relaxed);
                    if let Err(e) = tick() {
                        tracing::error!("test audio backend failed: {e}");
                        thread_failed.store(true, Ordering::Relaxed);
                        return;
                    }
                    if stopping {
                        return;
                    }
                    next += TICK;
                    if let Some(wait) = next.checked_duration_since(Instant::now()) {
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

/// Reads a whole WAV file as mono `f32` at its own sample rate.
fn read_wav_mono(path: &Path) -> Result<(Vec<f32>, u32), IoError> {
    let mut reader = hound::WavReader::open(path).map_err(wav_err)?;
    let spec = reader.spec();
    let channels = usize::from(spec.channels.max(1));
    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>(),
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|s| s as f32 * scale))
                .collect::<Result<_, _>>()
        }
    }
    .map_err(wav_err)?;
    let mono = interleaved
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect();
    Ok((mono, spec.sample_rate))
}

type WavWriter = hound::WavWriter<std::io::BufWriter<std::fs::File>>;

fn create_wav(path: &Path) -> Result<WavWriter, IoError> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    hound::WavWriter::create(path, spec).map_err(wav_err)
}

fn to_i16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16
}

fn wav_err(err: hound::Error) -> IoError {
    IoError::Backend(format!("wav: {err}"))
}
