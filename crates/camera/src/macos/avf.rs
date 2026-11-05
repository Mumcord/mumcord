//! The AVFoundation side of the macOS backend: device list, hot-plug, permission and the capture
//! session.

use std::cell::UnsafeCell;
use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Instant;

use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchRetained};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, ProtocolObject};
use objc2::{AnyThread, DefinedClass, define_class, msg_send};
use objc2_av_foundation::{
    AVAuthorizationStatus, AVCaptureConnection, AVCaptureDevice, AVCaptureDeviceDiscoverySession,
    AVCaptureDeviceFormat, AVCaptureDeviceInput, AVCaptureDevicePosition, AVCaptureDeviceType,
    AVCaptureDeviceTypeBuiltInWideAngleCamera, AVCaptureDeviceWasConnectedNotification,
    AVCaptureDeviceWasDisconnectedNotification, AVCaptureOutput, AVCaptureSession,
    AVCaptureSessionRuntimeErrorNotification, AVCaptureVideoDataOutput,
    AVCaptureVideoDataOutputSampleBufferDelegate, AVMediaType, AVMediaTypeVideo,
};
use objc2_core_media::{CMSampleBuffer, CMTime, CMVideoFormatDescriptionGetDimensions};
use objc2_core_video::{
    CVPixelBufferGetBaseAddressOfPlane, CVPixelBufferGetBytesPerRowOfPlane, CVPixelBufferGetHeight,
    CVPixelBufferGetHeightOfPlane, CVPixelBufferGetPixelFormatType, CVPixelBufferGetPlaneCount,
    CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags,
    CVPixelBufferUnlockBaseAddress, kCVPixelBufferHeightKey, kCVPixelBufferPixelFormatTypeKey,
    kCVPixelBufferWidthKey, kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange, kCVReturnSuccess,
};
use objc2_foundation::{
    NSArray, NSDictionary, NSError, NSNotification, NSNotificationCenter, NSNotificationName,
    NSNumber, NSObject, NSObjectProtocol, NSString,
};

use super::mode::{self, Choice, Mode, Rate};
use crate::convert::{Picture, to_nv12};
use crate::mode::Geometry;
use crate::{
    CameraBackend, CameraDevice, CameraError, CameraFrame, CaptureRequest, CaptureSession,
    DeviceId, FramePool, FrameSink, HotplugCallback, HotplugWatch, POOL_BUFFERS, nv12_len,
};

/// Backend name in the device ids (`DeviceId::from_os_key`).
const BACKEND: &str = "avf";

/// More undecodable frames in a row than this end the session (FR-8.26).
const MAX_UNREADABLE: u32 = 30;

/// `AVErrorApplicationIsNotAuthorizedToUseDevice`: the user refused camera access.
const AV_ERROR_NOT_AUTHORIZED: isize = -11852;

/// The AVFoundation camera backend.
#[derive(Default)]
pub struct MacCamera;

impl MacCamera {
    pub fn new() -> Self {
        Self
    }
}

impl CameraBackend for MacCamera {
    fn devices(&self) -> Result<Vec<CameraDevice>, CameraError> {
        let video = video()?;
        // SAFETY: plain AVFoundation queries with a valid media type.
        let default = unsafe { AVCaptureDevice::defaultDeviceWithMediaType(video) }
            .map(|device| unsafe { device.uniqueID() });
        Ok(cameras(video)
            .iter()
            .map(|device| {
                // SAFETY: a device from the discovery session.
                let key = unsafe { device.uniqueID() };
                CameraDevice {
                    id: DeviceId::from_os_key(BACKEND, &key.to_string()),
                    // SAFETY: as above.
                    name: unsafe { device.localizedName() }.to_string(),
                    is_default: default.as_deref() == Some(&*key),
                }
            })
            .collect())
    }

    fn watch(&self, changed: HotplugCallback) -> Result<Option<HotplugWatch>, CameraError> {
        let mut observers = Observers::default();
        // SAFETY: AVFoundation's own notification names, as extern statics.
        let names = unsafe {
            [
                AVCaptureDeviceWasConnectedNotification,
                AVCaptureDeviceWasDisconnectedNotification,
            ]
        };
        for name in names {
            let changed = changed.clone();
            observers.add(name, None, move || changed());
        }
        Ok(Some(Box::new(observers)))
    }

