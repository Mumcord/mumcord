//! Window or screen capture on Windows (FR-2.42 – FR-2.46).
//!
//! The system picker (Windows.Graphics.Capture) lets the user choose a window or a screen. The
//! frames land in a free-threaded frame pool as GPU textures; the encode thread copies the newest
//! one into a CPU-readable texture, then scales and converts it like the Linux backends. When the
//! picked item has an app behind it and Windows can isolate that app's sound, the capture comes
//! with a [`ProcessLoopback`] for it.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

use mumcord_screenshare::I420Frame;
use windows::Foundation::TypedEventHandler;
use windows::Graphics::Capture::{
    Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCapturePicker, GraphicsCaptureSession,
};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Graphics::SizeInt32;
use windows::Wdk::System::SystemServices::RtlGetVersion;
use windows::Win32::Foundation::{HMODULE, HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ,
    D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::System::SystemInformation::OSVERSIONINFOW;
use windows::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};
use windows::Win32::System::WinRT::Direct3D11::{
    CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
};
use windows::Win32::System::WinRT::{RO_INIT_SINGLETHREADED, RoInitialize, RoUninitialize};
use windows::Win32::UI::Shell::IInitializeWithWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, EnumWindows, GetMessageW, GetWindowTextW, GetWindowThreadProcessId,
    IsWindowVisible, MSG, PM_NOREMOVE, PeekMessageW, PostThreadMessageW, TranslateMessage, WM_QUIT,
};
use windows::core::{BOOL, Interface};

use super::capture::{CaptureSource, SoundSource};
use super::convert::{bgrx_to_i420, pack_rows};
use super::sound_windows::ProcessLoopback;
use super::windows_rules::{TopWindow, capture_supported, sound_process, sound_supported};

/// Why no capture was started.
#[derive(Debug, thiserror::Error)]
pub enum WindowsCaptureError {
    /// The user closed the picker without choosing.
    #[error("the picker was closed")]
    Cancelled,
    #[error("screen sharing needs Windows 10 version 1903 or newer")]
    Unsupported,
    #[error("Mumcord's window is not known yet, so the picker cannot be shown")]
    NoOwner,
    #[error("the picker could not run: {0}")]
    Picker(String),
    #[error("screen capture failed: {0}")]
    Failed(#[from] windows::core::Error),
}

/// The app window the picker belongs to; the shell sets it once its main window exists.
static PICKER_OWNER: AtomicIsize = AtomicIsize::new(0);

/// Tells the capture which window owns the system picker (its raw `HWND`).
pub fn set_picker_owner(hwnd: isize) {
    PICKER_OWNER.store(hwnd, Ordering::Relaxed);
}

/// The Windows build number (19041 is Windows 10 2004), 0 when it cannot be read.
pub(crate) fn windows_build() -> u32 {
    let mut info = OSVERSIONINFOW {
        dwOSVersionInfoSize: size_of::<OSVERSIONINFOW>() as u32,
        ..Default::default()
    };
    // RtlGetVersion, unlike GetVersionEx, is not capped at Windows 8 for unmanifested programs.
    // SAFETY: `info` is a valid OSVERSIONINFOW with its size set.
    let status = unsafe { RtlGetVersion(&mut info) };
    if status.is_ok() {
        info.dwBuildNumber
    } else {
        0
    }
}

/// Whether this Windows can send the shared app's sound (FR-2.42).
pub(crate) fn sound_available() -> bool {
    sound_supported(windows_build())
}

/// Number of frames the pool holds: one being read, one being written.
const POOL_FRAMES: i32 = 2;
const PIXEL_FORMAT: DirectXPixelFormat = DirectXPixelFormat::B8G8R8A8UIntNormalized;

/// A running capture of the item the user picked.
pub struct WindowsCapture {
    item: GraphicsCaptureItem,
    closed_token: i64,
    session: GraphicsCaptureSession,
    pool: Direct3D11CaptureFramePool,
    pool_size: SizeInt32,
    winrt_device: IDirect3DDevice,
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    /// CPU-readable copy of the frame texture, with its size.
    staging: Option<(ID3D11Texture2D, u32, u32)>,
    /// The newest picture as tight BGRX rows. Windows only sends frames when the content
    /// changes, so this is sent again while nothing moves (viewers who join need a picture).
    picture: Vec<u8>,
    picture_size: (u32, u32),
    ended: Arc<AtomicBool>,
    sound: Option<Box<dyn SoundSource>>,
}

// SAFETY: the WinRT capture objects are agile. The D3D11 device is free-threaded; its immediate
// context is not, but after `open` only the encode thread uses it, one call at a time (`frame`
// takes `&mut self`).
unsafe impl Send for WindowsCapture {}

impl WindowsCapture {
    /// Shows the system picker and starts capturing what the user chose.
    pub async fn open() -> Result<Self, WindowsCaptureError> {
        let build = windows_build();
        if !capture_supported(build) || !GraphicsCaptureSession::IsSupported()? {
            return Err(WindowsCaptureError::Unsupported);
        }
        let owner = PICKER_OWNER.load(Ordering::Relaxed);
        if owner == 0 {
            return Err(WindowsCaptureError::NoOwner);
        }
        let (tx, rx) = tokio::sync::oneshot::channel();
        std::thread::Builder::new()
            .name("screen-picker".into())
            .spawn(move || {
                let _ = tx.send(pick(owner));
            })
            .map_err(|e| WindowsCaptureError::Picker(e.to_string()))?;
        let item = rx
            .await
            .map_err(|_| WindowsCaptureError::Picker("the picker thread ended".into()))??;
        let mut capture = Self::start(item)?;
        if sound_supported(build) {
            capture.sound = capture.sound_source();
        }
        Ok(capture)
    }

