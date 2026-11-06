//! V4L2 directly on `/dev/video*`, the fallback when there is no camera portal (FR-8.25).
//!
//! There is no hot-plug notification here; the caller polls [`V4l2::devices`] every second.
//! Polling stays cheap because the capabilities of a `/dev/video*` node are only queried again
//! when the set of nodes changed.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use v4l::buffer::Type;
use v4l::capability::Flags;
use v4l::frameinterval::FrameIntervalEnum;
use v4l::framesize::FrameSizeEnum;
use v4l::io::mmap::Stream;
use v4l::io::traits::CaptureStream;
use v4l::video::Capture;
use v4l::{Device, Format, FourCC};

use super::ranges;
use crate::mode::{self, Geometry, Mode, PixelFormat, Rate};
use crate::pump::{Pump, Raw};
use crate::{CameraDevice, CameraError, CaptureRequest, CaptureSession, DeviceId, FrameSink};

/// Backend name in [`DeviceId::from_os_key`].
const BACKEND: &str = "v4l2";

/// Driver buffers; one is with Mumcord while the driver fills the others.
const DRIVER_BUFFERS: u32 = 4;

/// How long the first picture may take after streaming starts (camera power-up, exposure).
const FIRST_FRAME_TIMEOUT: Duration = Duration::from_secs(5);

/// How often the capture thread checks for a stop while no picture arrives.
const STOP_POLL_MS: i32 = 200;

/// One capture node found by the last scan.
#[derive(Clone)]
struct Node {
    path: PathBuf,
    id: DeviceId,
    name: String,
}

#[derive(Default)]
struct Scan {
    /// The `/dev/video*` paths of the last scan, sorted; capture nodes are only re-read when
    /// this changes.
    paths: Vec<PathBuf>,
    nodes: Vec<Node>,
}

#[derive(Default)]
pub struct V4l2 {
    scan: Mutex<Scan>,
}

impl V4l2 {
    pub fn devices(&self) -> Vec<CameraDevice> {
        let mut paths: Vec<PathBuf> = v4l::context::enum_devices()
            .into_iter()
            .map(|node| node.path().to_owned())
            .collect();
        paths.sort();
        let mut scan = self.scan.lock().unwrap_or_else(PoisonError::into_inner);
        if scan.paths != paths {
            let by_id = stable_names();
            scan.nodes = paths
                .iter()
                .filter_map(|path| capture_node(path, &by_id))
                .collect();
            scan.paths = paths;
        }
        scan.nodes
            .iter()
            .enumerate()
            .map(|(index, node)| CameraDevice {
                id: node.id.clone(),
                name: node.name.clone(),
                is_default: index == 0,
            })
            .collect()
    }

    pub fn open(
        &self,
        device: &DeviceId,
        request: CaptureRequest,
        sink: Box<dyn FrameSink>,
    ) -> Result<Box<dyn CaptureSession>, CameraError> {
        // A rescan picks up a camera plugged in since the last poll.
        self.devices();
        let path = {
            let scan = self.scan.lock().unwrap_or_else(PoisonError::into_inner);
            scan.nodes
                .iter()
                .find(|node| &node.id == device)
                .map(|node| node.path.clone())
                .ok_or(CameraError::NoCamera)?
        };
        let device = Device::with_path(&path).map_err(open_error)?;
        let candidates = mode::candidates(&modes(&device), &request);
        let mode = *candidates.first().ok_or_else(|| {
            CameraError::Backend("the camera offers no usable picture format".to_owned())
        })?;
        let geometry = geometry_for(mode, request)?;
        let stride = configure(&device, mode)?;
        tracing::debug!(?mode, "v4l2 camera mode");

        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let stop = Arc::clone(&stop);
            std::thread::Builder::new()
                .name("camera-v4l2".to_owned())
                .spawn(move || {
                    let mut pump = Pump::new(mode, geometry, sink);
                    let mut outcome = capture(&device, mode, stride, &mut pump, &stop);
                    // A mode that stays silent: try the next one with the same sink.
                    for &next in candidates.iter().skip(1) {
                        if !matches!(outcome, Run::NoFirstPicture) {
                            break;
                        }
                        tracing::info!(
                            silent = ?pump.mode(),
                            ?next,
                            "camera sent no picture, trying another mode"
                        );
                        let (Ok(geometry), Ok(stride)) =
                            (geometry_for(next, request), configure(&device, next))
                        else {
                            continue;
                        };
                        pump = Pump::new(next, geometry, pump.into_sink());
                        outcome = capture(&device, next, stride, &mut pump, &stop);
                    }
                    match outcome {
                        Run::Stopped => {}
                        Run::NoFirstPicture => pump.end(CameraError::Backend(
                            "the camera sends no pictures".to_owned(),
                        )),
                        Run::Failed(reason) => pump.end(reason),
                    }
                })
                .map_err(|err| CameraError::Backend(format!("camera thread: {err}")))?
        };
        Ok(Box::new(V4l2Session {
            stop,
            thread: Some(thread),
        }))
    }
}

