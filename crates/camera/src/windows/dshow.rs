//! DirectShow cameras (#631). Virtual cameras such as the OBS Virtual Camera register only as
//! DirectShow capture filters, which Media Foundation does not list. The DirectShow cameras that
//! Media Foundation does not list already ([`listed_by_mf`]) join the camera list; opening one
//! runs the filter graph `camera → Sample Grabber → Null Renderer`.
//!
//! The Sample Grabber calls back on the camera filter's own streaming thread. The callback only
//! hands the sample (a reference, no copy) to the capture thread, which converts it through the
//! same mode choice, crop and pump as Media Foundation ([`crate::mode`], [`crate::pump`]).
//! DirectShow filters send no device notifications, so a newly installed virtual camera appears
//! the next time the list is read.

use std::ffi::c_void;
use std::mem::ManuallyDrop;
use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use ::windows::Win32::Foundation::{E_NOTIMPL, RECT, S_OK};
use ::windows::Win32::Media::DirectShow::{
    IAMStreamConfig, IBaseFilter, ICreateDevEnum, IGraphBuilder, IMediaControl, IMediaEvent,
    IMediaFilter, IMediaSample, IPin, PIN_DIRECTION, PINDIR_INPUT, PINDIR_OUTPUT,
    VIDEO_STREAM_CONFIG_CAPS,
};
use ::windows::Win32::Media::IReferenceClock;
use ::windows::Win32::Media::MediaFoundation::{
    AM_MEDIA_TYPE, CLSID_FilterGraph, CLSID_SystemDeviceEnum, CLSID_VideoInputDeviceCategory,
    FORMAT_VideoInfo, FORMAT_VideoInfo2, MEDIATYPE_Video, VIDEOINFOHEADER, VIDEOINFOHEADER2,
};
use ::windows::Win32::System::Com::StructuredStorage::IPropertyBag;
use ::windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree, CreateBindCtx, IBindCtx, IErrorLog,
    IMoniker,
};
use ::windows::Win32::System::Variant::{VARIANT, VT_BSTR, VariantClear};
use ::windows::core::{GUID, HRESULT, Interface, PCWSTR, implement, w};

use self::qedit::{ISampleGrabber, ISampleGrabberCB, ISampleGrabberCB_Impl};
use super::capture::UNREADABLE;
use super::com::{self, Com};
use super::dshow_type::{
    dshow_error, event_end, image_size, listed_by_mf, scaled_size, video_mode,
};
use super::media_type::{device_key, raw, tight_stride};
use crate::mode::{self, Geometry, Mode, PixelFormat};
use crate::pump::Pump;
use crate::{CameraDevice, CameraError, CaptureRequest, CaptureSession, DeviceId, FrameSink};

/// The Sample Grabber and Null Renderer filters (`qedit.dll`, part of Windows). Their header
/// left the Windows SDK, so the `windows` crate has neither; the ids and the two interfaces
/// below are the ones `qedit.h` documents.
const CLSID_SAMPLE_GRABBER: GUID = GUID::from_u128(0xc1f400a0_3f08_11d3_9f0b_006008039e37);
const CLSID_NULL_RENDERER: GUID = GUID::from_u128(0xc1f400a4_3f08_11d3_9f0b_006008039e37);

/// How long the capture thread waits for a picture before it checks for the stop and for graph
/// events again.
const WAIT: Duration = Duration::from_millis(100);
/// A camera that sends no picture for this long ends the session: a virtual camera whose app is
/// gone may stay silent instead of reporting anything.
const SILENCE_LIMIT: Duration = Duration::from_secs(5);
/// Building and starting the graph runs the camera filter's own code. One that has not finished
/// after this long fails the start instead of blocking Mumcord.
const START_TIMEOUT: Duration = Duration::from_secs(10);

/// The Sample Grabber's interfaces as `qedit.h` declares them.
#[allow(
    non_snake_case,
    dead_code,
    reason = "COM names follow the SDK, and the vtables list every method in order, also those \
              Mumcord does not call"
)]
mod qedit {
    use std::ffi::c_void;

    use ::windows::Win32::Media::MediaFoundation::AM_MEDIA_TYPE;
    use ::windows::core::{BOOL, HRESULT, IUnknown, IUnknown_Vtbl, interface};

