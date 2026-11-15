//! One capture session: a synchronous `IMFSourceReader` on its own thread.
//!
//! The reader runs with converters disabled, so it hands over the camera's native pictures in
//! the mode [`crate::mode::choose`] picked; the shared [`Pump`] crops and converts them.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::thread::JoinHandle;

use ::windows::Win32::Media::MediaFoundation::{
    IMF2DBuffer2, IMFMediaBuffer, IMFMediaSource, IMFMediaType, IMFSample, IMFSourceReader,
    MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK, MF_E_NO_MORE_TYPES,
    MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE, MF_MT_SUBTYPE,
    MF_READWRITE_DISABLE_CONVERTERS, MF_SOURCE_READER_ALL_STREAMS,
    MF_SOURCE_READER_FIRST_VIDEO_STREAM, MF_SOURCE_READERF_ENDOFSTREAM, MF_SOURCE_READERF_ERROR,
    MF2DBuffer_LockFlags_Read, MFCreateAttributes, MFCreateDeviceSource,
    MFCreateSourceReaderFromMediaSource,
};
use ::windows::core::{HSTRING, Interface};

use super::bounded::{STOP_TIMEOUT, run_bounded};
use super::com::{Com, MediaFoundation, mf_error, video_capture_attributes};
use super::media_type::{self, raw, tight_stride};
use crate::mode::{self, Geometry, Mode, PixelFormat};
use crate::pump::{Pump, Raw};
use crate::{CameraError, CaptureRequest, CaptureSession, FrameSink};

const STREAM: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
const ALL_STREAMS: u32 = MF_SOURCE_READER_ALL_STREAMS.0 as u32;

/// Handed to the pump when a sample cannot be read; it counts as an unreadable frame.
pub(super) const UNREADABLE: Raw<'static> = Raw::Packed {
    data: &[],
    stride: 0,
};

/// The camera's media source, kept by the session to stop a `ReadSample` that waits.
struct Source(IMFMediaSource);

impl Source {
    /// Releases the camera (its light goes off) and wakes a `ReadSample` waiting for a frame.
    fn shutdown(&self) {
        // SAFETY: see the `Send` impl; shutting down twice only returns `MF_E_SHUTDOWN`.
        if let Err(err) = unsafe { self.0.Shutdown() } {
            tracing::debug!(code = err.code().0, "camera source shutdown failed");
        }
    }
}

// SAFETY: Media Foundation media sources are free-threaded; `Shutdown` may be called from any
// thread while another one reads from it.
unsafe impl Send for Source {}

/// A running Media Foundation capture.
pub struct Session {
    stop: Arc<AtomicBool>,
    source: Source,
    thread: Option<JoinHandle<()>>,
}

impl CaptureSession for Session {}

impl Session {
    /// Opens the camera behind `symbolic_link` on a capture thread and returns once it streams
    /// or failed to start (privacy refusal: [`CameraError::PermissionDenied`]).
    pub fn start(
        symbolic_link: String,
        request: CaptureRequest,
        sink: Box<dyn FrameSink>,
    ) -> Result<Self, CameraError> {
        let stop = Arc::new(AtomicBool::new(false));
        let (ready, started) = mpsc::sync_channel(1);
        let thread = {
            let stop = Arc::clone(&stop);
            std::thread::Builder::new()
                .name("camera-mf".to_owned())
                .spawn(move || run(&symbolic_link, request, sink, &stop, &ready))
                .map_err(|err| CameraError::Backend(format!("capture thread: {err}")))?
        };
        match started.recv() {
            Ok(Ok(source)) => Ok(Self {
                stop,
                source,
                thread: Some(thread),
            }),
            Ok(Err(err)) => {
                join(thread);
                Err(err)
            }
            Err(_) => {
                join(thread);
                Err(CameraError::Backend(
                    "the capture thread ended early".to_owned(),
                ))
            }
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let thread = self.thread.take();
        let source = Source(self.source.0.clone());
        // A camera that opened but never delivered a picture can leave `ReadSample` waiting
        // even after the shutdown, and the shutdown itself can wait on the driver. Both run on
        // a helper thread, so a camera switch or closing the app cannot freeze on them.
        let stopped = run_bounded("camera-mf-stop", STOP_TIMEOUT, move || {
            let _com = Com::init();
            source.shutdown();
            if let Some(thread) = thread {
                join(thread);
            }
            // Released while COM is still initialised on this thread.
            drop(source);
        });
        if !stopped {
            tracing::warn!("the camera did not stop in time; its capture thread is left behind");
        }
    }
}

fn join(thread: JoinHandle<()>) {
    if thread.join().is_err() {
        tracing::error!("camera capture thread panicked");
    }
}

/// The capture thread: open, report the start, then read until stopped or the camera ends.
fn run(
    symbolic_link: &str,
    request: CaptureRequest,
    sink: Box<dyn FrameSink>,
    stop: &AtomicBool,
    ready: &SyncSender<Result<Source, CameraError>>,
) {
    let _com = Com::init();
    let _mf = match MediaFoundation::start() {
        Ok(mf) => mf,
        Err(err) => {
            let _ = ready.send(Err(err));
            return;
        }
    };
    let reader = match Reader::open(symbolic_link, request) {
        Ok(reader) => reader,
        Err(err) => {
            let _ = ready.send(Err(err));
            return;
        }
    };
    tracing::debug!(mode = ?reader.mode, "media foundation camera mode");
    if ready.send(Ok(Source(reader.source.clone()))).is_err() {
        return;
    }
    let mut pump = Pump::new(reader.mode, reader.geometry, sink);
    reader.read(&mut pump, stop);
}

struct Reader {
    source: IMFMediaSource,
    reader: IMFSourceReader,
    mode: Mode,
    geometry: Geometry,
    /// Bytes per row for buffers that cannot be locked as 2D buffers.
    stride: usize,
}

impl Reader {
    fn open(symbolic_link: &str, request: CaptureRequest) -> Result<Self, CameraError> {
        let attributes = video_capture_attributes(2)?;
        // SAFETY: the key is a constant and the value an `HSTRING` that outlives the call.
        unsafe {
            attributes.SetString(
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK,
                &HSTRING::from(symbolic_link),
            )
        }
        .map_err(|err| mf_error(&err))?;
        // A camera the privacy setting blocks fails here with E_ACCESSDENIED.
        // SAFETY: valid attributes from above.
        let source = unsafe { MFCreateDeviceSource(&attributes) }.map_err(|err| mf_error(&err))?;
        match Self::configure(&source, request) {
            Ok(reader) => Ok(reader),
            Err(err) => {
                // The reader was not created or is dropped already; free the camera now.
                // SAFETY: the source is ours and no longer used.
                let _ = unsafe { source.Shutdown() };
                Err(err)
            }
        }
    }

