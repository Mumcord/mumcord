//! Plugin sound effects (spec 09, FR-9.65 … FR-9.68): short sounds bundled in a plugin package,
//! played locally only and never sent to other participants.
//!
//! The plugin host hands over a request (plugin, file, gain) and returns at once. One worker
//! thread reads and decodes the file (Ogg Vorbis or WAV, to 48 kHz mono), mixes up to
//! [`MAX_VOICES`] sounds across all plugins together with Mumcord's own notification sounds and
//! writes the result in 10 ms chunks into the notification-sound output of the [`SoundHub`]
//! (the mixer's mix-in port, a lock-free ring the playback thread reads). Nothing here runs on
//! the real-time audio thread: decoding, mixing, gain and logging all happen on the worker.
//!
//! While plugin sounds play, the hub's notification sounds go to the worker too, so both are
//! summed instead of queued one after the other in the ring.
//!
//! Gain rules: the user's plugin-sound volume and mute (Settings → Plugins, default 70 %, not
//! muted), −12 dB while anyone (the user included) talks, silent while the user is deafened.
//!
//! [`SoundHub`]: crate::sounds::SoundHub

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use lewton::inside_ogg::OggStreamReader;

use crate::session::EngineEvent;

/// At most this many plugin sounds play at once across all plugins; a new one stops the oldest.
pub const MAX_VOICES: usize = 4;
/// Gain while anyone talks: −12 dB.
pub const DUCK_GAIN: f32 = 0.251_188_64;
/// Plugin-sound volume until the settings say otherwise (FR-9.68).
pub const DEFAULT_VOLUME: u8 = 70;
/// Output rate of the mix-in port.
pub const SAMPLE_RATE: u32 = 48_000;
/// Largest sound file (FR-9.65); the package check enforces it at install, this guards the
/// files on disk.
pub const MAX_FILE_BYTES: u64 = 1024 * 1024;
/// Longest sound (FR-9.65), in output samples, with a little room for resampling.
pub const MAX_SAMPLES: usize = SAMPLE_RATE as usize * 10 + SAMPLE_RATE as usize / 10;
/// Name of the worker thread.
pub const WORKER_NAME: &str = "mumcord-plugin-sounds";

/// Samples per chunk the worker writes (10 ms).
const CHUNK: usize = SAMPLE_RATE as usize / 100;
/// How far the worker writes ahead of real time, so a late wake-up does not starve the mixer.
const LEAD: usize = 4 * CHUNK;
const TICK: Duration = Duration::from_millis(10);

/// Where mixed chunks go: the hub's notification-sound output. `false` when there is no output
/// or no room right now.
pub type Push = Box<dyn Fn(&[f32]) -> bool + Send + Sync>;

enum Command {
    Play {
        plugin: Arc<str>,
        file: PathBuf,
        gain: f32,
    },
    StopAll(Arc<str>),
    /// The plugin is gone: stop its sounds and drop its decoded files.
    Forget(Arc<str>),
    /// Notification samples to sum in while plugin sounds play.
    Notification(Vec<f32>),
    /// Audio closed: drop everything that plays.
    Reset,
}

/// The state the worker reads every chunk; written from any thread.
#[derive(Debug)]
struct Shared {
    volume: AtomicU8,
    muted: AtomicBool,
    deaf: AtomicBool,
    talking: AtomicBool,
    /// The worker mixes (plugin sounds play): notification sounds go through it.
    active: AtomicBool,
    /// Peak of the last mixed chunk (`f32` bits), for the debug audio meter.
    peak: AtomicU32,
}

/// Plays plugin sounds. Cheap to share; the worker starts with the first sound.
pub struct PluginSounds {
    shared: Arc<Shared>,
    /// Sessions talking right now (ducking), fed from the engine's talk events.
    talkers: Mutex<BTreeSet<u32>>,
    push: Arc<Push>,
    tx: Mutex<Option<Sender<Command>>>,
}

impl std::fmt::Debug for PluginSounds {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginSounds")
            .field("shared", &self.shared)
            .finish_non_exhaustive()
    }
}

impl PluginSounds {
    /// Sounds whose mixed chunks go to `push`.
    pub fn new(push: Push) -> Self {
        Self {
            shared: Arc::new(Shared {
                volume: AtomicU8::new(DEFAULT_VOLUME),
                muted: AtomicBool::new(false),
                deaf: AtomicBool::new(false),
                talking: AtomicBool::new(false),
                active: AtomicBool::new(false),
                peak: AtomicU32::new(0),
            }),
            talkers: Mutex::new(BTreeSet::new()),
            push: Arc::new(push),
            tx: Mutex::new(None),
        }
    }