    #[interface("6B652FFF-11FE-4FCE-92AD-0266B5D7C78F")]
    pub unsafe trait ISampleGrabber: IUnknown {
        pub fn SetOneShot(&self, one_shot: BOOL) -> HRESULT;
        pub fn SetMediaType(&self, media_type: *const AM_MEDIA_TYPE) -> HRESULT;
        pub fn GetConnectedMediaType(&self, media_type: *mut AM_MEDIA_TYPE) -> HRESULT;
        pub fn SetBufferSamples(&self, buffer_them: BOOL) -> HRESULT;
        pub fn GetCurrentBuffer(&self, size: *mut i32, buffer: *mut i32) -> HRESULT;
        pub fn GetCurrentSample(&self, sample: *mut *mut c_void) -> HRESULT;
        pub fn SetCallback(&self, callback: *mut c_void, which: i32) -> HRESULT;
    }

    #[interface("0579154A-2B53-4994-B0D0-E773148EFF85")]
    pub unsafe trait ISampleGrabberCB: IUnknown {
        pub fn SampleCB(&self, sample_time: f64, sample: *mut c_void) -> HRESULT;
        pub fn BufferCB(&self, sample_time: f64, buffer: *mut u8, length: i32) -> HRESULT;
    }
}

/// A DirectShow camera that Media Foundation does not list.
pub struct Camera {
    pub device: CameraDevice,
    /// The display name of the camera's moniker: the key it is opened with. Never logged or
    /// stored (FR-8.28).
    pub key: String,
}

/// The DirectShow cameras that are not in Media Foundation's list `listed`, in DirectShow's
/// order.
pub fn cameras(listed: &[com::Camera]) -> Result<Vec<Camera>, CameraError> {
    let _com = Com::init();
    let mut cameras = Vec::new();
    for moniker in monikers()? {
        let Some(key) = display_name(&moniker) else {
            continue;
        };
        // SAFETY: valid moniker; no bind context is needed for device monikers.
        let Ok(properties) =
            (unsafe { moniker.BindToStorage::<_, _, IPropertyBag>(None::<&IBindCtx>, None) })
        else {
            continue;
        };
        let name = property(&properties, w!("FriendlyName")).unwrap_or_else(|| "Camera".to_owned());
        let path = property(&properties, w!("DevicePath"));
        let mf = listed
            .iter()
            .map(|camera| (camera.device.name.as_str(), camera.symbolic_link.as_str()));
        if listed_by_mf(&name, path.as_deref(), mf) {
            continue;
        }
        cameras.push(Camera {
            device: CameraDevice {
                id: DeviceId::from_os_key("dshow", &device_key(&key)),
                name,
                is_default: false,
            },
            key,
        });
    }
    Ok(cameras)
}

/// The monikers of DirectShow's video input category.
fn monikers() -> Result<Vec<IMoniker>, CameraError> {
    // SAFETY: plain COM object creation; the result is checked.
    let devices: ICreateDevEnum =
        unsafe { CoCreateInstance(&CLSID_SystemDeviceEnum, None, CLSCTX_INPROC_SERVER) }
            .map_err(|err| dshow_error(err.code().0))?;
    let mut list = None;
    // SAFETY: the category is a constant and `list` an out-pointer to a local.
    unsafe { devices.CreateClassEnumerator(&CLSID_VideoInputDeviceCategory, &mut list, 0) }
        .map_err(|err| dshow_error(err.code().0))?;
    // An empty category returns S_FALSE without an enumerator.
    let Some(list) = list else {
        return Ok(Vec::new());
    };
    let mut monikers = Vec::new();
    loop {
        let mut next = [None];
        // SAFETY: room for one moniker, which is then ours.
        if unsafe { list.Next(&mut next, None) } != S_OK {
            break;
        }
        let [Some(moniker)] = next else {
            break;
        };
        monikers.push(moniker);
    }
    Ok(monikers)
}

/// The moniker's display name, the stable key of a DirectShow device.
fn display_name(moniker: &IMoniker) -> Option<String> {
    // SAFETY: plain call; the result is checked.
    let context = unsafe { CreateBindCtx(0) }.ok()?;
    // SAFETY: valid moniker and context; the string is CoTaskMem memory freed below.
    let name = unsafe { moniker.GetDisplayName(&context, None::<&IMoniker>) }.ok()?;
    // SAFETY: on success `name` is a valid NUL-terminated wide string.
    let text = unsafe { name.to_string() }.ok();
    // SAFETY: allocated by `GetDisplayName`, not used after this.
    unsafe { CoTaskMemFree(Some(name.0.cast_const().cast())) };
    text
}