    fn start(item: GraphicsCaptureItem) -> windows::core::Result<Self> {
        let (device, context) = create_device()?;
        let dxgi: IDXGIDevice = device.cast()?;
        // SAFETY: `dxgi` is a live DXGI device.
        let winrt_device: IDirect3DDevice =
            unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi)? }.cast()?;
        let pool_size = item.Size()?;
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &winrt_device,
            PIXEL_FORMAT,
            POOL_FRAMES,
            pool_size,
        )?;
        let session = pool.CreateCaptureSession(&item)?;
        let ended = Arc::new(AtomicBool::new(false));
        let flag = ended.clone();
        // Closed: the window was closed or the screen unplugged; the share stops (FR-2.45).
        let closed_token = item.Closed(&TypedEventHandler::new(move |_, _| {
            flag.store(true, Ordering::Relaxed);
            Ok(())
        }))?;
        session.StartCapture()?;
        Ok(Self {
            item,
            closed_token,
            session,
            pool,
            pool_size,
            winrt_device,
            device,
            context,
            staging: None,
            picture: Vec::new(),
            picture_size: (0, 0),
            ended,
            sound: None,
        })
    }

    /// The picked app's sound, when the picked item is one app's window.
    fn sound_source(&self) -> Option<Box<dyn SoundSource>> {
        let name = match self.item.DisplayName() {
            Ok(name) => name.to_string_lossy(),
            Err(e) => {
                tracing::warn!(error = %e, "screen share: the picked item has no name; no sound");
                return None;
            }
        };
        // SAFETY: no preconditions.
        let own = unsafe { GetCurrentProcessId() };
        match sound_process(&top_windows(), &name, own) {
            Some(pid) => Some(Box::new(ProcessLoopback::new(pid))),
            None => {
                tracing::info!("screen share: no single app behind the picked item; no sound");
                None
            }
        }
    }

    /// Copies the newest frame, if one came, into `picture`.
    fn take_newest(&mut self) -> windows::core::Result<()> {
        let mut newest = None;
        // TryGetNextFrame gives an error (a null frame) once the pool is empty.
        while let Ok(frame) = self.pool.TryGetNextFrame() {
            newest = Some(frame);
        }
        let Some(frame) = newest else {
            return Ok(());
        };
        let content = frame.ContentSize()?;
        let access: IDirect3DDxgiInterfaceAccess = frame.Surface()?.cast()?;
        // SAFETY: the surface of a capture frame is a D3D11 texture.
        let texture: ID3D11Texture2D = unsafe { access.GetInterface()? };
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        // SAFETY: `desc` is a valid out pointer.
        unsafe { texture.GetDesc(&mut desc) };
        let staging = self.staging_for(&desc)?;
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        // SAFETY: both textures belong to `self.device` and have the same size and format; the
        // mapping is read only within this block and unmapped before the texture can change.
        unsafe {
            self.context.CopyResource(&staging, &texture);
            self.context
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
            // A resized window fills only part of the texture until the pool is recreated.
            let width = (content.Width.max(0) as u32).min(desc.Width);
            let height = (content.Height.max(0) as u32).min(desc.Height);
            let stride = mapped.RowPitch as usize;
            let bytes = std::slice::from_raw_parts(
                mapped.pData as *const u8,
                stride * desc.Height as usize,
            );
            if pack_rows(bytes, stride, width, height, &mut self.picture) {
                self.picture_size = (width, height);
            }
            self.context.Unmap(&staging, 0);
        }
        drop(frame);
        if content.Width != self.pool_size.Width || content.Height != self.pool_size.Height {
            self.pool_size = content;
            self.pool
                .Recreate(&self.winrt_device, PIXEL_FORMAT, POOL_FRAMES, content)?;
        }
        Ok(())
    }

    fn staging_for(
        &mut self,
        src: &D3D11_TEXTURE2D_DESC,
    ) -> windows::core::Result<ID3D11Texture2D> {
        if let Some((texture, w, h)) = &self.staging
            && (*w, *h) == (src.Width, src.Height)
        {
            return Ok(texture.clone());
        }
        let desc = D3D11_TEXTURE2D_DESC {
            Width: src.Width,
            Height: src.Height,
            MipLevels: 1,
            ArraySize: 1,
            Format: src.Format,
            SampleDesc: src.SampleDesc,
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
        };
        let mut texture = None;
        // SAFETY: `desc` describes a valid staging texture; `texture` is a valid out pointer.
        unsafe {
            self.device
                .CreateTexture2D(&desc, None, Some(&mut texture))?
        };
        let texture = texture.ok_or_else(windows::core::Error::empty)?;
        self.staging = Some((texture.clone(), src.Width, src.Height));
        Ok(texture)
    }
}