    /// Plays `file` (a declared sound of `plugin`, already checked by the plugin host) at `gain`
    /// (0.0 … 1.0). Returns at once; the worker reads and decodes the file.
    pub fn play(&self, plugin: &str, file: PathBuf, gain: f32) {
        self.shared.active.store(true, Ordering::Release);
        self.send(Command::Play {
            plugin: plugin.into(),
            file,
            gain: gain.clamp(0.0, 1.0),
        });
    }

    /// Stops every sound `plugin` plays.
    pub fn stop_all(&self, plugin: &str) {
        self.send_if_running(Command::StopAll(plugin.into()));
    }

    /// The plugin was stopped or removed: stops its sounds and drops its decoded files.
    pub fn forget(&self, plugin: &str) {
        self.send_if_running(Command::Forget(plugin.into()));
    }

    /// The plugin-sound volume in percent (0 … 100).
    pub fn set_volume(&self, percent: u8) {
        self.shared
            .volume
            .store(percent.min(100), Ordering::Relaxed);
    }

    /// The global plugin-sound mute.
    pub fn set_muted(&self, muted: bool) {
        self.shared.muted.store(muted, Ordering::Relaxed);
    }

    /// The user's own deafen state; plugin sounds are silent while deafened.
    pub fn set_deafened(&self, deaf: bool) {
        self.shared.deaf.store(deaf, Ordering::Relaxed);
    }

    /// Peak level (0.0 … 1.0) of the plugin-sound output right now, for the debug audio meter
    /// (AC-9.23). Zero while nothing plays.
    pub fn meter(&self) -> f32 {
        f32::from_bits(self.shared.peak.load(Ordering::Relaxed))
    }

    /// Whether plugin sounds are being mixed; notification sounds then go through
    /// [`notification`](Self::notification).
    pub fn mixing(&self) -> bool {
        self.shared.active.load(Ordering::Acquire)
    }

    /// Sums notification samples into the plugin-sound mix.
    pub fn notification(&self, samples: &[f32]) {
        self.send_if_running(Command::Notification(samples.to_vec()));
    }

    /// Feeds an engine event: talk changes drive the ducking.
    pub fn observe(&self, event: &EngineEvent) {
        let mut talkers = self.talkers.lock().unwrap_or_else(PoisonError::into_inner);
        match event {
            EngineEvent::TalkStateChanged { session, talking } => {
                if *talking {
                    talkers.insert(*session);
                } else {
                    talkers.remove(session);
                }
            }
            EngineEvent::UserRemoved(session) => {
                talkers.remove(session);
            }
            _ => return,
        }
        self.shared
            .talking
            .store(!talkers.is_empty(), Ordering::Relaxed);
    }

    /// Audio closed: everything playing stops and nobody counts as talking any more.
    pub fn reset(&self) {
        self.talkers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
        self.shared.talking.store(false, Ordering::Relaxed);
        self.send_if_running(Command::Reset);
    }

    fn send_if_running(&self, command: Command) {
        let tx = self.tx.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(tx) = tx.as_ref() {
            // The worker only ends when this sender is dropped.
            let _ = tx.send(command);
        }
    }

    fn send(&self, command: Command) {
        let mut tx = self.tx.lock().unwrap_or_else(PoisonError::into_inner);
        if tx.is_none() {
            let (sender, rx) = mpsc::channel();
            let shared = Arc::clone(&self.shared);
            let push = Arc::clone(&self.push);
            let spawned = std::thread::Builder::new()
                .name(WORKER_NAME.into())
                .spawn(move || Worker::new(shared, push, rx).run());
            match spawned {
                Ok(_) => *tx = Some(sender),
                Err(error) => {
                    tracing::error!(%error, "plugin sounds: worker thread did not start");
                    self.shared.active.store(false, Ordering::Release);
                    return;
                }
            }
        }
        if let Some(tx) = tx.as_ref() {
            let _ = tx.send(command);
        }
    }
}

/// The gain every plugin sound gets right now.
fn target_gain(shared: &Shared) -> f32 {
    if shared.muted.load(Ordering::Relaxed) || shared.deaf.load(Ordering::Relaxed) {
        return 0.0;
    }
    let volume = f32::from(shared.volume.load(Ordering::Relaxed)) / 100.0;
    if shared.talking.load(Ordering::Relaxed) {
        volume * DUCK_GAIN
    } else {
        volume
    }
}