/// A string property of a device, `None` when it is missing.
fn property(properties: &IPropertyBag, key: PCWSTR) -> Option<String> {
    let mut value = VARIANT::default();
    // SAFETY: `value` is an empty variant that `Read` fills; it stays empty when `Read` fails.
    unsafe { properties.Read(key, &mut value, None::<&IErrorLog>) }.ok()?;
    // SAFETY: the BSTR member is read only when the variant says it holds one.
    let text = unsafe {
        let value = &value.Anonymous.Anonymous;
        (value.vt == VT_BSTR).then(|| value.Anonymous.bstrVal.to_string())
    };
    // SAFETY: frees what `Read` put into the variant, once.
    if let Err(err) = unsafe { VariantClear(&mut value) } {
        tracing::debug!(code = err.code().0, "camera property not freed");
    }
    text
}

/// A running DirectShow capture.
pub struct Session {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl CaptureSession for Session {}

impl Session {
    /// Opens the camera behind `key` (see [`Camera::key`]) on a capture thread and returns once
    /// its graph runs or failed to start.
    pub fn start(
        key: String,
        request: CaptureRequest,
        sink: Box<dyn FrameSink>,
    ) -> Result<Self, CameraError> {
        let stop = Arc::new(AtomicBool::new(false));
        let (ready, started) = mpsc::sync_channel(1);
        let thread = {
            let stop = Arc::clone(&stop);
            std::thread::Builder::new()
                .name("camera-dshow".to_owned())
                .spawn(move || run(&key, request, sink, &stop, &ready))
                .map_err(|err| CameraError::Backend(format!("capture thread: {err}")))?
        };
        match started.recv_timeout(START_TIMEOUT) {
            Ok(Ok(())) => Ok(Self {
                stop,
                thread: Some(thread),
            }),
            Ok(Err(err)) => {
                join(thread);
                Err(err)
            }
            Err(RecvTimeoutError::Timeout) => {
                // Not joined: the thread may hang in the filter. It stops the graph and ends on
                // its own once the filter returns, without ever calling the sink.
                stop.store(true, Ordering::Release);
                Err(CameraError::Backend("the camera did not start".to_owned()))
            }
            Err(RecvTimeoutError::Disconnected) => {
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
        // The capture thread sees the stop within `WAIT`; stopping its graph releases the camera.
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            join(thread);
        }
    }
}

fn join(thread: JoinHandle<()>) {
    if thread.join().is_err() {
        tracing::error!("camera capture thread panicked");
    }
}

/// The capture thread: build and start the graph, report the start, then read until stopped or
/// the camera ends.
fn run(
    key: &str,
    request: CaptureRequest,
    sink: Box<dyn FrameSink>,
    stop: &AtomicBool,
    ready: &SyncSender<Result<(), CameraError>>,
) {
    let _com = Com::init();
    let (samples, received) = mpsc::sync_channel(1);
    let graph = match Graph::open(key, request, samples) {
        Ok(graph) => graph,
        Err(err) => {
            let _ = ready.send(Err(err));
            return;
        }
    };
    tracing::debug!(mode = ?graph.mode, "directshow camera mode");
    // The start timed out (or the session is gone) when nobody waits any more.
    if stop.load(Ordering::Acquire) || ready.send(Ok(())).is_err() {
        return;
    }
    let mut pump = Pump::new(graph.mode, graph.geometry, sink);
    graph.read(&received, &mut pump, stop);
}

/// A picture the grabber handed over; holding it keeps its buffer from being reused.
struct Sample(IMediaSample);

// SAFETY: media samples are free-threaded: their allocator hands them from the streaming thread
// to whoever consumes them. Only the reference count and the buffer are used here.
unsafe impl Send for Sample {}

/// The grabber's callback, on the camera filter's streaming thread for every picture. It only
/// passes the sample on: no allocation, blocking or logging.
#[implement(ISampleGrabberCB)]
struct Grabbed(SyncSender<Sample>);

impl ISampleGrabberCB_Impl for Grabbed_Impl {
    unsafe fn SampleCB(&self, _sample_time: f64, sample: *mut c_void) -> HRESULT {
        // SAFETY: the grabber passes a valid sample for the duration of the call; `clone` takes
        // a reference of its own.
        if let Some(sample) = unsafe { IMediaSample::from_raw_borrowed(&sample) } {
            // Full: the capture thread still converts the last picture, so this one is dropped
            // and its buffer goes back to the camera.
            let _ = self.0.try_send(Sample(sample.clone()));
        }
        S_OK
    }

