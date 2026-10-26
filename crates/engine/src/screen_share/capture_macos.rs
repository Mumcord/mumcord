//! Screen capture on macOS through ScreenCaptureKit (FR-2.42 – FR-2.46).
//!
//! macOS 14+ shows the system picker (`SCContentSharingPicker`), where the user picks a window,
//! an app or a screen. macOS 12.3 and 13 have ScreenCaptureKit but no system picker; the spec
//! asks for the system picker and no own picker UI, so there the main screen is shared whole,
//! as on X11. The stream delivers BGRA pictures, already scaled by the system into the encoder's
//! largest box, and with macOS 13+ also the sound of exactly the shared content without
//! Mumcord's own output ([`super::sound_macos`]). On a whole screen that is every app's sound
//! but Mumcord's.
//!
//! Screen recording needs the user's permission. Without it the share fails with
//! [`MacCaptureError::ScreenRecordingDenied`], which the UI explains with a pointer to System
//! Settings. The picker on 14+ is its own consent, so the permission matters on 12.3 and 13.
//!
//! The parts without macOS calls are compiled everywhere for the tests.

use super::convert::fit;

/// The encoder's largest picture (720p30); the system scales into it, so a 5K screen costs no
/// more than a 720p one.
const CAPTURE_BOX: (u32, u32) = (1280, 720);

/// Why the screen cannot be captured.
#[derive(Debug, thiserror::Error)]
pub enum MacCaptureError {
    /// The user closed the system picker without choosing.
    #[error("the picker was closed")]
    Cancelled,
    #[error("screen recording is not allowed for Mumcord in System Settings")]
    ScreenRecordingDenied,
    #[error("screen sharing needs macOS 12.3 or newer")]
    Unsupported,
    #[error("screen capture failed: {0}")]
    Failed(String),
}

/// How the content to share is chosen on a macOS version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plan {
    /// Before 12.3: no ScreenCaptureKit.
    Unsupported,
    /// 12.3 and 13: no system picker, the main screen is shared whole.
    MainDisplay,
    /// 14+: the system picker.
    Picker,
}

pub fn plan(major: i64, minor: i64) -> Plan {
    match (major, minor) {
        (14.., _) => Plan::Picker,
        (13, _) | (12, 3..) => Plan::MainDisplay,
        _ => Plan::Unsupported,
    }
}

/// ScreenCaptureKit captures sound from macOS 13 on (FR-2.42).
pub fn sound_supported(major: i64) -> bool {
    major >= 13
}

/// Stream size for content of `width` × `height` pixels: fitted into [`CAPTURE_BOX`], never
/// enlarged; the box itself when the size is unknown.
pub fn stream_size(width: f64, height: f64) -> (u32, u32) {
    let px = |v: f64| {
        if v.is_finite() && v >= 1.0 {
            v.min(f64::from(u32::MAX)) as u32
        } else {
            0
        }
    };
    fit(px(width), px(height), CAPTURE_BOX.0, CAPTURE_BOX.1).unwrap_or(CAPTURE_BOX)
}

/// Whether ScreenCaptureKit's sound is what the share sends: 48 kHz mono 32-bit float, as
/// configured. Anything else is not converted but left out.
pub fn is_share_sound(sample_rate: f64, float: bool, bits: u32, channels: u32) -> bool {
    sample_rate == f64::from(mumcord_audio::SAMPLE_RATE) && float && bits == 32 && channels == 1
}

#[cfg(target_os = "macos")]
pub use sck::{MacCapture, os_version};

#[cfg(target_os = "macos")]
mod sck {
    use std::cell::Cell;
    use std::ptr::NonNull;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    use block2::RcBlock;
    use dispatch2::{DispatchQueue, DispatchRetained};
    use mumcord_screenshare::I420Frame;
    use objc2::rc::Retained;
    use objc2::runtime::ProtocolObject;
    use objc2::{AnyThread, DefinedClass, Message, define_class, msg_send};
    use objc2_core_audio_types::{kAudioFormatFlagIsFloat, kAudioFormatLinearPCM};
    use objc2_core_graphics::{
        CGMainDisplayID, CGPreflightScreenCaptureAccess, CGRequestScreenCaptureAccess,
    };
    use objc2_core_media::{
        CMAudioFormatDescriptionGetStreamBasicDescription, CMSampleBuffer, CMTime,
    };
    use objc2_core_video::{
        CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow, CVPixelBufferGetHeight,
        CVPixelBufferGetPixelFormatType, CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress,
        CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress, kCVPixelFormatType_32BGRA,
        kCVReturnSuccess,
    };
    use objc2_foundation::{NSArray, NSError, NSObject, NSObjectProtocol, NSProcessInfo};
    use objc2_screen_capture_kit::{
        SCContentFilter, SCContentSharingPicker, SCContentSharingPickerConfiguration,
        SCContentSharingPickerMode, SCContentSharingPickerObserver, SCShareableContent, SCStream,
        SCStreamConfiguration, SCStreamDelegate, SCStreamErrorCode, SCStreamOutput,
        SCStreamOutputType,
    };
    use tokio::sync::oneshot;

