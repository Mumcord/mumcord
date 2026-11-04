//! The share's sound on Windows (FR-7.79): WASAPI process loopback (Windows 10 2004+). A window
//! share captures the process tree of the app behind the window (include mode); a whole-screen
//! share captures everything the computer plays except Mumcord's own process tree (exclude
//! mode), so viewers never hear their own voices (FR-7.77).
//!
//! Everything runs on the share's sound thread, which is not the real-time audio thread: the
//! loopback is opened there on the first frame, in that thread's own COM apartment, and each
//! frame waits on the loopback's event for at most the caller's timeout. When the default output
//! device changes, or the stream is lost, the loopback is opened anew; after repeated failures
//! the sound counts as failed and the video share goes on without it (FR-7.84).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use mumcord_audio::{Frame, SAMPLE_RATE};
use windows::Win32::Foundation::{CloseHandle, ERROR_TIMEOUT, HANDLE, PROPERTYKEY, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::{
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
    AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_LOOPBACK, AUDIOCLIENT_ACTIVATION_PARAMS,
    AUDIOCLIENT_ACTIVATION_PARAMS_0, AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
    AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS, ActivateAudioInterfaceAsync, DEVICE_STATE, EDataFlow,
    ERole, IActivateAudioInterfaceAsyncOperation, IActivateAudioInterfaceCompletionHandler,
    IActivateAudioInterfaceCompletionHandler_Impl, IAudioCaptureClient, IAudioClient,
    IMMDeviceEnumerator, IMMNotificationClient, IMMNotificationClient_Impl, MMDeviceEnumerator,
    PROCESS_LOOPBACK_MODE, PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE,
    PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE, VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
    WAVE_FORMAT_PCM, WAVEFORMATEX, eConsole, eRender,
};
use windows::Win32::System::Com::StructuredStorage::{
    PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0,
};
use windows::Win32::System::Com::{
    BLOB, CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
    IAgileObject, IAgileObject_Impl,
};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::Win32::System::Variant::VT_BLOB;
use windows::core::{HRESULT, IUnknown, Interface, PCWSTR, Ref, implement};

use super::super::capture::SoundSource;
use super::super::windows_rules::{MonoFrames, Reopen};

/// Stereo 16-bit PCM is what process loopback is documented with; the mixdown to mono happens
/// in [`MonoFrames`]. The rate is the share's, so Windows resamples, not Mumcord.
const CHANNELS: u16 = 2;
const BITS: u16 = 16;

/// The loopback's own buffer: generous, so a briefly stalled sound thread loses nothing.
const BUFFER_100NS: i64 = 1_000_000;

/// How long opening the loopback may take before it counts as a failure.
const ACTIVATE_TIMEOUT: Duration = Duration::from_secs(5);

/// Pause before opening the loopback again after a failure.
const RETRY_AFTER: Duration = Duration::from_secs(1);

/// Whose sound the loopback captures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopbackTarget {
    /// The process tree of the app behind a shared window.
    App(u32),
    /// Everything except this process tree: Mumcord's own, for a whole-screen share.
    AllExcept(u32),
}

impl LoopbackTarget {
    fn params(self) -> (u32, PROCESS_LOOPBACK_MODE) {
        match self {
            Self::App(pid) => (pid, PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE),
            Self::AllExcept(pid) => (pid, PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE),
        }
    }
}

/// The sound of `target`, opened on first use.
pub struct ProcessLoopback {
    target: LoopbackTarget,
    /// Set up on the sound thread with the first frame.
    thread: Option<SoundThread>,
    state: State,
    failures: Reopen,
    frames: MonoFrames,
}

/// What the loopback needs on the sound thread besides the stream itself.
struct SoundThread {
    /// Whether this thread's COM apartment was entered here and has to be left again.
    com: bool,
    /// `None` when changes of the default output cannot be followed; a lost stream is still
    /// reopened.
    output: Option<DefaultOutput>,
}

enum State {
    /// To be opened now.
    Closed,
    /// To be opened again once this time has come.
    Retry(Instant),
    Open(Loopback),
    /// Given up; the share stays silent.
    Failed,
}

struct Loopback {
    client: IAudioClient,
    capture: IAudioCaptureClient,
    event: HANDLE,
}

// SAFETY: before the first frame the source holds only plain data; once opened, it is only used
// and dropped on the sound thread that opened it (the source moves there before its first frame
// and stays). The audio client objects are free-threaded anyway.
unsafe impl Send for ProcessLoopback {}

impl ProcessLoopback {
    pub fn new(target: LoopbackTarget) -> Self {
        Self {
            target,
            thread: None,
            state: State::Closed,
            failures: Reopen::default(),
            frames: MonoFrames::default(),
        }
    }

    /// Enters COM and starts following the default output, once, on the sound thread.
    fn enter(&mut self) {
        if self.thread.is_some() {
            return;
        }
        // SAFETY: entering the multithreaded apartment on this thread; left again on drop.
        // S_FALSE (already entered) also has to be balanced; a different apartment is an error
        // and nothing to balance.
        let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
        let output = match DefaultOutput::watch() {
            Ok(output) => Some(output),
            Err(e) => {
                tracing::warn!(
                    code = e.code().0,
                    reason = "device_watch",
                    "screen share sound does not follow the default output"
                );
                None
            }
        };
        self.thread = Some(SoundThread { com, output });
    }