    unsafe fn BufferCB(&self, _sample_time: f64, _buffer: *mut u8, _length: i32) -> HRESULT {
        E_NOTIMPL
    }
}

/// A running graph `camera → Sample Grabber → Null Renderer`; dropping it stops the camera.
struct Graph {
    control: IMediaControl,
    events: IMediaEvent,
    grabber: ISampleGrabber,
    /// Kept for as long as the grabber may call it.
    _callback: ISampleGrabberCB,
    mode: Mode,
    geometry: Geometry,
    /// Bytes per row: DirectShow pictures are not padded.
    stride: usize,
}

impl Graph {
    fn open(
        key: &str,
        request: CaptureRequest,
        samples: SyncSender<Sample>,
    ) -> Result<Self, CameraError> {
        let failed = |err: ::windows::core::Error| dshow_error(err.code().0);
        let camera = source(key)?;
        // SAFETY: plain COM object creation; the result is checked.
        let graph: IGraphBuilder =
            unsafe { CoCreateInstance(&CLSID_FilterGraph, None, CLSCTX_INPROC_SERVER) }
                .map_err(failed)?;
        // SAFETY: valid graph and filter; the name is a constant.
        unsafe { graph.AddFilter(&camera, w!("Camera")) }.map_err(failed)?;
        let output = pin(&camera, PINDIR_OUTPUT)?;
        let config = output.cast::<IAMStreamConfig>().ok();
        let formats = formats(&output, config.as_ref())?;
        let modes: Vec<Mode> = formats.iter().map(|format| format.mode).collect();
        let chosen = mode::choose(&modes, &request)
            .and_then(|mode| formats.iter().find(|format| format.mode == mode))
            .ok_or_else(|| {
                CameraError::Backend("the camera offers no usable picture format".to_owned())
            })?;

        let (grabbing, grabber, callback) = sample_grabber(chosen, samples)?;
        // SAFETY: plain COM object creation; the result is checked.
        let renderer: IBaseFilter =
            unsafe { CoCreateInstance(&CLSID_NULL_RENDERER, None, CLSCTX_INPROC_SERVER) }
                .map_err(failed)?;
        // SAFETY: valid graph and filters; the names are constants.
        unsafe { graph.AddFilter(&grabbing, w!("Sample Grabber")) }.map_err(failed)?;
        // SAFETY: as above.
        unsafe { graph.AddFilter(&renderer, w!("Null Renderer")) }.map_err(failed)?;
        connect(
            &graph,
            &output,
            &pin(&grabbing, PINDIR_INPUT)?,
            config.as_ref(),
            chosen,
            &request,
        )?;
        // SAFETY: pins of filters in this graph; the grabber passes on what it receives.
        unsafe {
            graph.ConnectDirect(
                &pin(&grabbing, PINDIR_OUTPUT)?,
                &pin(&renderer, PINDIR_INPUT)?,
                None,
            )
        }
        .map_err(failed)?;
        // No reference clock: every picture goes through as soon as it arrives, and the
        // renderer never holds the camera's thread back to wait for its time.
        // SAFETY: valid graph.
        unsafe {
            graph
                .cast::<IMediaFilter>()
                .and_then(|graph| graph.SetSyncSource(None::<&IReferenceClock>))
        }
        .map_err(failed)?;

        let mode = connected_mode(&grabber, chosen.mode)?;
        let geometry = Geometry::new(
            mode.width,
            mode.height,
            request.max_width,
            request.max_height,
        )
        .ok_or_else(|| CameraError::Backend("the camera picture is too small".to_owned()))?;
        let graph = Self {
            control: graph.cast().map_err(failed)?,
            events: graph.cast().map_err(failed)?,
            grabber,
            _callback: callback,
            mode,
            geometry,
            stride: tight_stride(mode.format, mode.width),
        };
        // SAFETY: valid, fully connected graph; `Drop` stops it again on any failure.
        unsafe { graph.control.Run() }.map_err(failed)?;
        Ok(graph)
    }