    use super::super::capture::{CaptureSource, SoundSource};
    use super::super::convert::{bgrx_to_i420, pack_rows};
    use super::super::sound_macos::{MacSound, SoundSender, sound_channel};
    use super::{MacCaptureError, Plan, is_share_sound, plan, sound_supported, stream_size};
    use crate::session::lock;

    /// Frames per second the stream delivers at most, the encoder's top rate.
    const CAPTURE_FPS: i32 = 30;

    /// The running macOS version as (major, minor).
    pub fn os_version() -> (i64, i64) {
        let version = NSProcessInfo::processInfo().operatingSystemVersion();
        (version.majorVersion as i64, version.minorVersion as i64)
    }

    /// ScreenCaptureKit objects moved between the runtime, the main queue and the capture.
    /// Apple marks them as safe to use from any thread (`Sendable` in Swift); only the moves
    /// happen here, never shared access without the framework's own locking.
    struct Moved<T>(T);
    // SAFETY: see above; only used for ScreenCaptureKit objects.
    unsafe impl<T> Send for Moved<T> {}

    /// The newest picture, as tight BGRA.
    #[derive(Default)]
    struct Latest {
        data: Vec<u8>,
        width: u32,
        height: u32,
        fresh: bool,
    }

    /// The share's sound feed with its sample buffer, so delivery does not allocate each time.
    struct SoundFeed {
        sender: SoundSender,
        samples: Vec<f32>,
    }

    #[derive(Default)]
    struct Shared {
        latest: Mutex<Latest>,
        sound: Mutex<Option<SoundFeed>>,
        ended: AtomicBool,
        /// A sound format we cannot send was reported once already.
        odd_sound: AtomicBool,
    }

    define_class!(
        // SAFETY: NSObject has no subclassing requirements and the handler overrides nothing.
        #[unsafe(super(NSObject))]
        #[name = "MumcordScreenShareHandler"]
        #[ivars = Arc<Shared>]
        struct StreamHandler;

        unsafe impl NSObjectProtocol for StreamHandler {}

        unsafe impl SCStreamOutput for StreamHandler {
            #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
            fn did_output(
                &self,
                _stream: &SCStream,
                buffer: &CMSampleBuffer,
                kind: SCStreamOutputType,
            ) {
                if kind == SCStreamOutputType::Screen {
                    picture(self.ivars(), buffer);
                } else if kind == SCStreamOutputType::Audio {
                    sound(self.ivars(), buffer);
                }
            }
        }

        unsafe impl SCStreamDelegate for StreamHandler {
            #[unsafe(method(stream:didStopWithError:))]
            fn did_stop(&self, _stream: &SCStream, error: &NSError) {
                // The menu bar's stop button, the shared window closed, or the display is gone.
                tracing::info!(code = error.code(), "the system ended the screen share");
                self.ivars().ended.store(true, Ordering::Relaxed);
            }
        }
    );

    impl StreamHandler {
        fn new(shared: Arc<Shared>) -> Retained<Self> {
            let this = Self::alloc().set_ivars(shared);
            // SAFETY: NSObject's plain initializer.
            unsafe { msg_send![super(this), init] }
        }
    }

    type PickResult = Result<Moved<Retained<SCContentFilter>>, MacCaptureError>;