impl CaptureSource for WindowsCapture {
    fn frame(&mut self, max_width: u32, max_height: u32) -> Option<I420Frame> {
        if let Err(e) = self.take_newest() {
            tracing::warn!(error = %e, "screen capture frame failed");
        }
        let (width, height) = self.picture_size;
        bgrx_to_i420(&self.picture, width, height, max_width, max_height)
    }

    fn ended(&self) -> bool {
        self.ended.load(Ordering::Relaxed)
    }

    fn take_sound(&mut self) -> Option<Box<dyn SoundSource>> {
        self.sound.take()
    }
}

impl Drop for WindowsCapture {
    fn drop(&mut self) {
        // Closing the session removes the yellow capture border and stops the frames.
        let closed = self
            .item
            .RemoveClosed(self.closed_token)
            .and_then(|()| self.session.Close())
            .and_then(|()| self.pool.Close());
        if let Err(e) = closed {
            tracing::debug!(error = %e, "screen capture did not close cleanly");
        }
    }
}

fn create_device() -> windows::core::Result<(ID3D11Device, ID3D11DeviceContext)> {
    // WARP (software) when there is no usable GPU, e.g. in a virtual machine.
    match device_of(D3D_DRIVER_TYPE_HARDWARE) {
        Ok(pair) => Ok(pair),
        Err(e) => {
            tracing::info!(error = %e, "no hardware Direct3D device; using WARP");
            device_of(D3D_DRIVER_TYPE_WARP)
        }
    }
}

fn device_of(
    driver: D3D_DRIVER_TYPE,
) -> windows::core::Result<(ID3D11Device, ID3D11DeviceContext)> {
    let (mut device, mut context) = (None, None);
    // SAFETY: valid out pointers; BGRA support is what the capture interop needs.
    unsafe {
        D3D11CreateDevice(
            None,
            driver,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )?;
    }
    match (device, context) {
        (Some(device), Some(context)) => Ok((device, context)),
        _ => Err(windows::core::Error::empty()),
    }
}