    fn configure(source: &IMFMediaSource, request: CaptureRequest) -> Result<Self, CameraError> {
        let mut attributes = None;
        // SAFETY: out-pointer to a local; the result is checked.
        unsafe { MFCreateAttributes(&mut attributes, 1) }.map_err(|err| mf_error(&err))?;
        let attributes = attributes
            .ok_or_else(|| CameraError::Backend("no Media Foundation attributes".into()))?;
        // Native pictures only: Mumcord converts and decodes itself (FR-8.26).
        // SAFETY: the key is a constant.
        unsafe { attributes.SetUINT32(&MF_READWRITE_DISABLE_CONVERTERS, 1) }
            .map_err(|err| mf_error(&err))?;
        // SAFETY: valid source and attributes.
        let reader = unsafe { MFCreateSourceReaderFromMediaSource(source, &attributes) }
            .map_err(|err| mf_error(&err))?;

        let types = native_types(&reader)?;
        let modes: Vec<Option<Mode>> = types.iter().map(native_mode).collect();
        let usable: Vec<Mode> = modes.iter().flatten().copied().collect();
        let mode = mode::choose(&usable, &request).ok_or_else(|| {
            CameraError::Backend("the camera offers no usable picture format".to_owned())
        })?;
        let index = modes
            .iter()
            .position(|candidate| *candidate == Some(mode))
            .ok_or_else(|| CameraError::Backend("the chosen camera mode vanished".to_owned()))?;
        let geometry = Geometry::new(
            mode.width,
            mode.height,
            request.max_width,
            request.max_height,
        )
        .ok_or_else(|| CameraError::Backend("the camera picture is too small".to_owned()))?;
        let media_type = &types[index];
        // SAFETY: valid reader and media type; stream indexes are the documented constants.
        unsafe {
            reader
                .SetStreamSelection(ALL_STREAMS, false)
                .and_then(|()| reader.SetStreamSelection(STREAM, true))
                .and_then(|()| reader.SetCurrentMediaType(STREAM, None, media_type))
        }
        .map_err(|err| mf_error(&err))?;
        // SAFETY: the key is a constant. The attribute holds a signed stride as a UINT32;
        // negative (bottom-up) strides only occur for RGB, which Mumcord does not read.
        let stride = unsafe { media_type.GetUINT32(&MF_MT_DEFAULT_STRIDE) }
            .ok()
            .map(|stride| stride as i32)
            .filter(|&stride| stride > 0)
            .map_or_else(|| tight_stride(mode.format, mode.width), |s| s as usize);
        Ok(Self {
            source: source.clone(),
            reader,
            mode,
            geometry,
            stride,
        })
    }

