//! Platform-fed backend ([`BackendKind::External`](super::BackendKind::External)): the embedding
//! app owns audio capture and playback and moves the samples itself.
//!
//! Phones need this (Epic 5, FR-5.25): only the system's voice-communication audio session gives
//! hardware echo cancellation, Bluetooth hands-free routing and call integration, and cpal cannot
//! ask for it. It works on every target, so it is testable on the desktop host.
//!
//! # Contract for the host
//!
//! [`external`] returns three parts: [`ExternalAudio`] goes into the engine (as
//! `BackendKind::External`), [`ExternalCapture`] is called from the host's microphone callback
//! and [`ExternalPlayback`] from its speaker callback.
//!
//! - **Format**: interleaved PCM, `f32` in −1.0..=1.0 or `i16`. Sample rate and channel count
//!   are fixed per direction when [`external`] is called ([`ExternalFormat`]). 48 kHz needs no
//!   conversion; other rates are resampled on the engine's own threads, never in the callback.
//!   A hardware rate change (a Bluetooth headset connecting) has to be absorbed by the platform
//!   audio session, which converts to the format the app asked for.
//! - **Channels**: captured channels are mixed down to mono. Playback delivers the mix on one
//!   channel, left and right on two, and the mix on every further channel.
//! - **Block size**: any number of frames per call, up to [`MAX_BLOCK_MS`] of audio. Slices
//!   whose length is not a multiple of the channel count lose the incomplete last frame.
//! - **Real time**: `push_*` and `pull_*` never block, allocate, free, lock, log or do I/O.
//! - **Threads**: each endpoint has one caller at a time (`&mut self`); the two endpoints may be
//!   called from different threads, and the thread may change between calls.
//! - **Overrun**: captured samples that do not fit the queue (the engine reads too slowly) are
//!   dropped and counted in [`IoStats::capture_overruns`](super::IoStats).
//! - **Underrun**: `pull_*` always fills the whole buffer; what the engine has not delivered is
//!   silence, counted in [`IoStats::playback_underruns`](super::IoStats) when it cuts into sound.
//! - **Start and stop**: while the engine's audio is not running, pushed audio is discarded and
//!   pulled audio is silence. The host may start calling before or after the engine starts.
//! - **Interruption**: when the host stops calling (the audio session was interrupted by a phone
//!   call), nothing blocks: the send path idles and the playback queue stays full. When
//!   `pull_*` is called again after more than [`STALL`], the audio queued before the
//!   interruption is discarded instead of being played as a late burst.
//! - **Clock drift**: capture and playback need no common clock. Captured audio paces the send
//!   path, and the mixer paces itself on how much playback is queued, as with device backends.

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use dasp_sample::{FromSample, Sample};
use rtrb::{Consumer, Producer, RingBuffer};

use super::controller::{Backend, Opened, StreamGuard};
use super::ring::{self, Counters};
use super::{DeviceInfo, Direction, IoError};

/// Longest block one `push_*` or `pull_*` call may carry, in milliseconds of audio.
pub const MAX_BLOCK_MS: u32 = 100;
/// A pause between two `pull_*` calls longer than this counts as an interruption: what was
/// queued before it is stale and dropped. Well above [`MAX_BLOCK_MS`], the longest regular gap.
pub const STALL: Duration = Duration::from_millis(250);

/// Id of the one logical device the external backend lists per direction.
pub const EXTERNAL_DEVICE_ID: &str = "external";

const MIN_RATE: u32 = 8_000;
const MAX_RATE: u32 = 192_000;
/// Streams the engine may open while the host is not calling; a further open waits (it is
/// retried) until the host calls again.
const PENDING_LINKS: usize = 2;
/// Old streams waiting to be freed by the engine: the host hands back at most the one it used
/// plus the pending ones.
const RETIRED_LINKS: usize = PENDING_LINKS + 2;

/// PCM layout of one direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExternalFormat {
    /// Frames per second, 8 000 to 192 000.
    pub sample_rate: u32,
    /// Interleaved channels per frame, at least 1.
    pub channels: u16,
}

