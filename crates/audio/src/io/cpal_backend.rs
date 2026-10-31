//! Real audio devices via cpal.

use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{
    BufferSize, Device, DeviceId, ErrorKind, FromSample, SampleFormat, SizedSample, StreamConfig,
    SupportedBufferSize, SupportedStreamConfig,
};
use rtrb::{Consumer, Producer};

#[cfg(target_os = "linux")]
use super::DeviceMigration;
use super::controller::{Backend, Opened, StreamGuard};
use super::ring::{self, Counters};
#[cfg(target_os = "linux")]
use super::sound_server::{ID_PREFIX, SoundServer};
use super::{DeviceInfo, Direction, IoError, LOW_LATENCY_BUFFER_FRAMES};
use crate::SAMPLE_RATE;

pub(crate) struct CpalBackend {
    /// Lists and opens the devices, except on a PipeWire desktop.
    host: cpal::Host,
    /// On a PipeWire desktop: the sound server, which lists its sinks and sources as the
    /// devices; streams open on them through cpal's PipeWire host (issue #183).
    #[cfg(target_os = "linux")]
    server: Option<SoundServer>,
}

impl CpalBackend {
    pub(crate) fn new() -> Self {
        #[cfg(target_os = "linux")]
        {
            // cpal's default host would be PipeWire, whose device list is a snapshot taken when
            // the host is created; without the sound server keep the ALSA host as before.
            let server = SoundServer::connect();
            let host = cpal::host_from_id(cpal::HostId::Alsa).unwrap_or_else(|e| {
                tracing::warn!("ALSA host unavailable ({e}); using cpal's default host");
                cpal::default_host()
            });
            Self { host, server }
        }
        #[cfg(not(target_os = "linux"))]
        Self {
            host: cpal::default_host(),
        }
    }

    /// The device to open and how it is listed.
    fn open_device(
        &self,
        direction: Direction,
        id: Option<&str>,
    ) -> Result<(Device, DeviceInfo), IoError> {
        #[cfg(target_os = "linux")]
        if self.server.is_some() {
            return self.server_device(direction, id);
        }
        let device = self.device(direction, id)?;
        let info = self.info(direction, &device)?;
        Ok((device, info))
    }

    /// Opens the listed node `id` (`None`: the current default node) on a fresh PipeWire host,
    /// since a host only knows the nodes that existed when it was created.
    #[cfg(target_os = "linux")]
    fn server_device(
        &self,
        direction: Direction,
        id: Option<&str>,
    ) -> Result<(Device, DeviceInfo), IoError> {
        let listed = self.devices(direction)?;
        let info = match id {
            Some(id) => listed.into_iter().find(|d| d.id == id),
            None => listed.into_iter().find(|d| d.is_default),
        }
        .ok_or(IoError::DeviceNotFound)?;
        let host = cpal::host_from_id(cpal::HostId::PipeWire).map_err(backend_err)?;
        let device = host
            .devices()
            .map_err(backend_err)?
            .find(|d| {
                let usable = match direction {
                    Direction::Input => d.supports_input(),
                    Direction::Output => d.supports_output(),
                };
                usable && d.id().is_ok_and(|node| node.to_string() == info.id)
            })
            .ok_or(IoError::DeviceNotFound)?;
        Ok((device, info))
    }

    fn default_device(&self, direction: Direction) -> Option<Device> {
        match direction {
            Direction::Input => self.host.default_input_device(),
            Direction::Output => self.host.default_output_device(),
        }
    }

    fn device(&self, direction: Direction, id: Option<&str>) -> Result<Device, IoError> {
        match id {
            // Match the ids exactly as the device list reports them first: cpal's ALSA
            // `device_by_id` rewrites `sysdefault:CARD=X` to `sysdefault:CARD=X,DEV=0`, which
            // matches no listed device, so a saved card id would never open.
            Some(id) => self
                .listed(direction)
                .find(|d| d.id().is_ok_and(|listed| listed.to_string() == id))
                .or_else(|| {
                    DeviceId::from_str(id)
                        .ok()
                        .and_then(|parsed| self.host.device_by_id(&parsed))
                })
                .ok_or(IoError::DeviceNotFound),
            None => self
                .default_device(direction)
                .ok_or(IoError::DeviceNotFound),
        }
    }

    /// The devices for `direction` as cpal enumerates them (empty if listing fails).
    fn listed(&self, direction: Direction) -> impl Iterator<Item = Device> {
        let devices = match direction {
            Direction::Input => self.host.input_devices().map(|d| d.collect::<Vec<_>>()),
            Direction::Output => self.host.output_devices().map(|d| d.collect::<Vec<_>>()),
        };
        devices
            .inspect_err(|e| tracing::debug!("listing audio devices failed: {e}"))
            .unwrap_or_default()
            .into_iter()
    }