    /// Reads until `stop` is set or the camera ends. Real-time path: no allocation, lock, I/O
    /// or logging per frame.
    fn read(&self, samples: &Receiver<Sample>, pump: &mut Pump, stop: &AtomicBool) {
        let mut last_picture = Instant::now();
        let mut events_read = Instant::now();
        loop {
            if stop.load(Ordering::Acquire) {
                pump.stop();
                return;
            }
            match samples.recv_timeout(WAIT) {
                Ok(sample) => {
                    last_picture = Instant::now();
                    self.deliver(pump, &sample.0);
                    if pump.ended() {
                        return;
                    }
                }
                Err(RecvTimeoutError::Timeout) if last_picture.elapsed() >= SILENCE_LIMIT => {
                    pump.end(CameraError::Backend(
                        "the camera sends no pictures".to_owned(),
                    ));
                    return;
                }
                Err(RecvTimeoutError::Timeout) => {}
                // Only once the graph is gone, which holds the sending end.
                Err(RecvTimeoutError::Disconnected) => {
                    pump.end(CameraError::DeviceGone);
                    return;
                }
            }
            if events_read.elapsed() >= WAIT {
                events_read = Instant::now();
                if let Some(reason) = self.ended() {
                    pump.end(reason);
                    return;
                }
            }
        }
    }

    fn deliver(&self, pump: &mut Pump, sample: &IMediaSample) {
        // SAFETY: valid sample; its buffer stays valid while the sample is held.
        let (data, length) = unsafe { (sample.GetPointer(), sample.GetActualDataLength()) };
        let (Ok(data), Ok(length)) = (data, usize::try_from(length)) else {
            pump.push(UNREADABLE);
            return;
        };
        if data.is_null() {
            pump.push(UNREADABLE);
            return;
        }
        // SAFETY: the sample's buffer holds `length` valid bytes at `data`.
        let bytes = unsafe { std::slice::from_raw_parts(data, length) };
        pump.push(
            raw(self.mode.format, bytes, self.stride, self.mode.height).unwrap_or(UNREADABLE),
        );
    }