    /// Reads until `stop` is set or the camera ends. Real-time path: no allocation, lock, I/O
    /// or logging per frame.
    fn read(&self, pump: &mut Pump, stop: &AtomicBool) {
        loop {
            if stop.load(Ordering::Acquire) {
                pump.stop();
                return;
            }
            let mut flags = 0u32;
            let mut sample: Option<IMFSample> = None;
            // SAFETY: out-pointers to locals.
            let result = unsafe {
                self.reader
                    .ReadSample(STREAM, 0, None, Some(&mut flags), None, Some(&mut sample))
            };
            // A stop shuts the source down, which fails the read; that is not an error.
            if stop.load(Ordering::Acquire) {
                pump.stop();
                return;
            }
            if let Err(err) = result {
                pump.end(mf_error(&err));
                return;
            }
            let ended = (MF_SOURCE_READERF_ERROR.0 | MF_SOURCE_READERF_ENDOFSTREAM.0) as u32;
            if flags & ended != 0 {
                pump.end(CameraError::DeviceGone);
                return;
            }
            // No sample: a stream tick (gap) or a format notice.
            if let Some(sample) = sample {
                self.deliver(pump, &sample);
                if pump.ended() {
                    return;
                }
            }
        }
    }

    fn deliver(&self, pump: &mut Pump, sample: &IMFSample) {
        // Cameras deliver one buffer per sample, so this does not copy.
        // SAFETY: valid sample.
        let Ok(buffer) = (unsafe { sample.ConvertToContiguousBuffer() }) else {
            pump.push(UNREADABLE);
            return;
        };
        if self.mode.format != PixelFormat::Mjpeg && self.deliver_2d(pump, &buffer) {
            return;
        }
        let mut data = std::ptr::null_mut();
        let mut length = 0u32;
        // SAFETY: out-pointers to locals; unlocked below.
        if unsafe { buffer.Lock(&mut data, None, Some(&mut length)) }.is_err() {
            pump.push(UNREADABLE);
            return;
        }
        // SAFETY: a locked buffer holds `length` valid bytes at `data` until `Unlock`.
        let bytes = unsafe { std::slice::from_raw_parts(data, length as usize) };
        pump.push(
            raw(self.mode.format, bytes, self.stride, self.mode.height).unwrap_or(UNREADABLE),
        );
        // SAFETY: locked above.
        let _ = unsafe { buffer.Unlock() };
    }

    /// Delivers an uncompressed picture through the 2D lock, which reports the real pitch of
    /// padded rows. `false` when the buffer has no 2D interface.
    fn deliver_2d(&self, pump: &mut Pump, buffer: &IMFMediaBuffer) -> bool {
        let Ok(buffer) = buffer.cast::<IMF2DBuffer2>() else {
            return false;
        };
        let (mut scanline0, mut pitch) = (std::ptr::null_mut(), 0i32);
        let (mut start, mut length) = (std::ptr::null_mut(), 0u32);
        // SAFETY: out-pointers to locals; unlocked below.
        let locked = unsafe {
            buffer.Lock2DSize(
                MF2DBuffer_LockFlags_Read,
                &mut scanline0,
                &mut pitch,
                &mut start,
                &mut length,
            )
        };
        if locked.is_err() {
            return false;
        }
        let offset = (scanline0 as usize).checked_sub(start as usize);
        let picture = match offset {
            Some(offset) if pitch > 0 && offset <= length as usize => {
                // SAFETY: the locked buffer spans `length` bytes from `start`, and the first
                // row starts `offset` bytes into it.
                let bytes =
                    unsafe { std::slice::from_raw_parts(scanline0, length as usize - offset) };
                raw(self.mode.format, bytes, pitch as usize, self.mode.height)
            }
            _ => None,
        };
        pump.push(picture.unwrap_or(UNREADABLE));
        // SAFETY: locked above.
        let _ = unsafe { buffer.Unlock2D() };
        true
    }
}

/// Every native media type of the video stream.
fn native_types(reader: &IMFSourceReader) -> Result<Vec<IMFMediaType>, CameraError> {
    let mut types = Vec::new();
    for index in 0.. {
        // SAFETY: valid reader; indexes past the end return MF_E_NO_MORE_TYPES.
        match unsafe { reader.GetNativeMediaType(STREAM, index) } {
            Ok(media_type) => types.push(media_type),
            Err(err) if err.code() == MF_E_NO_MORE_TYPES => break,
            Err(err) => return Err(mf_error(&err)),
        }
    }
    Ok(types)
}

fn native_mode(media_type: &IMFMediaType) -> Option<Mode> {
    // SAFETY: the keys are constants.
    let (subtype, size, rate) = unsafe {
        (
            media_type.GetGUID(&MF_MT_SUBTYPE).ok()?,
            media_type.GetUINT64(&MF_MT_FRAME_SIZE).ok()?,
            media_type.GetUINT64(&MF_MT_FRAME_RATE).ok()?,
        )
    };
    media_type::mode(subtype.to_u128(), size, rate)
}
