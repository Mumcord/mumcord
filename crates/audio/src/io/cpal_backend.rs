//! Real audio devices via cpal.

use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{
    BufferSize, Device, DeviceId, ErrorKind, FromSample, SampleFormat, SizedSample, StreamConfig,
    SupportedStreamConfig,
};
use rtrb::{Consumer, Producer};

use super::controller::{Backend, Opened, StreamGuard};
use super::ring::{self, Counters};
use super::{DeviceInfo, Direction, IoError};
use crate::SAMPLE_RATE;

pub(crate) struct CpalBackend {
    host: cpal::Host,
}

impl CpalBackend {
    pub(crate) fn new() -> Self {
        Self {
            host: cpal::default_host(),
        }
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
        let devices: Vec<Device> = match direction {
            Direction::Input => self.host.input_devices().map_err(backend_err)?.collect(),
            Direction::Output => self.host.output_devices().map_err(backend_err)?.collect(),
        };
        let infos = devices
            .iter()
            .map(|d| self.info(direction, d))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(super::devices::filter(infos))
    }

    fn polls_devices(&self) -> bool {
        true
    }

    fn open_input(
        &self,
        id: Option<&str>,
        counters: &Arc<Counters>,
    ) -> Result<Opened<Consumer<f32>>, IoError> {
        let device = self.device(Direction::Input, id)?;
        let info = self.info(Direction::Input, &device)?;
        let supported = pick_config(&device, Direction::Input)?;
        let config = stream_config(&supported);
        let (producer, consumer) = ring::ring(config.sample_rate);
        let failed = Arc::new(AtomicBool::new(false));
        let stream = build_input(
            &device,
            &supported,
            config,
            producer,
            counters.clone(),
            failed.clone(),
        )?;
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
    ) -> Result<Opened<Producer<f32>>, IoError> {
        let device = self.device(Direction::Output, id)?;
        let info = self.info(Direction::Output, &device)?;
        let supported = pick_config(&device, Direction::Output)?;
        let config = stream_config(&supported);
        let (producer, consumer) = ring::stereo_ring(config.sample_rate);
        let failed = Arc::new(AtomicBool::new(false));
        let stream = build_output(
            &device,
            &supported,
            config,
            consumer,
            counters.clone(),
            failed.clone(),
        )?;
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
fn error_flag(failed: Arc<AtomicBool>) -> impl FnMut(cpal::Error) + Send + 'static {
    move |err| {
        if err.kind() != ErrorKind::DeviceChanged {
            failed.store(true, Ordering::Relaxed);
        }
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
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                ring::capture(data, channels, &mut producer, &counters);
            },
            error_flag(failed),
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
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                ring::playback(data, channels, &mut consumer, &counters);
            },
            error_flag(failed),
            None,
        )
        .map_err(backend_err)
}

fn backend_err(err: impl std::fmt::Display) -> IoError {
    IoError::Backend(err.to_string())
}