    define_class!(
        // SAFETY: NSObject has no subclassing requirements and the observer overrides nothing.
        #[unsafe(super(NSObject))]
        #[name = "MumcordScreenSharePickerObserver"]
        #[ivars = Mutex<Option<oneshot::Sender<PickResult>>>]
        struct PickerObserver;

        unsafe impl NSObjectProtocol for PickerObserver {}

        unsafe impl SCContentSharingPickerObserver for PickerObserver {
            #[unsafe(method(contentSharingPicker:didCancelForStream:))]
            fn did_cancel(&self, _picker: &SCContentSharingPicker, _stream: Option<&SCStream>) {
                self.answer(Err(MacCaptureError::Cancelled));
            }

            #[unsafe(method(contentSharingPicker:didUpdateWithFilter:forStream:))]
            fn did_pick(
                &self,
                _picker: &SCContentSharingPicker,
                filter: &SCContentFilter,
                _stream: Option<&SCStream>,
            ) {
                self.answer(Ok(Moved(filter.retain())));
            }

            #[unsafe(method(contentSharingPickerStartDidFailWithError:))]
            fn did_fail(&self, error: &NSError) {
                self.answer(Err(stream_error(error)));
            }
        }
    );

    impl PickerObserver {
        fn new(tx: oneshot::Sender<PickResult>) -> Retained<Self> {
            let this = Self::alloc().set_ivars(Mutex::new(Some(tx)));
            // SAFETY: NSObject's plain initializer.
            unsafe { msg_send![super(this), init] }
        }

        /// Only the first answer counts; the picker may report more while it stays active.
        fn answer(&self, result: PickResult) {
            if let Some(tx) = lock(self.ivars()).take() {
                // The open call may have been dropped meanwhile; then nobody waits.
                let _ = tx.send(result);
            }
        }
    }

    fn stream_error(error: &NSError) -> MacCaptureError {
        if error.code() == SCStreamErrorCode::UserDeclined.0 {
            MacCaptureError::ScreenRecordingDenied
        } else {
            MacCaptureError::Failed(format!(
                "{} ({})",
                error.localizedDescription(),
                error.code()
            ))
        }
    }

    /// Copies the newest picture out of the system's buffer. Runs on the stream's queue.
    fn picture(shared: &Shared, buffer: &CMSampleBuffer) {
        // Frames where nothing changed come without a picture; the last one still stands.
        // SAFETY: a valid sample buffer from the stream.
        let Some(pixels) = (unsafe { buffer.image_buffer() }) else {
            return;
        };
        if CVPixelBufferGetPixelFormatType(&pixels) != kCVPixelFormatType_32BGRA {
            return;
        }
        // SAFETY: locked for reading only, unlocked below before the buffer is released.
        if unsafe { CVPixelBufferLockBaseAddress(&pixels, CVPixelBufferLockFlags::ReadOnly) }
            != kCVReturnSuccess
        {
            return;
        }
        let base = CVPixelBufferGetBaseAddress(&pixels);
        let stride = CVPixelBufferGetBytesPerRow(&pixels);
        let (width, height) = (
            CVPixelBufferGetWidth(&pixels),
            CVPixelBufferGetHeight(&pixels),
        );
        if !base.is_null() && width > 0 && height > 0 && stride >= width * 4 {
            // SAFETY: the locked base address holds `height` rows of `stride` bytes; the last
            // row is only read up to its pixels.
            let src = unsafe {
                std::slice::from_raw_parts(base.cast::<u8>(), stride * (height - 1) + width * 4)
            };
            let mut latest = lock(&shared.latest);
            if pack_rows(src, stride, width as u32, height as u32, &mut latest.data) {
                latest.width = width as u32;
                latest.height = height as u32;
                latest.fresh = true;
            }
        }
        // SAFETY: matches the lock above.
        unsafe { CVPixelBufferUnlockBaseAddress(&pixels, CVPixelBufferLockFlags::ReadOnly) };
    }