    fn info(&self, direction: Direction, device: &Device) -> Result<DeviceInfo, IoError> {
        let id = device.id().map_err(backend_err)?.to_string();
        let name = device
            .description()
            .map(|d| d.name().to_owned())
            .unwrap_or_else(|_| id.clone());
        let default_id = self
            .default_device(direction)
            .and_then(|d| d.id().ok())
            .map(|d| d.to_string());
        Ok(DeviceInfo {
            is_default: default_id.as_deref() == Some(id.as_str()),
            id,
            name,
        })
    }
}

impl Backend for CpalBackend {
    fn devices(&self, direction: Direction) -> Result<Vec<DeviceInfo>, IoError> {
        #[cfg(target_os = "linux")]
        if let Some(server) = &self.server {
            let (nodes, default) = server.nodes(direction)?;
            return Ok(super::devices::server_devices(
                ID_PREFIX,
                &nodes,
                default.as_deref(),
            ));
        }
        let devices: Vec<Device> = match direction {
            Direction::Input => self.host.input_devices().map_err(backend_err)?.collect(),
            Direction::Output => self.host.output_devices().map_err(backend_err)?.collect(),
        };
        // One device that cannot be described must not hide all the others.
        let infos = devices
            .iter()
            .filter_map(|d| {
                self.info(direction, d)
                    .inspect_err(|e| tracing::warn!("skipping audio device: {e}"))
                    .ok()
            })
            .collect();
        Ok(super::devices::filter(infos))
    }

    fn polls_devices(&self) -> bool {
        true
    }

    #[cfg(target_os = "linux")]
    fn migrate(&self, direction: Direction, saved: &str) -> Result<DeviceMigration, IoError> {
        match &self.server {
            Some(server) => {
                let (nodes, _) = server.nodes(direction)?;
                Ok(super::devices::migrate(saved, ID_PREFIX, &nodes))
            }
            None => Ok(DeviceMigration::Keep),
        }
    }

    fn open_input(
        &self,
        id: Option<&str>,
        counters: &Arc<Counters>,
        low_latency: bool,
    ) -> Result<Opened<Consumer<f32>>, IoError> {
        let (device, info) = self.open_device(Direction::Input, id)?;
        let supported = pick_config(&device, Direction::Input)?;
        let mut config = stream_config(&supported);
        let failed = Arc::new(AtomicBool::new(false));
        let small = low_latency
            .then(|| low_latency_buffer(&supported))
            .flatten();
        let mut stream = None;
        if let Some(frames) = small {
            let (producer, consumer) = ring::ring(config.sample_rate);
            let small_config = StreamConfig {
                buffer_size: BufferSize::Fixed(frames),
                ..config
            };
            match build_input(
                &device,
                &supported,
                small_config,
                producer,
                counters.clone(),
                failed.clone(),
            ) {
                Ok(built) => {
                    config = small_config;
                    stream = Some((built, consumer));
                }
                Err(e) => tracing::warn!(
                    frames,
                    "input device refused the low-latency buffer ({e}); using its default"
                ),
            }
        } else if low_latency {
            tracing::info!("input device reports no buffer range; using its default buffer");
        }
        let (stream, consumer) = match stream {
            Some(opened) => opened,
            None => {
                let (producer, consumer) = ring::ring(config.sample_rate);
                let built = build_input(
                    &device,
                    &supported,
                    config,
                    producer,
                    counters.clone(),
                    failed.clone(),
                )?;
                (built, consumer)
            }
        };
        stream.play().map_err(backend_err)?;
        Ok(Opened {
            guard: Box::new(CpalStream {
                _stream: stream,
                failed,
            }),
            ring: consumer,
            rate: config.sample_rate,
            device: info,
        })
    }