    fn open(
        &self,
        device: &DeviceId,
        request: CaptureRequest,
        sink: Box<dyn FrameSink>,
    ) -> Result<Box<dyn CaptureSession>, CameraError> {
        let video = video()?;
        ensure_access(video)?;
        let camera = cameras(video)
            .iter()
            // SAFETY: a device from the discovery session.
            .find(|camera| {
                DeviceId::from_os_key(BACKEND, &unsafe { camera.uniqueID() }.to_string()) == *device
            })
            .ok_or(CameraError::NoCamera)?;
        Ok(Box::new(MacSession::start(&camera, request, sink)?))
    }
}

fn video() -> Result<&'static AVMediaType, CameraError> {
    // SAFETY: AVFoundation's extern static.
    unsafe { AVMediaTypeVideo }.ok_or_else(|| CameraError::Backend("no video media type".into()))
}

/// The cameras AVFoundation offers: built-in ones and external ones (USB, virtual, Continuity).
fn cameras(video: &AVMediaType) -> Retained<NSArray<AVCaptureDevice>> {
    // `ExternalUnknown` is deprecated from macOS 14 on in favour of `External`, but `External`
    // does not exist before 14 and a missing symbol would stop the app from launching there. On
    // 14 and later the old name still finds the same devices.
    #[allow(deprecated)]
    // SAFETY: AVFoundation's extern statics, both present since macOS 10.15.
    let types: [&AVCaptureDeviceType; 2] = unsafe {
        [
            AVCaptureDeviceTypeBuiltInWideAngleCamera,
            objc2_av_foundation::AVCaptureDeviceTypeExternalUnknown,
        ]
    };
    let types = NSArray::from_slice(&types);
    // SAFETY: valid device types and media type.
    unsafe {
        AVCaptureDeviceDiscoverySession::discoverySessionWithDeviceTypes_mediaType_position(
            &types,
            Some(video),
            AVCaptureDevicePosition::Unspecified,
        )
        .devices()
    }
}

/// Asks for camera access on first use, which shows the system prompt with the Info.plist text;
/// blocks until the user answers. A refusal now or earlier is [`CameraError::PermissionDenied`].
fn ensure_access(video: &AVMediaType) -> Result<(), CameraError> {
    // SAFETY: plain AVFoundation call with a valid media type.
    let status = unsafe { AVCaptureDevice::authorizationStatusForMediaType(video) };
    if status == AVAuthorizationStatus::Authorized {
        return Ok(());
    }
    if status != AVAuthorizationStatus::NotDetermined {
        return Err(CameraError::PermissionDenied);
    }
    let (tx, rx) = mpsc::sync_channel(1);
    let answered = RcBlock::new(move |granted: Bool| {
        // The open call is still waiting for this; it cannot have gone away.
        let _ = tx.try_send(granted.as_bool());
    });
    // SAFETY: the handler runs once on an arbitrary queue; it only sends on a channel.
    unsafe { AVCaptureDevice::requestAccessForMediaType_completionHandler(video, &answered) };
    match rx.recv() {
        Ok(true) => Ok(()),
        Ok(false) => Err(CameraError::PermissionDenied),
        Err(_) => Err(CameraError::Backend(
            "the camera permission request did not answer".into(),
        )),
    }
}

/// Notification observers on the default centre; dropping removes them.
#[derive(Default)]
struct Observers(Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>);

// SAFETY: NSNotificationCenter is thread-safe; the tokens are only handed back to it.
unsafe impl Send for Observers {}

impl Observers {
    /// Calls `on_post` for every `name` notification (from `object` only, when given). It runs
    /// on the posting thread.
    fn add(
        &mut self,
        name: &NSNotificationName,
        object: Option<&AnyObject>,
        on_post: impl Fn() + 'static,
    ) {
        let block = RcBlock::new(move |_: NonNull<NSNotification>| on_post());
        // SAFETY: a valid name, object and block; no queue means the posting thread.
        let token = unsafe {
            NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
                Some(name),
                object,
                None,
                &block,
            )
        };
        self.0.push(token);
    }
}

impl Drop for Observers {
    fn drop(&mut self) {
        let centre = NSNotificationCenter::defaultCenter();
        for token in &self.0 {
            // SAFETY: a token this centre handed out.
            unsafe { centre.removeObserver(AsRef::<AnyObject>::as_ref(&**token)) };
        }
    }
}

/// What the capture queue needs. Only touched on the session's serial dispatch queue.
struct Delivery {
    pool: FramePool,
    /// The crop of the delivered pictures; its size is that of every frame.
    geometry: Geometry,
    started: Instant,
    state: UnsafeCell<QueueState>,
}

struct QueueState {
    sink: Box<dyn FrameSink>,
    unreadable: u32,
    /// The sink got `ended`, or the session is being dropped: no more sink calls.
    done: bool,
}