    /// Hands the shared content's sound to the share. Runs on the sound queue.
    fn sound(shared: &Shared, buffer: &CMSampleBuffer) {
        let mut feed = lock(&shared.sound);
        let Some(feed) = feed.as_mut() else {
            return;
        };
        // SAFETY: a valid sample buffer from the stream.
        let Some(format) = (unsafe { buffer.format_description() }) else {
            return;
        };
        // SAFETY: an audio format description, since this is the audio output.
        let Some(asbd) =
            (unsafe { CMAudioFormatDescriptionGetStreamBasicDescription(&format).as_ref() })
        else {
            return;
        };
        let float = asbd.mFormatID == kAudioFormatLinearPCM
            && asbd.mFormatFlags & kAudioFormatFlagIsFloat != 0;
        if !is_share_sound(
            asbd.mSampleRate,
            float,
            asbd.mBitsPerChannel,
            asbd.mChannelsPerFrame,
        ) {
            if !shared.odd_sound.swap(true, Ordering::Relaxed) {
                tracing::warn!(
                    rate = asbd.mSampleRate,
                    channels = asbd.mChannelsPerFrame,
                    bits = asbd.mBitsPerChannel,
                    "screen share sound has an unexpected format and is left out"
                );
            }
            return;
        }
        // SAFETY: a valid sample buffer from the stream.
        let Some(data) = (unsafe { buffer.data_buffer() }) else {
            return;
        };
        // SAFETY: the block buffer is valid while `data` is held.
        let len = unsafe { data.data_length() } / size_of::<f32>();
        feed.samples.resize(len, 0.0);
        if len == 0 {
            return;
        }
        let dest = NonNull::from(&mut feed.samples[..]).cast();
        // SAFETY: `dest` has room for exactly `len` floats, copied byte for byte.
        if unsafe { data.copy_data_bytes(0, len * size_of::<f32>(), dest) } != 0 {
            return;
        }
        let SoundFeed { sender, samples } = feed;
        sender.push(samples);
    }

    /// A running ScreenCaptureKit share. Dropping it stops the stream, which also removes the
    /// system's "screen is being shared" indicator.
    pub struct MacCapture {
        shared: Arc<Shared>,
        /// Picture buffer swapped with the slot, so neither side allocates per frame.
        spare: Vec<u8>,
        /// Until the share takes it ([`CaptureSource::take_sound`]).
        sound: Option<MacSound>,
        stream: Retained<SCStream>,
        _handler: Retained<StreamHandler>,
        _queues: [DispatchRetained<DispatchQueue>; 2],
    }

    // SAFETY: ScreenCaptureKit's stream may be stopped from any thread; the handler's state is
    // behind locks and atomics, and the dispatch queues are thread-safe.
    unsafe impl Send for MacCapture {}

    impl MacCapture {
        /// Lets the user pick what to share (macOS 14+) or takes the main screen (12.3, 13) and
        /// starts capturing it, with the shared content's sound where the system has it.
        pub async fn open() -> Result<Self, MacCaptureError> {
            let (major, minor) = os_version();
            let (filter, size) = match plan(major, minor) {
                Plan::Unsupported => return Err(MacCaptureError::Unsupported),
                Plan::MainDisplay => main_display().await?,
                Plan::Picker => {
                    let filter = pick().await?;
                    // SAFETY: macOS 14+ API on the picked filter.
                    let (rect, scale) = unsafe {
                        (
                            filter.0.contentRect(),
                            f64::from(filter.0.pointPixelScale()),
                        )
                    };
                    let size = stream_size(rect.size.width * scale, rect.size.height * scale);
                    (filter, size)
                }
            };
            start(filter, size, sound_supported(major)).await
        }
    }

    type Filter = Moved<Retained<SCContentFilter>>;

    /// The system picker. Shown from the main queue, where AppKit expects UI to come from.
    async fn pick() -> Result<Filter, MacCaptureError> {
        let (tx, rx) = oneshot::channel();
        let observer = Moved(PickerObserver::new(tx));
        let show = Moved(observer.0.clone());
        DispatchQueue::main().exec_async(move || {
            // The whole wrapper moves into the closure, not just its field.
            let show = show;
            // SAFETY: plain ScreenCaptureKit calls on the main queue; macOS 14+ is checked by
            // the caller.
            unsafe {
                let picker = SCContentSharingPicker::sharedPicker();
                let config = SCContentSharingPickerConfiguration::new();
                config.setAllowedPickerModes(
                    SCContentSharingPickerMode::SingleWindow
                        | SCContentSharingPickerMode::SingleApplication
                        | SCContentSharingPickerMode::SingleDisplay,
                );
                // Switching content mid-share would need the stream updated; a new share is
                // as quick.
                config.setAllowsChangingSelectedContent(false);
                picker.setDefaultConfiguration(&config);
                picker.addObserver(ProtocolObject::from_ref(&*show.0));
                picker.setActive(true);
                picker.present();
            }
        });
        let picked = rx.await;
        DispatchQueue::main().exec_async(move || {
            let observer = observer;
            // SAFETY: as above.
            unsafe {
                let picker = SCContentSharingPicker::sharedPicker();
                picker.removeObserver(ProtocolObject::from_ref(&*observer.0));
                picker.setActive(false);
            }
        });
        picked.map_err(|_| MacCaptureError::Failed("the picker gave no answer".into()))?
    }