impl ExternalFormat {
    fn check(self) -> Result<Self, IoError> {
        if !(MIN_RATE..=MAX_RATE).contains(&self.sample_rate) {
            return Err(IoError::UnsupportedFormat(format!(
                "sample rate {} Hz",
                self.sample_rate
            )));
        }
        if self.channels == 0 {
            return Err(IoError::UnsupportedFormat("no channels".to_owned()));
        }
        Ok(self)
    }
}

/// Creates the external backend: the engine's part and the host's two endpoints.
pub fn external(
    capture: ExternalFormat,
    playback: ExternalFormat,
) -> Result<(ExternalAudio, ExternalCapture, ExternalPlayback), IoError> {
    let (capture, playback) = (capture.check()?, playback.check()?);
    let (capture_port, capture_end) = handoff();
    let (playback_port, playback_end) = handoff();
    Ok((
        ExternalAudio(Arc::new(Shared {
            capture_format: capture,
            playback_format: playback,
            capture: Mutex::new(capture_port),
            playback: Mutex::new(playback_port),
        })),
        ExternalCapture {
            end: capture_end,
            channels: usize::from(capture.channels),
        },
        ExternalPlayback {
            end: playback_end,
            channels: usize::from(playback.channels),
            last_pull: None,
        },
    ))
}

/// The engine's part of the external backend; cheap to clone. Two values are equal when they
/// belong to the same [`external`] call.
#[derive(Clone)]
pub struct ExternalAudio(Arc<Shared>);

impl fmt::Debug for ExternalAudio {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExternalAudio")
            .field("capture", &self.0.capture_format)
            .field("playback", &self.0.playback_format)
            .finish()
    }
}

impl PartialEq for ExternalAudio {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for ExternalAudio {}

struct Shared {
    capture_format: ExternalFormat,
    playback_format: ExternalFormat,
    /// Locked by the engine's control thread only, never by the host endpoints.
    capture: Mutex<Port<Producer<f32>>>,
    playback: Mutex<Port<Consumer<f32>>>,
}

/// One opened stream as the host endpoint sees it: its end of the sample ring.
struct Link<R> {
    ring: R,
    counters: Arc<Counters>,
    /// Cleared when the engine stops the stream.
    open: Arc<AtomicBool>,
}

/// Engine side of the handoff: sends new streams to the host endpoint and takes back the ones
/// it replaced, so that freeing them happens here and never in the host's callback.
struct Port<R> {
    links: Producer<Link<R>>,
    retired: Consumer<Link<R>>,
}

/// Host side of the handoff.
struct HostEnd<R> {
    links: Consumer<Link<R>>,
    retired: Producer<Link<R>>,
    current: Option<Link<R>>,
}

fn handoff<R>() -> (Port<R>, HostEnd<R>) {
    let (links_tx, links_rx) = RingBuffer::new(PENDING_LINKS);
    let (retired_tx, retired_rx) = RingBuffer::new(RETIRED_LINKS);
    (
        Port {
            links: links_tx,
            retired: retired_rx,
        },
        HostEnd {
            links: links_rx,
            retired: retired_tx,
            current: None,
        },
    )
}

impl<R> Port<R> {
    /// Publishes a new stream to the host endpoint and frees the streams it gave back.
    fn publish(
        &mut self,
        ring: R,
        counters: &Arc<Counters>,
    ) -> Result<Box<dyn StreamGuard>, IoError> {
        while self.retired.pop().is_ok() {}
        let open = Arc::new(AtomicBool::new(true));
        self.links
            .push(Link {
                ring,
                counters: counters.clone(),
                open: open.clone(),
            })
            .map_err(|_| IoError::Backend("the audio host is not running".to_owned()))?;
        Ok(Box::new(Guard { open }))
    }
}

impl<R> HostEnd<R> {
    /// The stream to use now, after switching to the newest one the engine opened. Wait-free;
    /// a replaced stream goes back to the engine, and while there is no room to hand one back
    /// the switch waits for a later call, so nothing is ever freed here.
    fn current(&mut self) -> Option<&mut Link<R>> {
        while self.current.is_none() || !self.retired.is_full() {
            let Ok(link) = self.links.pop() else { break };
            if let Some(old) = self.current.replace(link) {
                let handed_back = self.retired.push(old).is_ok();
                debug_assert!(handed_back, "room was checked before the switch");
            }
        }
        self.current
            .as_mut()
            .filter(|link| link.open.load(Ordering::Relaxed))
    }
}

struct Guard {
    open: Arc<AtomicBool>,
}

impl StreamGuard for Guard {
    fn failed(&self) -> bool {
        false
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        self.open.store(false, Ordering::Relaxed);
    }
}

/// The host's microphone side: call `push_*` with every captured block.
pub struct ExternalCapture {
    end: HostEnd<Producer<f32>>,
    channels: usize,
}

impl ExternalCapture {
    /// Hands captured interleaved `f32` samples to the engine. Real-time safe.
    pub fn push_f32(&mut self, interleaved: &[f32]) {
        self.push(interleaved);
    }