    /// Opens the loopback when it is due, or gives up after too many failures.
    fn open_when_due(&mut self) {
        let due = match self.state {
            State::Closed => true,
            State::Retry(at) => Instant::now() >= at,
            State::Open(_) | State::Failed => false,
        };
        if !due {
            return;
        }
        self.state = match open(self.target) {
            Ok(loopback) => State::Open(loopback),
            Err(e) => self.failure(e, "open_failed"),
        };
    }

    /// The state after a failure: retry later, or give up for good.
    fn failure(&mut self, e: windows::core::Error, reason: &'static str) -> State {
        if self.failures.failed() {
            tracing::warn!(code = e.code().0, reason, "screen share sound failed");
            State::Failed
        } else {
            tracing::info!(code = e.code().0, reason, "screen share sound reopens");
            State::Retry(Instant::now() + RETRY_AFTER)
        }
    }

    /// Reads every packet the loopback has ready into `frames`.
    fn drain(loopback: &Loopback, frames: &mut MonoFrames) -> windows::core::Result<()> {
        loop {
            // SAFETY: the capture client is open; the buffer is read before it is released.
            unsafe {
                if loopback.capture.GetNextPacketSize()? == 0 {
                    return Ok(());
                }
                let (mut data, mut count, mut flags) = (std::ptr::null_mut(), 0u32, 0u32);
                loopback
                    .capture
                    .GetBuffer(&mut data, &mut count, &mut flags, None, None)?;
                let samples = count as usize * usize::from(CHANNELS);
                if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 || data.is_null() {
                    frames.push_silence(count as usize);
                } else {
                    let pcm = std::slice::from_raw_parts(data as *const i16, samples);
                    frames.push_stereo_i16(pcm);
                }
                loopback.capture.ReleaseBuffer(count)?;
            }
        }
    }
}

impl SoundSource for ProcessLoopback {
    fn next_frame(&mut self, frame: &mut Frame, timeout: Duration) -> bool {
        if self.frames.pop(frame) {
            return true;
        }
        self.enter();
        let output_changed = self
            .thread
            .as_ref()
            .and_then(|t| t.output.as_ref())
            .is_some_and(DefaultOutput::take_changed);
        if output_changed && matches!(self.state, State::Open(_)) {
            tracing::info!(reason = "device_changed", "screen share sound reopens");
            self.state = State::Closed;
        }
        self.open_when_due();
        let State::Open(loopback) = &self.state else {
            std::thread::sleep(timeout);
            return false;
        };
        let wait_ms = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
        // SAFETY: the event handle stays open as long as the loopback.
        if unsafe { WaitForSingleObject(loopback.event, wait_ms) } != WAIT_OBJECT_0 {
            return false;
        }
        if let Err(e) = Self::drain(loopback, &mut self.frames) {
            // Typically AUDCLNT_E_DEVICE_INVALIDATED: the device went away under the stream.
            self.state = self.failure(e, "stream_lost");
            return false;
        }
        self.failures.worked();
        self.frames.pop(frame)
    }

    fn failed(&self) -> bool {
        matches!(self.state, State::Failed)
    }
}

impl Drop for Loopback {
    fn drop(&mut self) {
        // SAFETY: the client was started in `open`; the handle is ours. The interfaces are
        // released by their own drops, which run after this.
        unsafe {
            if let Err(e) = self.client.Stop() {
                tracing::debug!(code = e.code().0, "screen share sound did not stop cleanly");
            }
            if let Err(e) = CloseHandle(self.event) {
                tracing::debug!(code = e.code().0, "screen share sound event not closed");
            }
        }
    }
}

impl Drop for ProcessLoopback {
    fn drop(&mut self) {
        // The stream and the device watch hold COM objects: released before the apartment is
        // left.
        self.state = State::Closed;
        if let Some(SoundThread { com, output }) = self.thread.take() {
            drop(output);
            if com {
                // SAFETY: matches the CoInitializeEx in `enter` on this same thread; all COM
                // objects of the loopback are released above.
                unsafe { CoUninitialize() };
            }
        }
    }
}

/// Follows changes of the default output device; the loopback is then opened anew.
struct DefaultOutput {
    enumerator: IMMDeviceEnumerator,
    client: IMMNotificationClient,
    changed: Arc<AtomicBool>,
}

impl DefaultOutput {
    fn watch() -> windows::core::Result<Self> {
        let changed = Arc::new(AtomicBool::new(false));
        let client: IMMNotificationClient = OutputChanged(changed.clone()).into();
        // SAFETY: COM is entered on this thread; the client is unregistered on drop.
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            enumerator.RegisterEndpointNotificationCallback(&client)?;
            Ok(Self {
                enumerator,
                client,
                changed,
            })
        }
    }

    /// Whether the default output changed since the last call.
    fn take_changed(&self) -> bool {
        self.changed.swap(false, Ordering::Relaxed)
    }
}