// SAFETY: `state` is only accessed through `on_queue`, whose callers all run on the session's
// one serial dispatch queue, so there is never more than one access at a time. Everything else is
// immutable or thread-safe.
unsafe impl Sync for Delivery {}

impl Delivery {
    /// # Safety
    ///
    /// Only call on the session's serial dispatch queue.
    unsafe fn on_queue<R>(&self, f: impl FnOnce(&mut QueueState) -> R) -> R {
        // SAFETY: the queue serialises all access (see the `Sync` impl).
        f(unsafe { &mut *self.state.get() })
    }

    /// Ends the session with `reason` unless it already ended. Queue only (see `on_queue`).
    unsafe fn end(&self, reason: CameraError) {
        // SAFETY: forwarded from the caller.
        unsafe {
            self.on_queue(|state| {
                if !state.done {
                    state.done = true;
                    state.sink.ended(reason);
                }
            });
        }
    }
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements and the handler overrides nothing.
    #[unsafe(super(NSObject))]
    #[name = "MumcordCameraFrameHandler"]
    #[ivars = Arc<Delivery>]
    struct FrameHandler;

    unsafe impl NSObjectProtocol for FrameHandler {}

    unsafe impl AVCaptureVideoDataOutputSampleBufferDelegate for FrameHandler {
        #[unsafe(method(captureOutput:didOutputSampleBuffer:fromConnection:))]
        fn did_output(
            &self,
            _output: &AVCaptureOutput,
            buffer: &CMSampleBuffer,
            _connection: &AVCaptureConnection,
        ) {
            // SAFETY: AVFoundation calls this on the queue the delegate was set with.
            unsafe { deliver(self.ivars(), buffer) };
        }
    }
);

impl FrameHandler {
    fn new(delivery: Arc<Delivery>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(delivery);
        // SAFETY: NSObject's plain initializer.
        unsafe { msg_send![super(this), init] }
    }
}

enum Copied {
    Frame(CameraFrame),
    /// Every pool buffer is still in use: the consumer is behind, the frame is skipped.
    NoBuffer,
    Unreadable,
}

/// Hands one picture to the sink. Real-time path: no allocation, lock, I/O or logging.
///
/// # Safety
///
/// Only call on the session's serial dispatch queue.
unsafe fn deliver(delivery: &Delivery, buffer: &CMSampleBuffer) {
    // SAFETY: forwarded from the caller.
    let ended = unsafe {
        delivery.on_queue(|state| {
            if state.done {
                return false;
            }
            match copy_frame(delivery, buffer) {
                Copied::Frame(frame) => {
                    state.unreadable = 0;
                    state.sink.frame(frame);
                }
                Copied::NoBuffer => {}
                Copied::Unreadable => state.unreadable += 1,
            }
            state.unreadable > MAX_UNREADABLE
        })
    };
    if ended {
        // SAFETY: still on the queue.
        unsafe { delivery.end(CameraError::Unreadable) };
    }
}

/// Copies the cropped NV12 picture out of the system's buffer into a pool buffer.
fn copy_frame(delivery: &Delivery, buffer: &CMSampleBuffer) -> Copied {
    // SAFETY: a valid sample buffer from the video output.
    let Some(pixels) = (unsafe { buffer.image_buffer() }) else {
        return Copied::Unreadable;
    };
    if CVPixelBufferGetPixelFormatType(&pixels) != kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange
        || CVPixelBufferGetPlaneCount(&pixels) != 2
    {
        return Copied::Unreadable;
    }
    let Some(mut out) = delivery.pool.acquire() else {
        return Copied::NoBuffer;
    };
    // SAFETY: locked for reading only, unlocked below before the buffer is released.
    if unsafe { CVPixelBufferLockBaseAddress(&pixels, CVPixelBufferLockFlags::ReadOnly) }
        != kCVReturnSuccess
    {
        return Copied::Unreadable;
    }
    let plane = |index: usize| {
        let base = CVPixelBufferGetBaseAddressOfPlane(&pixels, index);
        let stride = CVPixelBufferGetBytesPerRowOfPlane(&pixels, index);
        let rows = CVPixelBufferGetHeightOfPlane(&pixels, index);
        if base.is_null() {
            return &[][..];
        }
        // SAFETY: the locked plane holds `rows` rows of `stride` bytes.
        unsafe { std::slice::from_raw_parts(base.cast::<u8>(), stride * rows) }
    };
    let (width, height) = (
        CVPixelBufferGetWidth(&pixels) as u32,
        CVPixelBufferGetHeight(&pixels) as u32,
    );
    let picture = Picture::Nv12 {
        y: plane(0),
        y_stride: CVPixelBufferGetBytesPerRowOfPlane(&pixels, 0),
        uv: plane(1),
        uv_stride: CVPixelBufferGetBytesPerRowOfPlane(&pixels, 1),
    };
    let copied = to_nv12(picture, width, height, &delivery.geometry, &mut out);
    // SAFETY: matches the lock above.
    unsafe { CVPixelBufferUnlockBaseAddress(&pixels, CVPixelBufferLockFlags::ReadOnly) };
    if !copied {
        return Copied::Unreadable;
    }
    Copied::Frame(CameraFrame {
        width: delivery.geometry.width,
        height: delivery.geometry.height,
        timestamp: delivery.started.elapsed(),
        data: out,
    })
}