    /// The main screen with its size, after checking the screen recording permission (macOS
    /// 12.3 and 13).
    async fn main_display() -> Result<(Filter, (u32, u32)), MacCaptureError> {
        if !CGPreflightScreenCaptureAccess() {
            // Shows the system's prompt the first time, which leads to System Settings; the
            // permission only takes effect after Mumcord restarts.
            CGRequestScreenCaptureAccess();
            return Err(MacCaptureError::ScreenRecordingDenied);
        }
        let (tx, rx) = oneshot::channel();
        {
            let tx = Cell::new(Some(tx));
            let handler = RcBlock::new(
                move |content: *mut SCShareableContent, error: *mut NSError| {
                    let Some(tx) = tx.take() else {
                        return;
                    };
                    // SAFETY: ScreenCaptureKit hands over valid objects or null.
                    let result = match unsafe { (content.as_ref(), error.as_ref()) } {
                        (Some(content), _) => Ok(Moved(content.retain())),
                        (None, Some(error)) => Err(stream_error(error)),
                        (None, None) => Err(MacCaptureError::Failed("no shareable content".into())),
                    };
                    // The open call may have been dropped meanwhile; then nobody waits.
                    let _ = tx.send(result);
                },
            );
            // SAFETY: the completion handler matches the documented signature.
            unsafe { SCShareableContent::getShareableContentWithCompletionHandler(&handler) };
        }
        let content = rx
            .await
            .map_err(|_| MacCaptureError::Failed("no shareable content".into()))??;
        // SAFETY: plain ScreenCaptureKit calls on a valid content list.
        unsafe {
            let displays = content.0.displays();
            let main = CGMainDisplayID();
            let display = displays
                .iter()
                .find(|d| d.displayID() == main)
                .or_else(|| displays.firstObject())
                .ok_or_else(|| MacCaptureError::Failed("no display to share".into()))?;
            let size = stream_size(display.width() as f64, display.height() as f64);
            let filter = SCContentFilter::initWithDisplay_excludingWindows(
                SCContentFilter::alloc(),
                &display,
                &NSArray::new(),
            );
            Ok((Moved(filter), size))
        }
    }

    /// What keeps a stream running.
    struct Running {
        stream: Retained<SCStream>,
        handler: Retained<StreamHandler>,
        queues: [DispatchRetained<DispatchQueue>; 2],
    }

    /// Starts the stream for `filter` at `size`, with its sound when `with_sound`.
    async fn start(
        filter: Filter,
        size: (u32, u32),
        with_sound: bool,
    ) -> Result<MacCapture, MacCaptureError> {
        let shared = Arc::new(Shared::default());
        let sound = with_sound.then(|| {
            let (sender, source) = sound_channel();
            *lock(&shared.sound) = Some(SoundFeed {
                sender,
                samples: Vec::new(),
            });
            source
        });
        let (tx, rx) = oneshot::channel();
        let running = Moved(begin(&filter.0, size, with_sound, &shared, tx)?);
        rx.await
            .map_err(|_| MacCaptureError::Failed("the stream did not start".into()))??;
        let Running {
            stream,
            handler,
            queues,
        } = running.0;
        Ok(MacCapture {
            shared,
            spare: Vec::new(),
            sound,
            stream,
            _handler: handler,
            _queues: queues,
        })
    }