    fn open_output(
        &self,
        id: Option<&str>,
        counters: &Arc<Counters>,
        low_latency: bool,
    ) -> Result<Opened<Producer<f32>>, IoError> {
        let (device, info) = self.open_device(Direction::Output, id)?;
        let supported = pick_config(&device, Direction::Output)?;
        let mut config = stream_config(&supported);
        let failed = Arc::new(AtomicBool::new(false));
        let small = low_latency
            .then(|| low_latency_buffer(&supported))
            .flatten();
        let mut stream = None;
        if let Some(frames) = small {
            let (producer, consumer) = ring::stereo_ring(config.sample_rate);
            let small_config = StreamConfig {
                buffer_size: BufferSize::Fixed(frames),
                ..config
            };
            match build_output(
                &device,
                &supported,
                small_config,
                consumer,
                counters.clone(),
                failed.clone(),
            ) {
                Ok(built) => {
                    config = small_config;
                    stream = Some((built, producer));
                }
                Err(e) => tracing::warn!(
                    frames,
                    "output device refused the low-latency buffer ({e}); using its default"
                ),
            }
        } else if low_latency {
            tracing::info!("output device reports no buffer range; using its default buffer");
        }
        let (stream, producer) = match stream {
            Some(opened) => opened,
            None => {
                let (producer, consumer) = ring::stereo_ring(config.sample_rate);
                let built = build_output(
                    &device,
                    &supported,
                    config,
                    consumer,
                    counters.clone(),
                    failed.clone(),
                )?;
                (built, producer)
            }
        };
        stream.play().map_err(backend_err)?;
        Ok(Opened {
            guard: Box::new(CpalStream {
                _stream: stream,
                failed,
            }),
            ring: producer,
            rate: config.sample_rate,
            device: info,
        })
    }
}

struct CpalStream {
    _stream: cpal::Stream,
    failed: Arc<AtomicBool>,
}

impl StreamGuard for CpalStream {
    fn failed(&self) -> bool {
        self.failed.load(Ordering::Relaxed)
    }
}

/// The device's default config, but at 48 kHz when the device supports it
/// in the same sample format, so no resampling is needed.
fn pick_config(device: &Device, direction: Direction) -> Result<SupportedStreamConfig, IoError> {
    let (default, ranges) = match direction {
        Direction::Input => (
            device.default_input_config().map_err(backend_err)?,
            device
                .supported_input_configs()
                .map_err(backend_err)?
                .collect::<Vec<_>>(),
        ),
        Direction::Output => (
            device.default_output_config().map_err(backend_err)?,
            device
                .supported_output_configs()
                .map_err(backend_err)?
                .collect::<Vec<_>>(),
        ),
    };
    if default.sample_rate() == SAMPLE_RATE {
        return Ok(default);
    }
    let at_48k = ranges
        .into_iter()
        .filter(|r| r.sample_format() == default.sample_format())
        .filter_map(|r| r.try_with_sample_rate(SAMPLE_RATE))
        .min_by_key(|c| c.channels().abs_diff(default.channels()));
    Ok(at_48k.unwrap_or(default))
}

/// The low-latency buffer (issue #156) in device frames, or `None` when the device does not
/// report a range: forcing a size it may not support could fail late or glitch, so such
/// devices keep the host default. Hosts that map the period to their own graph (PipeWire via
/// ALSA, CoreAudio's buffer frame size, WASAPI) take the request as a quantum hint.
fn low_latency_buffer(supported: &SupportedStreamConfig) -> Option<u32> {
    match *supported.buffer_size() {
        SupportedBufferSize::Range { min, max } if min <= max => {
            Some(low_latency_frames(supported.sample_rate(), min, max))
        }
        _ => None,
    }
}

/// [`LOW_LATENCY_BUFFER_FRAMES`] at 48 kHz, scaled to `rate` and clamped to `min..=max`.
fn low_latency_frames(rate: u32, min: u32, max: u32) -> u32 {
    let wanted = u64::from(LOW_LATENCY_BUFFER_FRAMES) * u64::from(rate) / u64::from(SAMPLE_RATE);
    u32::try_from(wanted).unwrap_or(u32::MAX).clamp(min, max)
}

fn stream_config(supported: &SupportedStreamConfig) -> StreamConfig {
    StreamConfig {
        channels: supported.channels(),
        sample_rate: supported.sample_rate(),
        // The host default is the most robust choice across backends.
        buffer_size: BufferSize::Default,
    }
}

/// Error callbacks run on the audio thread: only flag the failure, the
/// control thread logs it and reopens the stream. `DeviceChanged` means the
/// host already moved the stream to the new default device, so it keeps running.
/// An xrun (the callback ran too late, e.g. under heavy CPU load) is only counted: the host has
/// already restarted the stream, and reopening it would turn one click into a long dropout.
/// So is a refused real-time request: the stream runs on at normal priority.
fn error_flag(
    failed: Arc<AtomicBool>,
    counters: Arc<Counters>,
) -> impl FnMut(cpal::Error) + Send + 'static {
    move |err| match err.kind() {
        ErrorKind::Xrun => counters.device_xrun(),
        ErrorKind::RealtimeDenied => counters.realtime_denied(),
        ErrorKind::DeviceChanged => {}
        _ => failed.store(true, Ordering::Relaxed),
    }
}