/// A running AVFoundation capture.
struct MacSession {
    session: Retained<AVCaptureSession>,
    output: Retained<AVCaptureVideoDataOutput>,
    _handler: Retained<FrameHandler>,
    queue: DispatchRetained<DispatchQueue>,
    observers: Option<Observers>,
    delivery: Arc<Delivery>,
}

// SAFETY: AVCaptureSession and its outputs may be started, stopped and reconfigured from any
// thread; the handler is only called by AVFoundation on `queue`.
unsafe impl Send for MacSession {}

impl CaptureSession for MacSession {}

impl MacSession {
    fn start(
        device: &AVCaptureDevice,
        request: CaptureRequest,
        sink: Box<dyn FrameSink>,
    ) -> Result<Self, CameraError> {
        // SAFETY: a device from the discovery session.
        let formats = unsafe { device.formats() };
        let modes: Vec<Mode> = formats.iter().map(|format| mode_of(&format)).collect();
        let choice = mode::choose(&modes, request)
            .ok_or_else(|| CameraError::Backend("the camera offers no video format".into()))?;
        let format = formats.objectAtIndex(choice.index);

        // SAFETY: a valid device; errors come back as NSError.
        let input = unsafe { AVCaptureDeviceInput::deviceInputWithDevice_error(device) }
            .map_err(|error| av_error(&error))?;

        let delivery = Arc::new(Delivery {
            pool: FramePool::new(
                POOL_BUFFERS,
                nv12_len(choice.geometry.width, choice.geometry.height),
            ),
            geometry: choice.geometry,
            started: Instant::now(),
            state: UnsafeCell::new(QueueState {
                sink,
                unreadable: 0,
                done: false,
            }),
        });
        let handler = FrameHandler::new(delivery.clone());
        let queue = DispatchQueue::new("app.mumcord.camera", None);
        let output = video_output(&choice, &handler, &queue);

        // SAFETY: AVCaptureSession calls in the documented order: configure inside
        // begin/commit, choose the format while the device is locked, start, then unlock so
        // the session keeps the format instead of applying its preset.
        let session = unsafe {
            let session = AVCaptureSession::new();
            session.beginConfiguration();
            if !session.canAddInput(&input) || !session.canAddOutput(&output) {
                session.commitConfiguration();
                return Err(CameraError::Backend(
                    "the capture session refused the camera".into(),
                ));
            }
            session.addInput(&input);
            session.addOutput(&output);
            if let Err(error) = device.lockForConfiguration() {
                session.commitConfiguration();
                return Err(av_error(&error));
            }
            device.setActiveFormat(&format);
            if let Some(duration) = frame_duration(&format, request.max_fps) {
                device.setActiveVideoMinFrameDuration(duration);
            }
            session.commitConfiguration();
            session.startRunning();
            device.unlockForConfiguration();
            session
        };

        let mut started = Self {
            session,
            output,
            _handler: handler,
            queue,
            observers: None,
            delivery,
        };
        started.observe(device);
        // SAFETY: a configured session.
        if !unsafe { started.session.isRunning() } {
            return Err(CameraError::Backend("the camera did not start".into()));
        }
        Ok(started)
    }

    /// Ends the session when the camera is unplugged or the capture fails.
    fn observe(&mut self, device: &AVCaptureDevice) {
        let mut observers = Observers::default();
        let end_on_queue = |reason: CameraError| {
            let (queue, delivery) = (self.queue.clone(), self.delivery.clone());
            move || {
                let (delivery, reason) = (delivery.clone(), reason.clone());
                // SAFETY: the block runs on the session's queue.
                queue.exec_async(move || unsafe { delivery.end(reason) });
            }
        };
        // SAFETY: AVFoundation's own notification names, as extern statics.
        unsafe {
            observers.add(
                AVCaptureDeviceWasDisconnectedNotification,
                Some(device),
                end_on_queue(CameraError::DeviceGone),
            );
            observers.add(
                AVCaptureSessionRuntimeErrorNotification,
                Some(&self.session),
                end_on_queue(CameraError::Backend("the capture session failed".into())),
            );
        }
        self.observers = Some(observers);
    }
}

