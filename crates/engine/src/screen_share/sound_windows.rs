//! The shared app's sound on Windows (FR-2.42): WASAPI process loopback (Windows 10 2004+)
//! captures exactly the sound of one process and its children (browsers play from a child
//! process), never the rest of the system and so never Mumcord's voice output.
//!
//! Everything runs on the share's sound thread, which is not the real-time audio thread: the
//! loopback is opened there on the first frame, in that thread's own COM apartment, and each
//! frame waits on the loopback's event for at most the caller's timeout.

use std::sync::mpsc;
use std::time::Duration;

use mumcord_audio::{Frame, SAMPLE_RATE};
use windows::Win32::Foundation::{CloseHandle, ERROR_TIMEOUT, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::{
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
    AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_LOOPBACK, AUDIOCLIENT_ACTIVATION_PARAMS,
    AUDIOCLIENT_ACTIVATION_PARAMS_0, AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
    AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS, ActivateAudioInterfaceAsync,
    IActivateAudioInterfaceAsyncOperation, IActivateAudioInterfaceCompletionHandler,
    IActivateAudioInterfaceCompletionHandler_Impl, IAudioCaptureClient, IAudioClient,
    PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE, VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
    WAVE_FORMAT_PCM, WAVEFORMATEX,
};
use windows::Win32::System::Com::StructuredStorage::{
    PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0,
};
use windows::Win32::System::Com::{
    BLOB, COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize, IAgileObject, IAgileObject_Impl,
};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::Win32::System::Variant::VT_BLOB;
use windows::core::{HRESULT, IUnknown, Interface, Ref, implement};

use super::capture::SoundSource;
use super::windows_rules::MonoFrames;

/// Stereo 16-bit PCM is what process loopback is documented with; the mixdown to mono happens
/// in [`MonoFrames`]. The rate is the share's, so Windows resamples, not Mumcord.
const CHANNELS: u16 = 2;
const BITS: u16 = 16;

/// The loopback's own buffer: generous, so a briefly stalled sound thread loses nothing.
const BUFFER_100NS: i64 = 1_000_000;

/// How long opening the loopback may take before the share goes on without sound.
const ACTIVATE_TIMEOUT: Duration = Duration::from_secs(5);

/// The sound of one process tree, opened on first use.
pub struct ProcessLoopback {
    pid: u32,
    state: State,
    frames: MonoFrames,
}

enum State {
    Closed,
    Open(Loopback),
    /// Opening failed (logged once); the share stays silent.
    Failed,
}

struct Loopback {
    client: IAudioClient,
    capture: IAudioCaptureClient,
    event: HANDLE,
    /// Whether this thread's COM apartment was entered here and has to be left again.
    com: bool,
}

// SAFETY: before the loopback is opened the source holds only plain data; once opened, it is
// only used and dropped on the sound thread that opened it (the source moves there before its
// first frame and stays). The audio client objects are free-threaded anyway.
unsafe impl Send for ProcessLoopback {}

impl ProcessLoopback {
    pub fn new(pid: u32) -> Self {
        Self {
            pid,
            state: State::Closed,
            frames: MonoFrames::default(),
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
        if matches!(self.state, State::Closed) {
            self.state = match open(self.pid) {
                Ok(loopback) => State::Open(loopback),
                Err(e) => {
                    tracing::warn!(error = %e, "screen share sound could not be captured");
                    State::Failed
                }
            };
        }
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
            tracing::warn!(error = %e, "screen share sound stopped");
            self.state = State::Failed;
            return false;
        }
        self.frames.pop(frame)
    }
}

impl Drop for Loopback {
    fn drop(&mut self) {
        // SAFETY: the client was started in `open`; the handle is ours. The interfaces are
        // released by their own drops, which run after this, so the apartment is left last
        // (in `ProcessLoopback`'s drop below).
        unsafe {
            if let Err(e) = self.client.Stop() {
                tracing::debug!(error = %e, "screen share sound did not stop cleanly");
            }
            if let Err(e) = CloseHandle(self.event) {
                tracing::debug!(error = %e, "screen share sound event not closed");
            }
        }
    }
}

impl Drop for ProcessLoopback {
    fn drop(&mut self) {
        if let State::Open(loopback) = std::mem::replace(&mut self.state, State::Closed) {
            let com = loopback.com;
            drop(loopback);
            if com {
                // SAFETY: matches the CoInitializeEx in `open` on this same thread; all COM
                // objects of the loopback are released above.
                unsafe { CoUninitialize() };
            }
        }
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

fn open(pid: u32) -> windows::core::Result<Loopback> {
    // SAFETY: entering the multithreaded apartment on this thread; left again on drop. S_FALSE
    // (already entered) also has to be balanced; a different apartment is an error and nothing
    // to balance.
    let entered = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    let com = entered.is_ok();
    let opened = activate(pid);
    if opened.is_err() && com {
        // SAFETY: balances the call above; `activate` released its objects on failure.
        unsafe { CoUninitialize() };
    }
    opened.map(|(client, capture, event)| Loopback {
        client,
        capture,
        event,
        com,
    })
}

fn activate(pid: u32) -> windows::core::Result<(IAudioClient, IAudioCaptureClient, HANDLE)> {
    let mut params = AUDIOCLIENT_ACTIVATION_PARAMS {
        ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
        Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
            ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                TargetProcessId: pid,
                ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
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
        Ok((client, capture, event))
    }
}