struct Worker {
    shared: Arc<Shared>,
    push: Arc<Push>,
    rx: Receiver<Command>,
    mixer: Mixer,
    /// Decoded files per plugin, so a repeated sound is decoded once.
    decoded: HashMap<Arc<str>, HashMap<PathBuf, Arc<[f32]>>>,
    chunk: Vec<f32>,
}

impl Worker {
    fn new(shared: Arc<Shared>, push: Arc<Push>, rx: Receiver<Command>) -> Self {
        Self {
            shared,
            push,
            rx,
            mixer: Mixer::default(),
            decoded: HashMap::new(),
            chunk: vec![0.0; CHUNK],
        }
    }

    fn run(mut self) {
        loop {
            // Idle: wait for the next command without spinning.
            self.shared.peak.store(0, Ordering::Relaxed);
            self.shared.active.store(false, Ordering::Release);
            match self.rx.recv() {
                Ok(command) => self.handle(command),
                Err(_) => return,
            }
            if self.mixer.idle() {
                continue;
            }
            self.shared.active.store(true, Ordering::Release);
            if !self.burst() {
                return;
            }
        }
    }

    /// Mixes until nothing plays; paced by the clock, [`LEAD`] ahead. `false` when the
    /// [`PluginSounds`] is gone.
    fn burst(&mut self) -> bool {
        let start = Instant::now();
        let mut written = 0usize;
        loop {
            loop {
                match self.rx.try_recv() {
                    Ok(command) => self.handle(command),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => return false,
                }
            }
            if self.mixer.idle() {
                return true;
            }
            let elapsed = start.elapsed().as_secs_f64();
            // Truncation is fine: the next tick writes the rest.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let due = LEAD + (elapsed * f64::from(SAMPLE_RATE)) as usize;
            while written + CHUNK <= due && !self.mixer.idle() {
                let peak = self.mixer.fill(&mut self.chunk, target_gain(&self.shared));
                if !(self.push)(&self.chunk) {
                    // No output or no room: this chunk is lost, time moves on regardless.
                    tracing::trace!("plugin sounds: mixer input full or closed");
                }
                self.shared.peak.store(peak.to_bits(), Ordering::Relaxed);
                written += CHUNK;
            }
            match self.rx.recv_timeout(TICK) {
                Ok(command) => self.handle(command),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return false,
            }
        }
    }

    fn handle(&mut self, command: Command) {
        match command {
            Command::Play { plugin, file, gain } => {
                if let Some(samples) = self.samples(&plugin, &file) {
                    self.mixer.play(plugin, samples, gain);
                }
            }
            Command::StopAll(plugin) => self.mixer.stop(&plugin),
            Command::Forget(plugin) => {
                self.mixer.stop(&plugin);
                self.decoded.remove(&plugin);
            }
            Command::Notification(samples) => self.mixer.notification(&samples),
            Command::Reset => self.mixer = Mixer::default(),
        }
    }

    fn samples(&mut self, plugin: &Arc<str>, file: &Path) -> Option<Arc<[f32]>> {
        let files = self.decoded.entry(Arc::clone(plugin)).or_default();
        if let Some(samples) = files.get(file) {
            return Some(Arc::clone(samples));
        }
        match decode_file(file) {
            Ok(samples) => {
                let samples: Arc<[f32]> = samples.into();
                files.insert(file.to_owned(), Arc::clone(&samples));
                Some(samples)
            }
            Err(error) => {
                let name = file.file_name().map(|n| n.to_string_lossy().into_owned());
                tracing::warn!(plugin = %plugin, file = ?name, %error, "plugin sound not played");
                None
            }
        }
    }
}

/// One plugin sound that plays.
struct Voice {
    plugin: Arc<str>,
    samples: Arc<[f32]>,
    pos: usize,
    gain: f32,
}

/// Sums the plugin sounds and the notification samples, chunk by chunk.
#[derive(Default)]
struct Mixer {
    /// Oldest first.
    voices: VecDeque<Voice>,
    notifications: VecDeque<f32>,
    /// The gain of the last chunk's end; the next chunk ramps from it to avoid clicks.
    gain: f32,
}

impl Mixer {
    fn play(&mut self, plugin: Arc<str>, samples: Arc<[f32]>, gain: f32) {
        if self.voices.len() >= MAX_VOICES {
            self.voices.pop_front();
        }
        self.voices.push_back(Voice {
            plugin,
            samples,
            pos: 0,
            gain,
        });
    }