/// Shows the picker on its own single-threaded apartment and waits for the choice. The picker
/// completes through the message queue, so the thread pumps messages until then.
fn pick(owner: isize) -> Result<GraphicsCaptureItem, WindowsCaptureError> {
    // SAFETY: this thread is new and uninitialised; it is uninitialised again before it ends.
    unsafe { RoInitialize(RO_INIT_SINGLETHREADED)? };
    let picked = pick_in_apartment(owner);
    // SAFETY: matches the RoInitialize above; every COM object of this thread is dropped.
    unsafe { RoUninitialize() };
    picked
}

fn pick_in_apartment(owner: isize) -> Result<GraphicsCaptureItem, WindowsCaptureError> {
    let picker = GraphicsCapturePicker::new()?;
    // Desktop apps have no CoreWindow; the picker needs the window it belongs to.
    // SAFETY: `owner` is the app's main window handle, set by the shell.
    unsafe {
        picker
            .cast::<IInitializeWithWindow>()?
            .Initialize(HWND(owner as *mut core::ffi::c_void))?;
    }
    let operation = picker.PickSingleItemAsync()?;
    let mut msg = MSG::default();
    // SAFETY: valid pointers; the thread ids are this thread's own.
    let thread = unsafe {
        // Creates this thread's message queue, so the quit message below cannot be lost.
        let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
        GetCurrentThreadId()
    };
    let (tx, rx) = std::sync::mpsc::channel();
    operation.when(move |result| {
        let _ = tx.send(result);
        // SAFETY: posting to a thread id is always memory safe.
        if let Err(e) = unsafe { PostThreadMessageW(thread, WM_QUIT, WPARAM(0), LPARAM(0)) } {
            tracing::warn!(error = %e, "screen picker thread could not be woken");
        }
    })?;
    // SAFETY: `msg` is a valid MSG for this thread's queue.
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    match rx.recv() {
        Ok(Ok(item)) => Ok(item),
        // Closing the picker completes with no item, which arrives as an empty error.
        Ok(Err(e)) if e.code().is_ok() => Err(WindowsCaptureError::Cancelled),
        Ok(Err(e)) => Err(e.into()),
        Err(_) => Err(WindowsCaptureError::Cancelled),
    }
}

/// The visible, not cloaked (hidden by the shell, e.g. on another virtual desktop) top-level
/// windows with a title.
fn top_windows() -> Vec<TopWindow> {
    unsafe extern "system" fn collect(hwnd: HWND, list: LPARAM) -> BOOL {
        // SAFETY: `list` is the Vec passed to EnumWindows below, alive for the whole call.
        let windows = unsafe { &mut *(list.0 as *mut Vec<TopWindow>) };
        // SAFETY: `hwnd` comes from EnumWindows; all out pointers are valid.
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() {
                return true.into();
            }
            let mut cloaked = 0u32;
            let cloak = DwmGetWindowAttribute(
                hwnd,
                DWMWA_CLOAKED,
                (&mut cloaked as *mut u32).cast(),
                size_of::<u32>() as u32,
            );
            if cloak.is_ok() && cloaked != 0 {
                return true.into();
            }
            let mut title = [0u16; 512];
            let len = GetWindowTextW(hwnd, &mut title);
            if len <= 0 {
                return true.into();
            }
            let mut pid = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            windows.push(TopWindow {
                title: String::from_utf16_lossy(&title[..len as usize]),
                pid,
            });
        }
        true.into()
    }

    let mut windows: Vec<TopWindow> = Vec::new();
    // SAFETY: `collect` only touches `windows`, which outlives the call.
    let listed = unsafe {
        EnumWindows(
            Some(collect),
            LPARAM(&mut windows as *mut Vec<TopWindow> as isize),
        )
    };
    if let Err(e) = listed {
        tracing::warn!(error = %e, "screen share: windows could not be listed");
    }
    windows
}