    /// Sets the stream up and asks it to start; `started` gets the outcome.
    fn begin(
        filter: &SCContentFilter,
        (width, height): (u32, u32),
        with_sound: bool,
        shared: &Arc<Shared>,
        started: oneshot::Sender<Result<(), MacCaptureError>>,
    ) -> Result<Running, MacCaptureError> {
        let handler = StreamHandler::new(shared.clone());
        let queues = [
            DispatchQueue::new("app.mumcord.screen-share.video", None),
            DispatchQueue::new("app.mumcord.screen-share.sound", None),
        ];
        // SAFETY: plain ScreenCaptureKit calls; the sound settings exist from macOS 13 on, the
        // only versions `with_sound` is set on.
        unsafe {
            let config = SCStreamConfiguration::new();
            config.setWidth(width as usize);
            config.setHeight(height as usize);
            config.setMinimumFrameInterval(CMTime::new(1, CAPTURE_FPS));
            config.setPixelFormat(kCVPixelFormatType_32BGRA);
            config.setShowsCursor(true);
            if with_sound {
                config.setCapturesAudio(true);
                config.setSampleRate(mumcord_audio::SAMPLE_RATE as isize);
                config.setChannelCount(1);
                // Viewers never hear their own voices back from the sharer (FR-2.42).
                config.setExcludesCurrentProcessAudio(true);
            }
            let stream = SCStream::initWithFilter_configuration_delegate(
                SCStream::alloc(),
                filter,
                &config,
                Some(ProtocolObject::from_ref(&*handler)),
            );
            let output = ProtocolObject::from_ref(&*handler);
            stream
                .addStreamOutput_type_sampleHandlerQueue_error(
                    output,
                    SCStreamOutputType::Screen,
                    Some(&queues[0]),
                )
                .map_err(|e| stream_error(&e))?;
            if with_sound {
                stream
                    .addStreamOutput_type_sampleHandlerQueue_error(
                        output,
                        SCStreamOutputType::Audio,
                        Some(&queues[1]),
                    )
                    .map_err(|e| stream_error(&e))?;
            }
            let started = Cell::new(Some(started));
            let done = RcBlock::new(move |error: *mut NSError| {
                if let Some(tx) = started.take() {
                    // SAFETY: ScreenCaptureKit hands over a valid error or null.
                    let result = error.as_ref().map_or(Ok(()), |e| Err(stream_error(e)));
                    // The open call may have been dropped meanwhile; then nobody waits.
                    let _ = tx.send(result);
                }
            });
            stream.startCaptureWithCompletionHandler(Some(&done));
            Ok(Running {
                stream,
                handler,
                queues,
            })
        }
    }

    impl CaptureSource for MacCapture {
        fn ended(&self) -> bool {
            self.shared.ended.load(Ordering::Relaxed)
        }

        fn take_sound(&mut self) -> Option<Box<dyn SoundSource>> {
            self.sound
                .take()
                .map(|s| Box::new(s) as Box<dyn SoundSource>)
        }

        fn frame(&mut self, max_width: u32, max_height: u32) -> Option<I420Frame> {
            let (width, height) = {
                let mut latest = lock(&self.shared.latest);
                if !latest.fresh {
                    return None;
                }
                latest.fresh = false;
                std::mem::swap(&mut latest.data, &mut self.spare);
                (latest.width, latest.height)
            };
            bgrx_to_i420(&self.spare, width, height, max_width, max_height)
        }
    }

    impl Drop for MacCapture {
        fn drop(&mut self) {
            // Ends the sound too: the sender goes, the sound thread sees silence.
            lock(&self.shared.sound).take();
            // SAFETY: stopping a started stream; no completion handler needed.
            unsafe { self.stream.stopCaptureWithCompletionHandler(None) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_picker_needs_14_and_capture_12_3() {
        assert_eq!(plan(15, 0), Plan::Picker);
        assert_eq!(plan(14, 0), Plan::Picker);
        assert_eq!(plan(13, 6), Plan::MainDisplay);
        assert_eq!(plan(12, 3), Plan::MainDisplay);
        assert_eq!(plan(12, 2), Plan::Unsupported);
        assert_eq!(plan(11, 7), Plan::Unsupported);
    }

    #[test]
    fn sound_needs_13() {
        assert!(!sound_supported(12));
        assert!(sound_supported(13));
        assert!(sound_supported(26));
    }

    #[test]
    fn the_stream_fits_the_encoder_box() {
        assert_eq!(stream_size(5120.0, 2880.0), (1280, 720));
        assert_eq!(stream_size(3440.0, 1440.0), (1280, 534));
        assert_eq!(stream_size(800.0, 601.0), (800, 600));
        assert_eq!(stream_size(0.0, 600.0), CAPTURE_BOX);
        assert_eq!(stream_size(f64::NAN, 600.0), CAPTURE_BOX);
    }

    #[test]
    fn only_48k_mono_float_is_sent() {
        assert!(is_share_sound(48_000.0, true, 32, 1));
        assert!(!is_share_sound(44_100.0, true, 32, 1));
        assert!(!is_share_sound(48_000.0, false, 16, 1));
        assert!(!is_share_sound(48_000.0, true, 32, 2));
    }
}