/// `/dev/videoN` → the `/dev/v4l/by-id` name pointing at it. Those names hold vendor, model and
/// serial number, so they stay the same across replugs and restarts (FR-8.27).
fn stable_names() -> Vec<(PathBuf, String)> {
    let Ok(entries) = std::fs::read_dir("/dev/v4l/by-id") else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let target = std::fs::canonicalize(entry.path()).ok()?;
            Some((target, entry.file_name().to_string_lossy().into_owned()))
        })
        .collect()
}

/// The camera at `path`, if it is a video capture node (not a metadata or output node) with at
/// least one format Mumcord reads.
fn capture_node(path: &Path, by_id: &[(PathBuf, String)]) -> Option<Node> {
    let device = Device::with_path(path).ok()?;
    let caps = device.query_caps().ok()?;
    if !caps.capabilities.contains(Flags::VIDEO_CAPTURE)
        || !caps.capabilities.contains(Flags::STREAMING)
    {
        return None;
    }
    let formats = device.enum_formats().ok()?;
    if !formats.iter().any(|f| pixel_format(f.fourcc).is_some()) {
        return None;
    }
    let key = by_id
        .iter()
        .find(|(target, _)| target == path)
        .map(|(_, name)| name.clone())
        // No by-id link (e.g. built-in cameras on some buses): the bus position is stable as
        // long as the camera stays in the same port.
        .unwrap_or_else(|| format!("{}\0{}", caps.card, caps.bus));
    Some(Node {
        path: path.to_owned(),
        id: DeviceId::from_os_key(BACKEND, &key),
        name: caps.card,
    })
}

fn pixel_format(fourcc: FourCC) -> Option<PixelFormat> {
    match &fourcc.repr {
        b"NV12" => Some(PixelFormat::Nv12),
        b"YUYV" => Some(PixelFormat::Yuy2),
        b"MJPG" => Some(PixelFormat::Mjpeg),
        _ => None,
    }
}

/// Every mode the camera offers in a format Mumcord reads.
fn modes(device: &Device) -> Vec<Mode> {
    let mut modes = Vec::new();
    for description in device.enum_formats().unwrap_or_default() {
        let Some(format) = pixel_format(description.fourcc) else {
            continue;
        };
        let sizes: Vec<(u32, u32)> = device
            .enum_framesizes(description.fourcc)
            .unwrap_or_default()
            .into_iter()
            .flat_map(|size| match size.size {
                FrameSizeEnum::Discrete(d) => vec![(d.width, d.height)],
                FrameSizeEnum::Stepwise(s) => ranges::sizes_in_range(
                    (s.min_width, s.min_height),
                    (s.max_width, s.max_height),
                    (s.step_width, s.step_height),
                )
                .collect(),
            })
            .collect();
        for (width, height) in sizes {
            let intervals = device
                .enum_frameintervals(description.fourcc, width, height)
                .unwrap_or_default();
            for interval in intervals {
                // V4L2 gives the time per frame; a rate is its inverse.
                let rate = match interval.interval {
                    FrameIntervalEnum::Discrete(t) => Rate {
                        num: t.denominator,
                        den: t.numerator,
                    },
                    FrameIntervalEnum::Stepwise(s) => ranges::rate_in_range(
                        Rate {
                            num: s.max.denominator,
                            den: s.max.numerator,
                        },
                        Rate {
                            num: s.min.denominator,
                            den: s.min.numerator,
                        },
                        crate::MAX_FPS,
                    ),
                };
                modes.push(Mode {
                    format,
                    width,
                    height,
                    rate,
                });
            }
        }
    }
    modes
}