    /// Why the camera ended, if a graph event says so.
    fn ended(&self) -> Option<CameraError> {
        loop {
            let (mut code, mut param1, mut param2) = (0, 0, 0);
            // SAFETY: out-pointers to locals; with no event waiting, the call fails at once.
            unsafe { self.events.GetEvent(&mut code, &mut param1, &mut param2, 0) }.ok()?;
            let end = event_end(code, param1, param2);
            // SAFETY: frees what `GetEvent` handed out, once.
            let _ = unsafe { self.events.FreeEventParams(code, param1, param2) };
            if end.is_some() {
                return end;
            }
        }
    }
}

impl Drop for Graph {
    fn drop(&mut self) {
        // SAFETY: valid graph; stopping a stopped graph does nothing. Stopping releases the
        // camera.
        if let Err(err) = unsafe { self.control.Stop() } {
            tracing::debug!(code = err.code().0, "camera graph stop failed");
        }
        // No more calls into the callback once it is released with the graph.
        // SAFETY: valid grabber; a null callback is allowed.
        let _ = unsafe { self.grabber.SetCallback(std::ptr::null_mut(), 0) };
    }
}

/// The camera's filter, found again by its key.
fn source(key: &str) -> Result<IBaseFilter, CameraError> {
    let wanted = device_key(key);
    let moniker = monikers()?
        .into_iter()
        .find(|moniker| display_name(moniker).is_some_and(|name| device_key(&name) == wanted))
        .ok_or(CameraError::NoCamera)?;
    // SAFETY: valid moniker; no bind context is needed for device monikers.
    unsafe { moniker.BindToObject::<_, _, IBaseFilter>(None::<&IBindCtx>, None::<&IMoniker>) }
        .map_err(|err| dshow_error(err.code().0))
}

/// The first pin of `filter` in `direction`. A camera filter's first output pin is the one that
/// captures.
fn pin(filter: &IBaseFilter, direction: PIN_DIRECTION) -> Result<IPin, CameraError> {
    // SAFETY: valid filter.
    let pins = unsafe { filter.EnumPins() }.map_err(|err| dshow_error(err.code().0))?;
    loop {
        let mut next = [None];
        // SAFETY: room for one pin, which is then ours.
        if unsafe { pins.Next(&mut next, None) } != S_OK {
            break;
        }
        let [Some(pin)] = next else {
            break;
        };
        // SAFETY: valid pin.
        if unsafe { pin.QueryDirection() }.is_ok_and(|found| found == direction) {
            return Ok(pin);
        }
    }
    Err(CameraError::Backend(
        "a DirectShow filter lacks a pin".to_owned(),
    ))
}

/// A media type a filter handed out; dropping it frees it as `DeleteMediaType` does.
struct MediaType(NonNull<AM_MEDIA_TYPE>);

impl MediaType {
    fn get(&self) -> &AM_MEDIA_TYPE {
        // SAFETY: owned and valid until `Drop`.
        unsafe { self.0.as_ref() }
    }
}

impl Drop for MediaType {
    fn drop(&mut self) {
        // SAFETY: the type and its format block are CoTaskMem memory the filter handed over;
        // neither is used after this.
        unsafe {
            free_format(self.0.as_mut());
            CoTaskMemFree(Some(self.0.as_ptr().cast_const().cast()));
        }
    }
}

/// Frees what a media type points to, as `FreeMediaType` does, and leaves it empty.
///
/// # Safety
///
/// The format block must be CoTaskMem memory the caller owns.
unsafe fn free_format(media_type: &mut AM_MEDIA_TYPE) {
    if !media_type.pbFormat.is_null() {
        // SAFETY: see above; the pointer is cleared right after.
        unsafe { CoTaskMemFree(Some(media_type.pbFormat.cast_const().cast())) };
        media_type.pbFormat = std::ptr::null_mut();
        media_type.cbFormat = 0;
    }
    // SAFETY: taken once and replaced right after, so it is never released twice.
    drop(unsafe { ManuallyDrop::take(&mut media_type.pUnk) });
    media_type.pUnk = ManuallyDrop::new(None);
}

/// The header at the start of a media type's format block, if the block holds one.
fn header<T: Copy>(media_type: &AM_MEDIA_TYPE) -> Option<T> {
    if media_type.pbFormat.is_null() || (media_type.cbFormat as usize) < size_of::<T>() {
        return None;
    }
    // SAFETY: the block holds at least one `T`; format blocks need not be aligned.
    Some(unsafe { media_type.pbFormat.cast::<T>().read_unaligned() })
}

/// The [`Mode`] of a video media type. `interval` stands in for a frame interval the header
/// leaves at 0.
fn mode_of(media_type: &AM_MEDIA_TYPE, interval: i64) -> Option<Mode> {
    if media_type.majortype != MEDIATYPE_Video {
        return None;
    }
    let (width, height, own) = if media_type.formattype == FORMAT_VideoInfo {
        let header: VIDEOINFOHEADER = header(media_type)?;
        (
            header.bmiHeader.biWidth,
            header.bmiHeader.biHeight,
            header.AvgTimePerFrame,
        )
    } else if media_type.formattype == FORMAT_VideoInfo2 {
        let header: VIDEOINFOHEADER2 = header(media_type)?;
        (
            header.bmiHeader.biWidth,
            header.bmiHeader.biHeight,
            header.AvgTimePerFrame,
        )
    } else {
        return None;
    };
    let interval = if own > 0 { own } else { interval };
    video_mode(media_type.subtype.to_u128(), width, height, interval)
}

/// One way the camera can deliver pictures, as its pin lists it.
struct Format {
    media_type: MediaType,
    mode: Mode,
}

/// Every format of the camera's pin that Mumcord reads: through `IAMStreamConfig`, which
/// virtual cameras offer so that apps can pick a format, else the pin's media types.
fn formats(pin: &IPin, config: Option<&IAMStreamConfig>) -> Result<Vec<Format>, CameraError> {
    let listed = match config {
        Some(config) => stream_caps(config)?,
        None => media_types(pin)?,
    };
    Ok(listed
        .into_iter()
        .filter_map(|(media_type, interval)| {
            let mode = mode_of(media_type.get(), interval)?;
            Some(Format { media_type, mode })
        })
        .collect())
}

/// The pin's capabilities, each with its shortest frame interval.
fn stream_caps(config: &IAMStreamConfig) -> Result<Vec<(MediaType, i64)>, CameraError> {
    let (mut count, mut size) = (0, 0);
    // SAFETY: out-pointers to locals.
    unsafe { config.GetNumberOfCapabilities(&mut count, &mut size) }
        .map_err(|err| dshow_error(err.code().0))?;
    // Room for the structure the pin fills, whatever size it reports; u64 for its alignment.
    let size = usize::try_from(size).unwrap_or_default();
    let mut caps = vec![0u64; size.max(size_of::<VIDEO_STREAM_CONFIG_CAPS>()).div_ceil(8)];
    let mut listed = Vec::new();
    for index in 0..count {
        let mut media_type = std::ptr::null_mut();
        // SAFETY: `caps` has room for what the pin reported; the media type becomes ours.
        let result =
            unsafe { config.GetStreamCaps(index, &mut media_type, caps.as_mut_ptr().cast()) };
        let Some(media_type) = NonNull::new(media_type).map(MediaType) else {
            continue;
        };
        if result.is_err() {
            continue;
        }
        let interval = if size >= size_of::<VIDEO_STREAM_CONFIG_CAPS>() {
            // SAFETY: the pin filled a whole video capability structure; `caps` is aligned.
            unsafe { caps.as_ptr().cast::<VIDEO_STREAM_CONFIG_CAPS>().read() }.MinFrameInterval
        } else {
            0
        };
        listed.push((media_type, interval));
    }
    Ok(listed)
}

/// The pin's own media types, for a pin without `IAMStreamConfig`.
fn media_types(pin: &IPin) -> Result<Vec<(MediaType, i64)>, CameraError> {
    // SAFETY: valid pin.
    let types = unsafe { pin.EnumMediaTypes() }.map_err(|err| dshow_error(err.code().0))?;
    let mut listed = Vec::new();
    loop {
        let mut next = [std::ptr::null_mut()];
        // SAFETY: room for one media type, which is then ours.
        if unsafe { types.Next(&mut next, None) } != S_OK {
            break;
        }
        let Some(media_type) = NonNull::new(next[0]).map(MediaType) else {
            break;
        };
        listed.push((media_type, 0));
    }
    Ok(listed)
}

/// The Sample Grabber (its filter, its interface and the callback it calls), set to take
/// `chosen`'s subtype and to hand every picture to `samples`.
fn sample_grabber(
    chosen: &Format,
    samples: SyncSender<Sample>,
) -> Result<(IBaseFilter, ISampleGrabber, ISampleGrabberCB), CameraError> {
    let failed = |err: ::windows::core::Error| dshow_error(err.code().0);
    // SAFETY: plain COM object creation; the result is checked.
    let filter: IBaseFilter =
        unsafe { CoCreateInstance(&CLSID_SAMPLE_GRABBER, None, CLSCTX_INPROC_SERVER) }
            .inspect_err(|err| {
                tracing::warn!(
                    code = err.code().0,
                    "DirectShow Sample Grabber not available"
                );
            })
            .map_err(failed)?;
    let grabber: ISampleGrabber = filter.cast().map_err(failed)?;
    // Only the subtype is fixed; the camera's pin proposes the rest.
    let wanted = AM_MEDIA_TYPE {
        majortype: MEDIATYPE_Video,
        subtype: chosen.media_type.get().subtype,
        ..Default::default()
    };
    let callback: ISampleGrabberCB = Grabbed(samples).into();
    // SAFETY: valid grabber; it copies the type, and the callback outlives its use in the graph
    // (0: call `SampleCB`).
    unsafe {
        grabber
            .SetMediaType(&wanted)
            .ok()
            .and_then(|()| grabber.SetOneShot(false.into()).ok())
            .and_then(|()| grabber.SetBufferSamples(false.into()).ok())
            .and_then(|()| grabber.SetCallback(callback.as_raw(), 0).ok())
    }
    .map_err(failed)?;
    Ok((filter, grabber, callback))
}

/// A media type that owns its format block.
struct OwnedType {
    media_type: AM_MEDIA_TYPE,
    /// The block `media_type.pbFormat` points to.
    _format: Vec<u8>,
}

/// `listed` asking for `width × height` instead (see [`scaled_size`]).
fn scaled_type(
    listed: &AM_MEDIA_TYPE,
    format: PixelFormat,
    (width, height): (u32, u32),
) -> Option<OwnedType> {
    let size = image_size(format, width, height)?;
    let (width, height) = (i32::try_from(width).ok()?, i32::try_from(height).ok()?);
    if listed.pbFormat.is_null() {
        return None;
    }
    // SAFETY: the listed type's format block holds `cbFormat` bytes.
    let mut block =
        unsafe { std::slice::from_raw_parts(listed.pbFormat, listed.cbFormat as usize) }.to_vec();
    let picture = RECT {
        left: 0,
        top: 0,
        right: width,
        bottom: height,
    };
    let resized = if listed.formattype == FORMAT_VideoInfo {
        edit(&mut block, |header: &mut VIDEOINFOHEADER| {
            header.rcSource = picture;
            header.rcTarget = picture;
            header.bmiHeader.biWidth = width;
            header.bmiHeader.biHeight = height;
            header.bmiHeader.biSizeImage = size;
        })
    } else if listed.formattype == FORMAT_VideoInfo2 {
        edit(&mut block, |header: &mut VIDEOINFOHEADER2| {
            header.rcSource = picture;
            header.rcTarget = picture;
            header.bmiHeader.biWidth = width;
            header.bmiHeader.biHeight = height;
            header.bmiHeader.biSizeImage = size;
        })
    } else {
        false
    };
    if !resized {
        return None;
    }
    let media_type = AM_MEDIA_TYPE {
        majortype: listed.majortype,
        subtype: listed.subtype,
        bFixedSizeSamples: listed.bFixedSizeSamples,
        bTemporalCompression: listed.bTemporalCompression,
        lSampleSize: size,
        formattype: listed.formattype,
        pUnk: ManuallyDrop::new(None),
        cbFormat: listed.cbFormat,
        pbFormat: block.as_mut_ptr(),
    };
    Some(OwnedType {
        media_type,
        _format: block,
    })
}

/// Changes the `T` at the start of `block`; `false` when the block is too short for one.
fn edit<T: Copy>(block: &mut [u8], change: impl FnOnce(&mut T)) -> bool {
    if block.len() < size_of::<T>() {
        return false;
    }
    let at = block.as_mut_ptr().cast::<T>();
    // SAFETY: the block holds a whole `T` (checked above); it need not be aligned.
    let mut value = unsafe { at.read_unaligned() };
    change(&mut value);
    // SAFETY: as above.
    unsafe { at.write_unaligned(value) };
    true
}

/// Connects the camera's pin straight to the grabber, with no converter in between (as Media
/// Foundation runs with converters disabled): at the scaled size first when [`scaled_size`]
/// asks for one, else as listed.
fn connect(
    graph: &IGraphBuilder,
    output: &IPin,
    input: &IPin,
    config: Option<&IAMStreamConfig>,
    chosen: &Format,
    request: &CaptureRequest,
) -> Result<(), CameraError> {
    let scaled = scaled_size(&chosen.mode, request)
        .and_then(|size| scaled_type(chosen.media_type.get(), chosen.mode.format, size));
    if let Some(scaled) = &scaled {
        match connect_as(graph, output, input, config, &scaled.media_type) {
            Ok(()) => return Ok(()),
            Err(err) => tracing::debug!(%err, "camera refused the scaled picture size"),
        }
    }
    connect_as(graph, output, input, config, chosen.media_type.get())
}

fn connect_as(
    graph: &IGraphBuilder,
    output: &IPin,
    input: &IPin,
    config: Option<&IAMStreamConfig>,
    media_type: &AM_MEDIA_TYPE,
) -> Result<(), CameraError> {
    if let Some(config) = config {
        // SAFETY: valid pin and media type; the pin copies the type.
        unsafe { config.SetFormat(media_type) }.map_err(|err| dshow_error(err.code().0))?;
    }
    // SAFETY: pins of filters in `graph`; the type is valid for the call.
    unsafe { graph.ConnectDirect(output, input, Some(std::ptr::from_ref(media_type))) }
        .map_err(|err| dshow_error(err.code().0))
}

/// The mode the camera and the grabber agreed on, which is what the pictures are in.
fn connected_mode(grabber: &ISampleGrabber, chosen: Mode) -> Result<Mode, CameraError> {
    let mut connected = AM_MEDIA_TYPE::default();
    // SAFETY: valid grabber; it fills `connected`, whose format block is freed below.
    unsafe { grabber.GetConnectedMediaType(&mut connected) }
        .ok()
        .map_err(|err| dshow_error(err.code().0))?;
    let mode = mode_of(&connected, i64::from(chosen.rate.den));
    // SAFETY: the block was allocated for us by the grabber.
    unsafe { free_format(&mut connected) };
    mode.ok_or_else(|| CameraError::Backend("the camera picture format is unreadable".to_owned()))
}