macro_rules! dispatch_format {
    ($format:expr, $build:ident, $($args:expr),*) => {
        match $format {
            SampleFormat::F32 => $build::<f32>($($args),*),
            SampleFormat::F64 => $build::<f64>($($args),*),
            SampleFormat::I16 => $build::<i16>($($args),*),
            SampleFormat::U16 => $build::<u16>($($args),*),
            SampleFormat::I32 => $build::<i32>($($args),*),
            SampleFormat::U32 => $build::<u32>($($args),*),
            SampleFormat::I8 => $build::<i8>($($args),*),
            SampleFormat::U8 => $build::<u8>($($args),*),
            other => Err(IoError::UnsupportedFormat(other.to_string())),
        }
    };
}

fn build_input(
    device: &Device,
    supported: &SupportedStreamConfig,
    config: StreamConfig,
    producer: Producer<f32>,
    counters: Arc<Counters>,
    failed: Arc<AtomicBool>,
) -> Result<cpal::Stream, IoError> {
    // cpal moves its callback threads to real-time priority (`realtime-dbus`).
    crate::rt::guard_overruns();
    dispatch_format!(
        supported.sample_format(),
        input_stream,
        device,
        config,
        producer,
        counters,
        failed
    )
}

fn input_stream<T>(
    device: &Device,
    config: StreamConfig,
    mut producer: Producer<f32>,
    counters: Arc<Counters>,
    failed: Arc<AtomicBool>,
) -> Result<cpal::Stream, IoError>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let channels = usize::from(config.channels);
    device
        .build_input_stream(
            config,
            {
                let counters = counters.clone();
                move |data: &[T], _: &cpal::InputCallbackInfo| {
                    ring::capture(data, channels, &mut producer, &counters);
                }
            },
            error_flag(failed, counters),
            None,
        )
        .map_err(backend_err)
}

fn build_output(
    device: &Device,
    supported: &SupportedStreamConfig,
    config: StreamConfig,
    consumer: Consumer<f32>,
    counters: Arc<Counters>,
    failed: Arc<AtomicBool>,
) -> Result<cpal::Stream, IoError> {
    // cpal moves its callback threads to real-time priority (`realtime-dbus`).
    crate::rt::guard_overruns();
    dispatch_format!(
        supported.sample_format(),
        output_stream,
        device,
        config,
        consumer,
        counters,
        failed
    )
}

fn output_stream<T>(
    device: &Device,
    config: StreamConfig,
    mut consumer: Consumer<f32>,
    counters: Arc<Counters>,
    failed: Arc<AtomicBool>,
) -> Result<cpal::Stream, IoError>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = usize::from(config.channels);
    device
        .build_output_stream(
            config,
            {
                let counters = counters.clone();
                move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                    ring::playback(data, channels, &mut consumer, &counters);
                }
            },
            error_flag(failed, counters),
            None,
        )
        .map_err(backend_err)
}

fn backend_err(err: impl std::fmt::Display) -> IoError {
    IoError::Backend(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_real_failures_reopen_the_stream() {
        let failed = Arc::new(AtomicBool::new(false));
        let counters = Arc::new(Counters::default());
        let mut on_error = error_flag(failed.clone(), counters.clone());
        on_error(cpal::Error::new(ErrorKind::Xrun));
        on_error(cpal::Error::new(ErrorKind::RealtimeDenied));
        on_error(cpal::Error::new(ErrorKind::DeviceChanged));
        assert!(!failed.load(Ordering::Relaxed), "the stream keeps running");
        let stats = counters.snapshot();
        assert_eq!((stats.device_xruns, stats.realtime_denied), (1, 1));

        on_error(cpal::Error::new(ErrorKind::DeviceNotAvailable));
        assert!(failed.load(Ordering::Relaxed), "a lost device is reopened");
    }

    #[test]
    fn low_latency_buffer_scales_and_clamps_to_the_device_range() {
        assert_eq!(low_latency_frames(48_000, 16, 8192), 128);
        assert_eq!(low_latency_frames(96_000, 16, 8192), 256);
        assert_eq!(low_latency_frames(44_100, 16, 8192), 117);
        // A device that cannot go that low gets its smallest buffer, never less.
        assert_eq!(low_latency_frames(48_000, 256, 8192), 256);
        assert_eq!(low_latency_frames(48_000, 16, 64), 64);
    }
}