/// Sets the chosen mode; returns the row stride of the pictures.
fn configure(device: &Device, mode: Mode) -> Result<usize, CameraError> {
    let fourcc = FourCC::new(match mode.format {
        PixelFormat::Nv12 => b"NV12",
        PixelFormat::Yuy2 => b"YUYV",
        PixelFormat::Mjpeg => b"MJPG",
    });
    let set = device
        .set_format(&Format::new(mode.width, mode.height, fourcc))
        .map_err(open_error)?;
    if set.fourcc != fourcc || set.width != mode.width || set.height != mode.height {
        return Err(CameraError::Backend(
            "the camera did not accept the chosen mode".to_owned(),
        ));
    }
    let params =
        v4l::video::capture::Parameters::new(v4l::Fraction::new(mode.rate.den, mode.rate.num));
    if let Err(err) = device.set_params(&params) {
        // Some drivers have a fixed rate; the picture still comes.
        tracing::debug!(%err, "v4l2 frame rate not set");
    }
    Ok(set.stride as usize)
}

fn open_error(err: io::Error) -> CameraError {
    match err.raw_os_error() {
        Some(libc::EACCES | libc::EPERM) => CameraError::PermissionDenied,
        Some(libc::ENOENT | libc::ENODEV | libc::ENXIO) => CameraError::DeviceGone,
        Some(libc::EBUSY) => CameraError::Backend("the camera is in use by another app".to_owned()),
        _ => CameraError::Backend(format!("v4l2: {err}")),
    }
}

/// How a capture run ended.
enum Run {
    /// The owner stopped the session (or the pump ended it).
    Stopped,
    /// The first picture never came; another mode may work.
    NoFirstPicture,
    Failed(CameraError),
}

fn geometry_for(mode: Mode, request: CaptureRequest) -> Result<Geometry, CameraError> {
    Geometry::new(
        mode.width,
        mode.height,
        request.max_width,
        request.max_height,
    )
    .ok_or_else(|| CameraError::Backend("the camera picture is too small".to_owned()))
}

/// Streams until `stop` is set or the camera fails.
fn capture(device: &Device, mode: Mode, stride: usize, pump: &mut Pump, stop: &AtomicBool) -> Run {
    let mut stream = match Stream::with_buffers(device, Type::VideoCapture, DRIVER_BUFFERS) {
        Ok(stream) => stream,
        Err(err) => return Run::Failed(open_error(err)),
    };
    // `next` re-queues the previous buffer before it waits, so it must not time out after the
    // first picture (the buffer would be queued twice). The first wait has a deadline; later
    // ones wait for readiness in short slices first so a stop is noticed.
    stream.set_timeout(FIRST_FRAME_TIMEOUT);
    let handle = device.handle();
    let mut first = true;
    while !stop.load(Ordering::Acquire) {
        if !first {
            match handle.poll(libc::POLLIN, STOP_POLL_MS) {
                Ok(0) => continue,
                Ok(_) => {}
                Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                Err(err) => return Run::Failed(stream_error(err)),
            }
        }
        match stream.next() {
            Ok((data, meta)) => {
                let used = (meta.bytesused as usize).min(data.len());
                pump.push(raw(mode, &data[..used], stride));
                if pump.ended() {
                    return Run::Stopped;
                }
            }
            Err(err) if first && err.kind() == io::ErrorKind::TimedOut => {
                return Run::NoFirstPicture;
            }
            Err(err) => return Run::Failed(stream_error(err)),
        }
        first = false;
    }
    Run::Stopped
}