    fn stop(&mut self, plugin: &str) {
        self.voices.retain(|v| &*v.plugin != plugin);
    }

    fn notification(&mut self, samples: &[f32]) {
        self.notifications.extend(samples);
    }

    fn idle(&self) -> bool {
        self.voices.is_empty() && self.notifications.is_empty()
    }

    /// Writes the next chunk into `out` with the plugin sounds at `target` gain (ramped from
    /// the previous chunk's); returns its peak.
    fn fill(&mut self, out: &mut [f32], target: f32) -> f32 {
        out.fill(0.0);
        #[allow(clippy::cast_precision_loss)]
        let step = (target - self.gain) / out.len() as f32;
        for voice in &mut self.voices {
            let rest = &voice.samples[voice.pos.min(voice.samples.len())..];
            for (i, (dst, src)) in out.iter_mut().zip(rest).enumerate() {
                #[allow(clippy::cast_precision_loss)]
                let gain = (i + 1) as f32 * step + self.gain;
                *dst += src * voice.gain * gain.min(target.max(self.gain));
            }
            voice.pos += out.len();
        }
        self.gain = target;
        self.voices.retain(|v| v.pos < v.samples.len());
        for dst in out.iter_mut() {
            match self.notifications.pop_front() {
                Some(s) => *dst += s,
                None => break,
            }
        }
        out.iter_mut().fold(0.0_f32, |peak, s| {
            *s = s.clamp(-1.0, 1.0);
            peak.max(s.abs())
        })
    }
}

/// Why a sound file did not play.
#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("cannot read the file: {0}")]
    Read(#[from] std::io::Error),
    #[error("the file is larger than 1 MiB")]
    TooLarge,
    #[error("the file is longer than 10 seconds")]
    TooLong,
    #[error("not a playable Ogg Vorbis or WAV file")]
    Format,
}

/// Reads and decodes a sound file to 48 kHz mono. Runs on the worker thread only.
pub fn decode_file(file: &Path) -> Result<Vec<f32>, DecodeError> {
    #[cfg(test)]
    tests::note_decode_thread();
    if std::fs::metadata(file)?.len() > MAX_FILE_BYTES {
        return Err(DecodeError::TooLarge);
    }
    let data = std::fs::read(file)?;
    let is_wav = file
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("wav"));
    let (mono, rate) = if is_wav {
        decode_wav(&data)
    } else {
        decode_ogg(&data)
    }
    .ok_or(DecodeError::Format)?;
    let samples = resample(&mono, rate);
    if samples.len() > MAX_SAMPLES {
        return Err(DecodeError::TooLong);
    }
    Ok(samples)
}

/// Mono samples and their rate.
fn decode_wav(data: &[u8]) -> Option<(Vec<f32>, u32)> {
    let reader = hound::WavReader::new(Cursor::new(data)).ok()?;
    let spec = reader.spec();
    let channels = usize::from(spec.channels);
    if channels == 0 || spec.sample_rate == 0 {
        return None;
    }
    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .into_samples::<f32>()
            .collect::<Result<_, _>>()
            .ok()?,
        hound::SampleFormat::Int => {
            let scale =
                1.0 / f64::from(1u32 << spec.bits_per_sample.clamp(1, 32).saturating_sub(1));
            reader
                .into_samples::<i32>()
                .map(|s| s.map(|s| (f64::from(s) * scale) as f32))
                .collect::<Result<_, _>>()
                .ok()?
        }
    };
    Some((downmix(&interleaved, channels), spec.sample_rate))
}

fn decode_ogg(data: &[u8]) -> Option<(Vec<f32>, u32)> {
    let mut reader = OggStreamReader::new(Cursor::new(data)).ok()?;
    let channels = usize::from(reader.ident_hdr.audio_channels);
    let rate = reader.ident_hdr.audio_sample_rate;
    if channels == 0 || rate == 0 {
        return None;
    }
    let mut interleaved = Vec::new();
    while let Some(packet) = reader.read_dec_packet_itl().ok()? {
        interleaved.extend(packet.into_iter().map(|s| f32::from(s) / 32768.0));
        // Stop decoding a file that claims to be short but is not.
        if interleaved.len() / channels > MAX_SAMPLES * 4 {
            return None;
        }
    }
    Some((downmix(&interleaved, channels), rate))
}

fn downmix(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels == 1 {
        return interleaved.to_vec();
    }
    #[allow(clippy::cast_precision_loss)]
    let scale = 1.0 / channels as f32;
    interleaved
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() * scale)
        .collect()
}