    /// Hands captured interleaved `i16` samples to the engine. Real-time safe.
    pub fn push_i16(&mut self, interleaved: &[i16]) {
        self.push(interleaved);
    }

    fn push<T>(&mut self, interleaved: &[T])
    where
        T: Sample,
        f32: FromSample<T>,
    {
        if let Some(link) = self.end.current() {
            ring::capture(interleaved, self.channels, &mut link.ring, &link.counters);
        }
    }
}

/// The host's speaker side: call `pull_*` for every block the system wants to play.
pub struct ExternalPlayback {
    end: HostEnd<Consumer<f32>>,
    channels: usize,
    last_pull: Option<Instant>,
}

impl ExternalPlayback {
    /// Fills `interleaved` with the engine's playback as `f32` samples. Real-time safe.
    pub fn pull_f32(&mut self, interleaved: &mut [f32]) {
        self.pull(interleaved, Instant::now());
    }

    /// Fills `interleaved` with the engine's playback as `i16` samples. Real-time safe.
    pub fn pull_i16(&mut self, interleaved: &mut [i16]) {
        self.pull(interleaved, Instant::now());
    }

    fn pull<T>(&mut self, interleaved: &mut [T], now: Instant)
    where
        T: Sample + FromSample<f32>,
    {
        // The first call and a call after an interruption find audio from before the pause.
        let stalled = self
            .last_pull
            .is_none_or(|last| now.saturating_duration_since(last) > STALL);
        self.last_pull = Some(now);
        match self.end.current() {
            Some(link) => {
                if stalled {
                    let queued = link.ring.slots();
                    if let Ok(stale) = link.ring.read_chunk(queued) {
                        stale.commit_all();
                    }
                }
                ring::playback(interleaved, self.channels, &mut link.ring, &link.counters);
            }
            None => interleaved.fill(T::EQUILIBRIUM),
        }
    }
}

impl ExternalAudio {
    fn device(direction: Direction) -> DeviceInfo {
        DeviceInfo {
            id: EXTERNAL_DEVICE_ID.to_owned(),
            name: match direction {
                Direction::Input => "System microphone".to_owned(),
                Direction::Output => "System audio output".to_owned(),
            },
            is_default: true,
        }
    }
}

/// The system chooses microphone and speaker, so there is one logical device per direction and
/// a saved device id is ignored.
impl Backend for ExternalAudio {
    fn devices(&self, direction: Direction) -> Result<Vec<DeviceInfo>, IoError> {
        Ok(vec![Self::device(direction)])
    }

    fn polls_devices(&self) -> bool {
        false
    }

    fn open_input(
        &self,
        _id: Option<&str>,
        counters: &Arc<Counters>,
        _low_latency: bool,
    ) -> Result<Opened<Consumer<f32>>, IoError> {
        let rate = self.0.capture_format.sample_rate;
        let (producer, consumer) = ring::ring(rate);
        let guard = self
            .0
            .capture
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .publish(producer, counters)?;
        Ok(Opened {
            guard,
            ring: consumer,
            rate,
            device: Self::device(Direction::Input),
        })
    }