/// One driver buffer as the pump reads it. Single-planar NV12 has the chroma rows right after
/// the luma rows, at the same stride.
fn raw(mode: Mode, data: &[u8], stride: usize) -> Raw<'_> {
    match mode.format {
        PixelFormat::Nv12 => {
            let (y, uv) = data.split_at((stride * mode.height as usize).min(data.len()));
            Raw::Nv12 {
                y,
                y_stride: stride,
                uv,
                uv_stride: stride,
            }
        }
        PixelFormat::Yuy2 | PixelFormat::Mjpeg => Raw::Packed { data, stride },
    }
}

fn stream_error(err: io::Error) -> CameraError {
    match err.raw_os_error() {
        // Unplugged while streaming.
        Some(libc::ENODEV | libc::ENXIO | libc::EIO) => CameraError::DeviceGone,
        _ if err.kind() == io::ErrorKind::TimedOut => {
            CameraError::Backend("the camera sends no pictures".to_owned())
        }
        _ => CameraError::Backend(format!("v4l2: {err}")),
    }
}

struct V4l2Session {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl CaptureSession for V4l2Session {}

impl Drop for V4l2Session {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            tracing::error!("the v4l2 camera thread panicked");
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::sync::mpsc;
    use std::time::Instant;

    use super::*;
    use crate::CameraFrame;

    struct Sink(mpsc::Sender<Result<(u32, u32, usize), CameraError>>);

    impl FrameSink for Sink {
        fn frame(&mut self, frame: CameraFrame) {
            let _ = self
                .0
                .send(Ok((frame.width, frame.height, frame.data.len())));
        }
        fn ended(&mut self, reason: CameraError) {
            let _ = self.0.send(Err(reason));
        }
    }

    /// Runs against the first real camera or v4l2loopback device; skipped without one (CI).
    #[test]
    fn live_camera_delivers_16_9_nv12_frames() {
        let backend = V4l2::default();
        let Some(camera) = backend.devices().into_iter().next() else {
            eprintln!("no V4L2 camera: live test skipped");
            return;
        };
        let loopback = {
            let scan = backend.scan.lock().unwrap();
            let path = &scan.nodes[0].path;
            Device::with_path(path)
                .and_then(|d| d.query_caps())
                .is_ok_and(|caps| caps.driver.contains("loopback"))
        };
        let (tx, rx) = mpsc::channel();
        let session = match backend.open(&camera.id, CaptureRequest::default(), Box::new(Sink(tx)))
        {
            Ok(session) => session,
            Err(err) if loopback => {
                eprintln!("v4l2loopback without a producer ({err}): live test skipped");
                return;
            }
            Err(err) => panic!("opening the camera failed: {err}"),
        };
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut frames = 0;
        while frames < 3 {
            let left = deadline.saturating_duration_since(Instant::now());
            match rx.recv_timeout(left) {
                Ok(Ok((width, height, len))) => {
                    assert!(width <= crate::MAX_WIDTH && height <= crate::MAX_HEIGHT);
                    assert_eq!(width * 9 / 16 / 2, height / 2, "16:9 after the crop");
                    assert_eq!(len, crate::nv12_len(width, height));
                    frames += 1;
                }
                Ok(Err(err)) if loopback => {
                    eprintln!("v4l2loopback without a producer ({err}): live test skipped");
                    return;
                }
                Ok(Err(err)) => panic!("the camera stopped: {err}"),
                Err(_) => panic!("only {frames} frames within 8 s"),
            }
        }
        drop(session);
        assert!(
            rx.try_iter().all(|event| event.is_ok()),
            "no end event after the session was dropped"
        );
    }
}