impl Drop for MacSession {
    fn drop(&mut self) {
        // No new end notifications; then stop the camera (its light goes off) and detach the
        // handler.
        self.observers.take();
        // SAFETY: stopping a session and clearing its delegate are always allowed.
        unsafe {
            self.session.stopRunning();
            self.output.setSampleBufferDelegate_queue(None, None);
        }
        // Frames or end notifications already queued run before this, and see `done` after it:
        // no sink call happens once the drop returns.
        let delivery = self.delivery.clone();
        // SAFETY: the block runs on the session's queue.
        self.queue
            .exec_sync(move || unsafe { delivery.on_queue(|state| state.done = true) });
    }
}

/// The video output: NV12 video range at the chosen size, late frames dropped, frames to
/// `handler` on `queue`.
fn video_output(
    choice: &Choice,
    handler: &FrameHandler,
    queue: &DispatchQueue,
) -> Retained<AVCaptureVideoDataOutput> {
    // The CoreVideo keys are CFStrings, toll-free bridged to NSString.
    fn key<CFString>(key: &'static CFString) -> &'static NSString {
        // SAFETY: only called with CFString keys, the same objects as NSStrings (toll-free
        // bridging).
        unsafe { &*std::ptr::from_ref(key).cast::<NSString>() }
    }
    let format = NSNumber::new_u32(kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange);
    let width = NSNumber::new_u32(choice.output.0);
    let height = NSNumber::new_u32(choice.output.1);
    // SAFETY: CoreVideo's extern statics.
    let keys = unsafe {
        [
            key(kCVPixelBufferPixelFormatTypeKey),
            key(kCVPixelBufferWidthKey),
            key(kCVPixelBufferHeightKey),
        ]
    };
    let values: [&AnyObject; 3] = [&format, &width, &height];
    let settings = NSDictionary::<NSString, AnyObject>::from_slices(&keys, &values);
    // SAFETY: a fresh output, a dictionary of the documented keys and NSNumber values, a serial
    // queue that outlives the delegate (see `MacSession::drop`).
    unsafe {
        let output = AVCaptureVideoDataOutput::new();
        output.setVideoSettings(Some(&settings));
        output.setAlwaysDiscardsLateVideoFrames(true);
        output.setSampleBufferDelegate_queue(Some(ProtocolObject::from_ref(handler)), Some(queue));
        output
    }
}

/// Size and fastest frame rate of one camera format.
fn mode_of(format: &AVCaptureDeviceFormat) -> Mode {
    // SAFETY: plain queries on a format of the device.
    unsafe {
        let size = CMVideoFormatDescriptionGetDimensions(&format.formatDescription());
        let max_fps = format
            .videoSupportedFrameRateRanges()
            .iter()
            .map(|range| range.maxFrameRate())
            .fold(0.0, f64::max);
        Mode {
            width: size.width.max(0) as u32,
            height: size.height.max(0) as u32,
            max_fps,
        }
    }
}

/// The shortest frame duration to set for `fps`, always one the format accepts (setting any
/// other throws an Objective-C exception, which would abort the app).
fn frame_duration(format: &AVCaptureDeviceFormat, fps: u32) -> Option<CMTime> {
    // SAFETY: plain queries on a format of the device.
    unsafe {
        let ranges = format.videoSupportedFrameRateRanges();
        let rates: Vec<(f64, f64)> = ranges
            .iter()
            .map(|range| (range.minFrameRate(), range.maxFrameRate()))
            .collect();
        match mode::frame_rate(&rates, fps)? {
            Rate::Wanted(fps) => Some(CMTime::new(1, i32::try_from(fps).ok()?)),
            Rate::RangeMax(index) => Some(ranges.objectAtIndex(index).minFrameDuration()),
        }
    }
}

/// Maps an AVFoundation error; never includes device names (FR-8.28).
fn av_error(error: &NSError) -> CameraError {
    if error.code() == AV_ERROR_NOT_AUTHORIZED {
        CameraError::PermissionDenied
    } else {
        CameraError::Backend(format!("AVFoundation error {}", error.code()))
    }
}