/// Linear resampling to [`SAMPLE_RATE`]; short effects do not need more.
fn resample(samples: &[f32], rate: u32) -> Vec<f32> {
    if rate == SAMPLE_RATE || samples.is_empty() {
        return samples.to_vec();
    }
    let ratio = f64::from(rate) / f64::from(SAMPLE_RATE);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let len = (samples.len() as f64 / ratio) as usize;
    let last = samples.len() - 1;
    (0..len)
        .map(|i| {
            #[allow(clippy::cast_precision_loss)]
            let at = i as f64 * ratio;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let i0 = (at as usize).min(last);
            let i1 = (i0 + 1).min(last);
            #[allow(clippy::cast_possible_truncation)]
            let t = (at - i0 as f64) as f32;
            samples[i0] + (samples[i1] - samples[i0]) * t
        })
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    static DECODE_THREADS: Mutex<Vec<String>> = Mutex::new(Vec::new());

    pub(super) fn note_decode_thread() {
        let name = std::thread::current().name().unwrap_or("").to_owned();
        DECODE_THREADS.lock().unwrap().push(name);
    }

    fn tone(len: usize, amp: f32) -> Arc<[f32]> {
        (0..len)
            .map(|i| {
                #[allow(clippy::cast_precision_loss)]
                let t = i as f32 / SAMPLE_RATE as f32;
                amp * (2.0 * std::f32::consts::PI * 440.0 * t).sin()
            })
            .collect()
    }

    fn wav_tone(dir: &Path, name: &str, rate: u32, channels: u16, seconds: f32) -> PathBuf {
        let path = dir.join(name);
        let spec = hound::WavSpec {
            channels,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(&path, spec).unwrap();
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let frames = (rate as f32 * seconds) as u32;
        for i in 0..frames {
            #[allow(clippy::cast_precision_loss)]
            let t = i as f32 / rate as f32;
            #[allow(clippy::cast_possible_truncation)]
            let s = (0.5 * (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 32767.0) as i16;
            for _ in 0..channels {
                w.write_sample(s).unwrap();
            }
        }
        w.finalize().unwrap();
        path
    }

    fn peak(s: &[f32]) -> f32 {
        s.iter().fold(0.0_f32, |m, x| m.max(x.abs()))
    }

    /// Fills chunks until the ramp has settled and returns the last chunk's peak.
    fn settled_peak(mixer: &mut Mixer, target: f32) -> f32 {
        let mut out = vec![0.0; CHUNK];
        mixer.fill(&mut out, target);
        mixer.fill(&mut out, target)
    }

    #[test]
    fn at_most_four_voices_and_the_oldest_stops() {
        let mut mixer = Mixer::default();
        for name in ["a", "b", "c", "d", "e"] {
            mixer.play(name.into(), tone(SAMPLE_RATE as usize, 0.1), 1.0);
        }
        assert_eq!(mixer.voices.len(), MAX_VOICES);
        let plugins: Vec<&str> = mixer.voices.iter().map(|v| &*v.plugin).collect();
        assert_eq!(plugins, ["b", "c", "d", "e"]);
    }

    #[test]
    fn stop_all_only_stops_that_plugin() {
        let mut mixer = Mixer::default();
        mixer.play("a".into(), tone(1000, 0.1), 1.0);
        mixer.play("b".into(), tone(1000, 0.1), 1.0);
        mixer.play("a".into(), tone(1000, 0.1), 1.0);
        mixer.stop("a");
        assert_eq!(mixer.voices.len(), 1);
        assert_eq!(&*mixer.voices[0].plugin, "b");
    }

    #[test]
    fn gain_follows_volume_mute_deafen_and_ducking() {
        let sounds = PluginSounds::new(Box::new(|_| true));
        let gain = || target_gain(&sounds.shared);
        assert!((gain() - 0.7).abs() < 1e-6, "default 70 %");
        sounds.observe(&EngineEvent::TalkStateChanged {
            session: 3,
            talking: true,
        });
        assert!(
            (gain() - 0.7 * DUCK_GAIN).abs() < 1e-6,
            "−12 dB while talking"
        );
        sounds.observe(&EngineEvent::UserRemoved(3));
        assert!(
            (gain() - 0.7).abs() < 1e-6,
            "a removed talker stops ducking"
        );
        sounds.set_volume(250);
        assert!((gain() - 1.0).abs() < 1e-6, "volume is capped at 100 %");
        sounds.set_muted(true);
        assert!(gain().abs() < f32::EPSILON);
        sounds.set_muted(false);
        sounds.set_deafened(true);
        assert!(gain().abs() < f32::EPSILON, "silent while deafened");
    }

    #[test]
    fn ducking_lowers_the_level_by_12_db() {
        let mut mixer = Mixer::default();
        mixer.play("a".into(), tone(SAMPLE_RATE as usize, 0.5), 1.0);
        let full = settled_peak(&mut mixer, 1.0);
        let ducked = settled_peak(&mut mixer, DUCK_GAIN);
        let db = 20.0 * (ducked / full).log10();
        assert!((db + 12.0).abs() < 0.3, "ducked by {db} dB");
        assert!(settled_peak(&mut mixer, 0.0) < 1e-6, "silent at gain 0");
    }

    #[test]
    fn notification_samples_are_summed_in_at_unity() {
        let mut mixer = Mixer::default();
        mixer.play("a".into(), Arc::from(vec![0.25; CHUNK * 4]), 1.0);
        mixer.notification(&[0.5; CHUNK]);
        let mut out = vec![0.0; CHUNK];
        mixer.fill(&mut out, 0.0);
        assert!(
            out.iter().all(|s| (s - 0.5).abs() < 1e-6),
            "notification at gain 0 of plugins"
        );
        mixer.fill(&mut out, 1.0);
        assert!(out.iter().all(|s| *s <= 0.25 + 1e-6));
    }

    #[test]
    fn decodes_wav_to_48k_mono_and_rejects_bad_files() {
        let dir = tempfile::tempdir().unwrap();
        let stereo = wav_tone(dir.path(), "s.wav", 44_100, 2, 0.5);
        let samples = decode_file(&stereo).unwrap();
        assert!(
            (samples.len() as i64 - 24_000).abs() <= 2,
            "{}",
            samples.len()
        );
        assert!((peak(&samples) - 0.5).abs() < 0.02);

        let long = wav_tone(dir.path(), "long.wav", 8_000, 1, 11.0);
        assert!(matches!(decode_file(&long), Err(DecodeError::TooLong)));

        let bad = dir.path().join("bad.ogg");
        std::fs::write(&bad, b"not ogg").unwrap();
        assert!(matches!(decode_file(&bad), Err(DecodeError::Format)));

        let big = dir.path().join("big.wav");
        std::fs::write(&big, vec![0u8; 1024 * 1024 + 1]).unwrap();
        assert!(matches!(decode_file(&big), Err(DecodeError::TooLarge)));
    }

    /// A played test tone reaches the output at volume × gain, ducked while someone talks,
    /// and is decoded on the worker thread.
    #[test]
    fn worker_plays_a_tone_at_the_expected_level() {
        let dir = tempfile::tempdir().unwrap();
        let file = wav_tone(dir.path(), "tone.wav", 48_000, 1, 1.0);
        let out = Arc::new(Mutex::new(Vec::<f32>::new()));
        let sink = Arc::clone(&out);
        let sounds = PluginSounds::new(Box::new(move |s| {
            sink.lock().unwrap().extend_from_slice(s);
            true
        }));
        sounds.set_volume(80);
        sounds.play("p", file.clone(), 0.5);
        std::thread::sleep(Duration::from_millis(300));
        assert!(sounds.mixing());
        assert!(sounds.meter() > 0.0);
        let level = peak(&out.lock().unwrap()[CHUNK * 2..]);
        assert!((level - 0.5 * 0.8 * 0.5).abs() < 0.01, "level {level}");

        out.lock().unwrap().clear();
        sounds.observe(&EngineEvent::TalkStateChanged {
            session: 1,
            talking: true,
        });
        std::thread::sleep(Duration::from_millis(200));
        let level = peak(&out.lock().unwrap()[CHUNK * 2..]);
        assert!(
            (level - 0.2 * DUCK_GAIN).abs() < 0.01,
            "ducked level {level}"
        );

        sounds.stop_all("p");
        std::thread::sleep(Duration::from_millis(100));
        assert!(!sounds.mixing());
        assert!(sounds.meter().abs() < f32::EPSILON);
        let threads = DECODE_THREADS.lock().unwrap();
        assert!(
            threads.iter().any(|t| t == WORKER_NAME),
            "decoded on the worker"
        );
        let me = std::thread::current().name().map(str::to_owned);
        assert!(
            !threads.iter().any(|t| Some(t) == me.as_ref()),
            "never on the caller"
        );
    }
}