    fn open_output(
        &self,
        _id: Option<&str>,
        counters: &Arc<Counters>,
        _low_latency: bool,
    ) -> Result<Opened<Producer<f32>>, IoError> {
        let rate = self.0.playback_format.sample_rate;
        let (producer, consumer) = ring::stereo_ring(rate);
        let guard = self
            .0
            .playback
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .publish(consumer, counters)?;
        Ok(Opened {
            guard,
            ring: producer,
            rate,
            device: Self::device(Direction::Output),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::{AudioHandles, AudioIo, BackendKind, DeviceEvent, FrameSink, FrameSource};
    use crate::{FRAME_SAMPLES, Frame, SAMPLE_RATE};

    const MONO_48K: ExternalFormat = ExternalFormat {
        sample_rate: SAMPLE_RATE,
        channels: 1,
    };
    const STEREO_48K: ExternalFormat = ExternalFormat {
        sample_rate: SAMPLE_RATE,
        channels: 2,
    };

    struct Rig {
        handles: AudioHandles,
        capture: ExternalCapture,
        playback: ExternalPlayback,
    }

    fn start(capture: ExternalFormat, playback: ExternalFormat) -> Rig {
        let (audio, capture, playback) = external(capture, playback).expect("external");
        let handles = AudioIo::start_with(BackendKind::External(audio), None, None).expect("start");
        wait_active(&handles, 2);
        Rig {
            handles,
            capture,
            playback,
        }
    }

    fn wait_active(handles: &AudioHandles, count: usize) {
        for _ in 0..count {
            loop {
                let event = handles
                    .events
                    .recv_timeout(Duration::from_secs(5))
                    .expect("stream start");
                if matches!(event, DeviceEvent::Active { .. }) {
                    break;
                }
            }
        }
    }

    fn sine(frames: usize, rate: u32, offset: usize) -> Vec<f32> {
        (0..frames)
            .map(|i| {
                let t = (offset + i) as f32 / rate as f32;
                0.5 * (std::f32::consts::TAU * 440.0 * t).sin()
            })
            .collect()
    }

    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len().max(1) as f32).sqrt()
    }

    fn read_frames(source: &mut FrameSource, count: usize) -> Vec<f32> {
        let mut out = Vec::new();
        let mut frame: Frame = [0.0; FRAME_SAMPLES];
        for _ in 0..count {
            assert!(source.read_frame(&mut frame), "a captured frame is ready");
            out.extend_from_slice(&frame);
        }
        out
    }

    fn write_level(sink: &mut FrameSink, level: f32) {
        assert!(sink.write_frame(&[level; FRAME_SAMPLES]), "frame queued");
    }

    #[test]
    fn rejects_unusable_formats() {
        let bad_rate = ExternalFormat {
            sample_rate: 4_000,
            channels: 1,
        };
        let no_channels = ExternalFormat {
            sample_rate: SAMPLE_RATE,
            channels: 0,
        };
        assert!(external(bad_rate, MONO_48K).is_err());
        assert!(external(MONO_48K, no_channels).is_err());
    }

    #[test]
    fn lists_one_device_per_direction() {
        let (audio, _capture, _playback) = external(MONO_48K, MONO_48K).expect("external");
        for direction in [Direction::Input, Direction::Output] {
            let devices =
                AudioIo::list_devices_with(BackendKind::External(audio.clone()), direction)
                    .expect("devices");
            assert_eq!(devices.len(), 1);
            assert_eq!(devices[0].id, EXTERNAL_DEVICE_ID);
            assert!(devices[0].is_default);
        }
    }

    #[test]
    fn idle_endpoints_discard_and_play_silence() {
        let (_audio, mut capture, mut playback) = external(MONO_48K, MONO_48K).expect("external");
        capture.push_f32(&[0.5; 480]);
        let mut out = [1.0f32; 480];
        playback.pull_f32(&mut out);
        assert!(out.iter().all(|&s| s == 0.0));
        let mut out = [7i16; 480];
        playback.pull_i16(&mut out);
        assert!(out.iter().all(|&s| s == 0));
    }

    #[test]
    fn captured_f32_arrives_unchanged() {
        let mut rig = start(MONO_48K, MONO_48K);
        let tone = sine(2 * FRAME_SAMPLES, SAMPLE_RATE, 0);
        // Odd block sizes, as a system callback may deliver them.
        for block in tone.chunks(137) {
            rig.capture.push_f32(block);
        }
        assert_eq!(read_frames(&mut rig.handles.source, 2), tone);
        let mut frame: Frame = [0.0; FRAME_SAMPLES];
        assert!(!rig.handles.source.read_frame(&mut frame), "nothing left");
    }

    #[test]
    fn captured_i16_stereo_is_mixed_down() {
        let mut rig = start(STEREO_48K, MONO_48K);
        let block: Vec<i16> = (0..FRAME_SAMPLES).flat_map(|_| [i16::MAX, 0]).collect();
        rig.capture.push_i16(&block);
        let frame = read_frames(&mut rig.handles.source, 1);
        assert!(frame.iter().all(|&s| (s - 0.5).abs() < 0.001));
    }

    #[test]
    fn capture_overrun_drops_and_counts() {
        let mut rig = start(MONO_48K, MONO_48K);
        // 150 ms into a 100 ms queue that nobody reads.
        for _ in 0..15 {
            rig.capture.push_f32(&[0.25; FRAME_SAMPLES]);
        }
        assert!(rig.handles.io.stats().capture_overruns >= 1);
        assert_eq!(
            rig.handles.source.queued_samples(),
            SAMPLE_RATE as usize / 10
        );
    }

    #[test]
    fn playback_reaches_every_channel_layout() {
        let mut rig = start(MONO_48K, STEREO_48K);
        let mut warm_up = [0.0f32; 2];
        rig.playback.pull_f32(&mut warm_up);
        let (left, right) = ([0.5; FRAME_SAMPLES], [-0.25; FRAME_SAMPLES]);
        assert!(rig.handles.sink.write_stereo(&left, &right));
        let mut out = [0i16; 2 * FRAME_SAMPLES];
        rig.playback.pull_i16(&mut out);
        for (index, &sample) in out.iter().enumerate() {
            let expected = if index % 2 == 0 {
                i32::from(i16::MAX) / 2
            } else {
                -i32::from(i16::MAX) / 4
            };
            assert!((i32::from(sample) - expected).abs() <= 1);
        }
        assert_eq!(rig.handles.sink.device_period_samples(), FRAME_SAMPLES);
    }

    #[test]
    fn playback_underrun_is_silence_and_counted() {
        let mut rig = start(MONO_48K, MONO_48K);
        let mut out = [0.0f32; 2 * FRAME_SAMPLES];
        rig.playback.pull_f32(&mut out[..1]);
        write_level(&mut rig.handles.sink, 0.5);
        rig.playback.pull_f32(&mut out);
        assert!(out[..FRAME_SAMPLES].iter().all(|&s| s == 0.5));
        assert!(out[FRAME_SAMPLES..].iter().all(|&s| s == 0.0));
        assert_eq!(rig.handles.io.stats().playback_underruns, 1);
        // An empty queue is "nobody talks", not an underrun.
        rig.playback.pull_f32(&mut out);
        assert_eq!(rig.handles.io.stats().playback_underruns, 1);
    }

    #[test]
    fn audio_queued_before_an_interruption_is_not_played_late() {
        let mut rig = start(MONO_48K, MONO_48K);
        let mut out = [0.0f32; FRAME_SAMPLES];
        let start = Instant::now();

        // Queued before the host's first callback: stale as well.
        write_level(&mut rig.handles.sink, 0.9);
        rig.playback.pull(&mut out, start);
        assert!(out.iter().all(|&s| s == 0.0), "audio from before the start");

        write_level(&mut rig.handles.sink, 0.5);
        rig.playback
            .pull(&mut out, start + Duration::from_millis(10));
        assert!(out.iter().all(|&s| s == 0.5), "regular playback");

        // The host stops pulling (phone call); the mixer had queued more by then.
        write_level(&mut rig.handles.sink, 0.9);
        write_level(&mut rig.handles.sink, 0.9);
        let resumed = start + Duration::from_secs(30);
        rig.playback.pull(&mut out, resumed);
        assert!(out.iter().all(|&s| s == 0.0), "stale audio was played");
        assert_eq!(rig.handles.sink.queued_samples(), 0);
        assert_eq!(
            rig.handles.io.stats().playback_underruns,
            0,
            "dropping stale audio is no underrun"
        );

        write_level(&mut rig.handles.sink, 0.25);
        rig.playback
            .pull(&mut out, resumed + Duration::from_millis(10));
        assert!(
            out.iter().all(|&s| s == 0.25),
            "fresh audio after the pause"
        );
    }

    #[test]
    fn other_rates_are_resampled_outside_the_callbacks() {
        let format = ExternalFormat {
            sample_rate: 44_100,
            channels: 1,
        };
        let mut rig = start(format, format);

        // Capture: 300 ms of a 44.1 kHz tone arrives as 48 kHz frames at the same level.
        let mut captured = Vec::new();
        let mut frame: Frame = [0.0; FRAME_SAMPLES];
        for block in 0..30 {
            rig.capture.push_f32(&sine(441, 44_100, block * 441));
            while rig.handles.source.read_frame(&mut frame) {
                captured.extend_from_slice(&frame);
            }
        }
        assert!(captured.len() >= 25 * FRAME_SAMPLES);
        let level = rms(&captured[FRAME_SAMPLES..]);
        assert!((level - 0.354).abs() < 0.02, "capture level {level}");

        // Playback: 300 ms of 48 kHz frames come out as 44.1 kHz at the same level.
        let mut played = Vec::new();
        let mut out = [0.0f32; 441];
        rig.playback.pull_f32(&mut out);
        for block in 0..30 {
            let tone = sine(FRAME_SAMPLES, SAMPLE_RATE, block * FRAME_SAMPLES);
            let frame: Frame = tone.try_into().expect("frame");
            assert!(rig.handles.sink.write_frame(&frame));
            rig.playback.pull_f32(&mut out);
            played.extend_from_slice(&out);
        }
        let level = rms(&played[2 * 441..]);
        assert!((level - 0.354).abs() < 0.03, "playback level {level}");
    }

    #[test]
    fn endpoints_keep_working_across_a_reopen() {
        let mut rig = start(MONO_48K, MONO_48K);
        let mut out = [0.0f32; FRAME_SAMPLES];
        rig.capture.push_f32(&[0.5; FRAME_SAMPLES]);
        rig.playback.pull_f32(&mut out);

        // Low-latency mode reopens both streams, here twice.
        for on in [true, false] {
            rig.handles.io.set_low_latency(on).expect("reopen");
            wait_active(&rig.handles, 2);
            let mut frame: Frame = [0.0; FRAME_SAMPLES];
            while rig.handles.source.read_frame(&mut frame) {}

            rig.capture.push_f32(&[0.25; FRAME_SAMPLES]);
            let frame = read_frames(&mut rig.handles.source, 1);
            assert!(frame.iter().all(|&s| s == 0.25));

            rig.playback.pull_f32(&mut out);
            write_level(&mut rig.handles.sink, 0.75);
            rig.playback.pull_f32(&mut out);
            assert!(out.iter().all(|&s| s == 0.75));
        }
    }

    #[test]
    fn a_host_that_is_not_calling_delays_reopens_without_blocking() {
        let (audio, mut capture, _playback) = external(MONO_48K, MONO_48K).expect("external");
        let counters = Arc::new(Counters::default());
        let open = || audio.open_input(None, &counters, false);
        let first = open().expect("first open");
        let second = open().expect("second open");
        assert!(open().is_err(), "a third stream has to wait for the host");

        // The host calls again and switches to the newest stream; the engine can open again.
        drop(first.guard);
        capture.push_f32(&[0.5; 480]);
        let mut newest = second.ring;
        assert_eq!(newest.slots(), 480);
        let third = open().expect("open after the host called");
        capture.push_f32(&[0.5; 480]);
        assert_eq!(third.ring.slots(), 480);
        if let Ok(chunk) = newest.read_chunk(480) {
            chunk.commit_all();
        }
        assert_eq!(newest.slots(), 0, "the replaced stream gets nothing more");
    }

    #[test]
    fn stopping_the_engine_side_silences_the_host_endpoints() {
        let mut rig = start(MONO_48K, MONO_48K);
        let mut out = [0.0f32; FRAME_SAMPLES];
        rig.playback.pull_f32(&mut out);
        write_level(&mut rig.handles.sink, 0.5);
        drop(rig.handles);
        rig.capture.push_f32(&[0.5; FRAME_SAMPLES]);
        rig.playback.pull_f32(&mut out);
        assert!(out.iter().all(|&s| s == 0.0));
    }
}
