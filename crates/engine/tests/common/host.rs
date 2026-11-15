//! A stand-in for the phone app's audio session: a thread that calls the endpoints of the
//! platform-fed audio backend every 10 ms, as the system's microphone and speaker callbacks do.

use std::collections::VecDeque;
use std::f32::consts::PI;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use mumcord_audio::io::{ExternalAudio, ExternalFormat, external};

pub const SAMPLE_RATE: usize = 48_000;
/// Samples per callback: 10 ms.
const BLOCK: usize = SAMPLE_RATE / 100;

pub struct Host {
    stop: Arc<AtomicBool>,
    microphone: Arc<Mutex<VecDeque<f32>>>,
    played: Arc<Mutex<Vec<f32>>>,
    thread: Option<JoinHandle<()>>,
}

impl Host {
    /// Starts the callbacks (48 kHz mono in both directions) and returns the engine's part.
    pub fn start() -> (Self, ExternalAudio) {
        let format = ExternalFormat {
            sample_rate: SAMPLE_RATE as u32,
            channels: 1,
        };
        let (audio, mut capture, mut playback) = external(format, format).expect("external audio");
        let stop = Arc::new(AtomicBool::new(false));
        let microphone = Arc::new(Mutex::new(VecDeque::new()));
        let played = Arc::new(Mutex::new(Vec::new()));
        let (thread_stop, thread_microphone, thread_played) =
            (stop.clone(), microphone.clone(), played.clone());
        let thread = std::thread::spawn(move || {
            let start = Instant::now();
            let (mut input, mut output) = ([0.0f32; BLOCK], [0.0f32; BLOCK]);
            let mut done = 0u128;
            while !thread_stop.load(Ordering::Relaxed) {
                // Every block that came due, so a late wake-up does not slow the clock down.
                let due = start.elapsed().as_millis() / 10;
                while done < due {
                    {
                        let mut microphone = lock(&thread_microphone);
                        input.fill_with(|| microphone.pop_front().unwrap_or(0.0));
                    }
                    capture.push_f32(&input);
                    playback.pull_f32(&mut output);
                    lock(&thread_played).extend_from_slice(&output);
                    done += 1;
                }
                std::thread::sleep(Duration::from_millis(2));
            }
        });
        let host = Self {
            stop,
            microphone,
            played,
            thread: Some(thread),
        };
        (host, audio)
    }

    /// Queues `samples` as what the microphone hears next.
    pub fn speak(&self, samples: &[f32]) {
        lock(&self.microphone).extend(samples);
    }

    /// Samples played so far; also the position on the playback timeline.
    pub fn played_len(&self) -> usize {
        lock(&self.played).len()
    }

    /// Everything played from position `from` on.
    pub fn played_from(&self, from: usize) -> Vec<f32> {
        lock(&self.played)[from..].to_vec()
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A voiced, 4 Hz pulsed signal that noise suppression and voice activation treat as speech.
pub fn speech_like(len: usize, amplitude: f32) -> Vec<f32> {
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

pub fn rms(samples: &[f32]) -> f32 {
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len().max(1) as f32).sqrt()
}