impl Drop for DefaultOutput {
    fn drop(&mut self) {
        // SAFETY: registered in `watch` with this same client.
        let unregistered = unsafe {
            self.enumerator
                .UnregisterEndpointNotificationCallback(&self.client)
        };
        if let Err(e) = unregistered {
            tracing::debug!(code = e.code().0, "screen share device watch not removed");
        }
    }
}

/// Raises the flag when the default output for normal playback changes. Called on a system
/// thread: only an atomic store, nothing that allocates, locks or logs.
#[implement(IMMNotificationClient)]
struct OutputChanged(Arc<AtomicBool>);

impl IMMNotificationClient_Impl for OutputChanged_Impl {
    fn OnDeviceStateChanged(&self, _: &PCWSTR, _: DEVICE_STATE) -> windows::core::Result<()> {
        Ok(())
    }

    fn OnDeviceAdded(&self, _: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }

    fn OnDeviceRemoved(&self, _: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }

    fn OnDefaultDeviceChanged(
        &self,
        flow: EDataFlow,
        role: ERole,
        _: &PCWSTR,
    ) -> windows::core::Result<()> {
        if flow == eRender && role == eConsole {
            self.0.store(true, Ordering::Relaxed);
        }
        Ok(())
    }

    fn OnPropertyValueChanged(&self, _: &PCWSTR, _: &PROPERTYKEY) -> windows::core::Result<()> {
        Ok(())
    }
}

/// Signals when the asynchronous activation is done. Agile, as the activation requires: it is
/// called on a worker thread.
#[implement(IActivateAudioInterfaceCompletionHandler, IAgileObject)]
struct Activated(mpsc::Sender<()>);

impl IAgileObject_Impl for Activated_Impl {}

impl IActivateAudioInterfaceCompletionHandler_Impl for Activated_Impl {
    fn ActivateCompleted(
        &self,
        _: Ref<IActivateAudioInterfaceAsyncOperation>,
    ) -> windows::core::Result<()> {
        // The receiver is gone only if `open` timed out; nothing waits any more.
        let _ = self.0.send(());
        Ok(())
    }
}

fn open(target: LoopbackTarget) -> windows::core::Result<Loopback> {
    let (pid, mode) = target.params();
    let mut params = AUDIOCLIENT_ACTIVATION_PARAMS {
        ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
        Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
            ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                TargetProcessId: pid,
                ProcessLoopbackMode: mode,
            },
        },
    };
    // The parameters travel as a blob that points at `params`; nothing frees it (no
    // PropVariantClear), it only has to outlive the activation call.
    let blob = PROPVARIANT {
        Anonymous: PROPVARIANT_0 {
            Anonymous: std::mem::ManuallyDrop::new(PROPVARIANT_0_0 {
                vt: VT_BLOB,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: PROPVARIANT_0_0_0 {
                    blob: BLOB {
                        cbSize: size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32,
                        pBlobData: (&mut params as *mut AUDIOCLIENT_ACTIVATION_PARAMS).cast(),
                    },
                },
            }),
        },
    };
    let (tx, rx) = mpsc::channel();
    let handler: IActivateAudioInterfaceCompletionHandler = Activated(tx).into();
    // SAFETY: `blob` and `params` outlive the call, which copies them.
    let operation = unsafe {
        ActivateAudioInterfaceAsync(
            VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
            &IAudioClient::IID,
            Some(&blob),
            &handler,
        )?
    };
    if rx.recv_timeout(ACTIVATE_TIMEOUT).is_err() {
        return Err(windows::core::Error::new(
            ERROR_TIMEOUT.to_hresult(),
            "activating the process loopback timed out",
        ));
    }
    let mut result = HRESULT(0);
    let mut unknown: Option<IUnknown> = None;
    // SAFETY: the activation completed; valid out pointers.
    unsafe { operation.GetActivateResult(&mut result, &mut unknown)? };
    result.ok()?;
    let client: IAudioClient = unknown.ok_or_else(windows::core::Error::empty)?.cast()?;

    let block_align = CHANNELS * BITS / 8;
    let format = WAVEFORMATEX {
        wFormatTag: WAVE_FORMAT_PCM as u16,
        nChannels: CHANNELS,
        nSamplesPerSec: SAMPLE_RATE,
        nAvgBytesPerSec: SAMPLE_RATE * u32::from(block_align),
        nBlockAlign: block_align,
        wBitsPerSample: BITS,
        cbSize: 0,
    };
    // SAFETY: `format` is a complete PCM description; the event handle is closed by `Loopback`
    // or below on failure.
    unsafe {
        client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_LOOPBACK
                | AUDCLNT_STREAMFLAGS_EVENTCALLBACK
                | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
            BUFFER_100NS,
            0,
            &format,
            None,
        )?;
        let capture: IAudioCaptureClient = client.GetService()?;
        let event = CreateEventW(None, false, false, None)?;
        let started = client.SetEventHandle(event).and_then(|()| client.Start());
        if let Err(e) = started {
            let _ = CloseHandle(event);
            return Err(e);
        }
        Ok(Loopback {
            client,
            capture,
            event,
        })
    }
}
