//! Windows in-game overlay (FR-2.28): a separate transparent, click-through, always-on-top
//! layered window placed over the game in the foreground. Nothing touches the game: no DLL, no
//! injection, no files in its folder. The game window is only observed:
//! - `SetWinEventHook` with `WINEVENT_OUTOFCONTEXT` delivers foreground, minimize and move/resize
//!   events to this process's own thread; no code runs inside the game.
//! - The game's executable path is read with a `PROCESS_QUERY_LIMITED_INFORMATION` handle, the
//!   same read-only access Task Manager uses.
//!
//! One thread owns the overlay window, the event hooks and the message loop. It draws the panel
//! with the shared CPU renderer (`mumcord_overlay_ipc::render`, the same look as the Linux layer)
//! into the window with `UpdateLayeredWindow`; Windows (DWM) composites it, also on HDR desktops
//! (FR-2.29). Games in exclusive fullscreen own the screen, so the overlay hides there.

use std::cell::RefCell;
use std::sync::{Arc, mpsc};

use mumcord_engine::store::GameKey;
use mumcord_overlay_ipc::render::{self, Image};
use mumcord_overlay_ipc::wire::{Config, Scene};
use windows_sys::Win32::Foundation::{
    CloseHandle, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM,
};
use windows_sys::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, ClientToScreen,
    CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, ReleaseDC,
    SelectObject,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows_sys::Win32::System::Threading::{
    GetCurrentThreadId, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    QueryFullProcessImageNameW,
};
use windows_sys::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows_sys::Win32::UI::Shell::{QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, EVENT_OBJECT_LOCATIONCHANGE,
    EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_MINIMIZEEND, EVENT_SYSTEM_MINIMIZESTART, GetClientRect,
    GetForegroundWindow, GetMessageW, GetWindowThreadProcessId, HWND_TOPMOST, IsIconic, MSG,
    OBJID_WINDOW, PM_NOREMOVE, PeekMessageW, PostThreadMessageW, RegisterClassW, SW_HIDE,
    SWP_NOACTIVATE, SWP_SHOWWINDOW, SetWindowPos, ShowWindow, ULW_ALPHA, UpdateLayeredWindow,
    WINEVENT_OUTOFCONTEXT, WM_APP, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

use super::follow::{self, Foreground, Rect};
use super::{SeenGame, Shared};

/// Posted to the overlay thread: the scene changed (redraw).
const MSG_SCENE: u32 = WM_APP + 1;
/// Posted to the overlay thread: the settings changed (decide again which game is shown).
const MSG_SETTINGS: u32 = WM_APP + 2;

/// Handle to the overlay thread, kept for the app's lifetime.
pub struct Overlay {
    thread_id: u32,
}

impl Overlay {
    /// Redraw with the current scene.
    pub fn scene_changed(&self) {
        self.post(MSG_SCENE);
    }

    /// Decide again whether and how the foreground game shows the overlay.
    pub fn settings_changed(&self) {
        self.post(MSG_SETTINGS);
    }

    fn post(&self, message: u32) {
        // SAFETY: plain Win32 call; the thread id came from `GetCurrentThreadId`.
        if unsafe { PostThreadMessageW(self.thread_id, message, 0, 0) } == 0 {
            tracing::warn!("notifying the in-game overlay window failed");
        }
    }
}

/// Starts the overlay thread. Failures only mean no overlay; the app runs regardless.
pub fn start(shared: Arc<Shared>) -> Option<Overlay> {
    let (ready_tx, ready_rx) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("ingame-overlay-window".to_owned())
        .spawn(move || run(shared, &ready_tx));
    let result = match spawned {
        Ok(_) => ready_rx.recv().unwrap_or(Err("the overlay thread ended")),
        Err(e) => {
            tracing::warn!(error = %e, "starting the in-game overlay thread failed");
            return None;
        }
    };
    match result {
        Ok(thread_id) => Some(Overlay { thread_id }),
        Err(e) => {
            tracing::warn!(error = e, "starting the in-game overlay window failed");
            None
        }
    }
}

thread_local! {
    /// The WinEvent callback has no user data, so the thread's state lives here.
    static TRACKER: RefCell<Option<Tracker>> = const { RefCell::new(None) };
}

/// Creates the window and hooks, then pumps messages for the app's lifetime.
fn run(shared: Arc<Shared>, ready: &mpsc::Sender<Result<u32, &'static str>>) {
    // SAFETY: plain Win32 calls; `msg` is a valid MSG buffer and the callbacks match their types.
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        // Create this thread's message queue so `PostThreadMessageW` cannot get lost.
        PeekMessageW(&mut msg, std::ptr::null_mut(), WM_APP, WM_APP, PM_NOREMOVE);
        let Some(window) = create_window() else {
            let _ = ready.send(Err("CreateWindowExW failed"));
            return;
        };
        let foreground = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            std::ptr::null_mut(),
            Some(on_win_event),
            0,
            0,
            WINEVENT_OUTOFCONTEXT,
        );
        let minimize = SetWinEventHook(
            EVENT_SYSTEM_MINIMIZESTART,
            EVENT_SYSTEM_MINIMIZEEND,
            std::ptr::null_mut(),
            Some(on_win_event),
            0,
            0,
            WINEVENT_OUTOFCONTEXT,
        );
        if foreground.is_null() || minimize.is_null() {
            let _ = ready.send(Err("SetWinEventHook failed"));
            return;
        }
        TRACKER.with_borrow_mut(|t| *t = Some(Tracker::new(shared, window)));
        let _ = ready.send(Ok(GetCurrentThreadId()));
        with_tracker(|t| t.update(true));
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            if msg.hwnd.is_null() && (msg.message == MSG_SCENE || msg.message == MSG_SETTINGS) {
                let reclassify = msg.message == MSG_SETTINGS;
                with_tracker(|t| t.update(reclassify));
            } else {
                DispatchMessageW(&msg);
            }
        }
    }
}

/// Runs `f` on the tracker. A call that arrives while the tracker is busy (a Win32 call inside an
/// update can deliver events) is retried from the queue instead of nesting.
fn with_tracker(f: impl FnOnce(&mut Tracker)) {
    TRACKER.with(|cell| match cell.try_borrow_mut() {
        Ok(mut tracker) => {
            if let Some(tracker) = tracker.as_mut() {
                f(tracker);
            }
        }
        Err(_) => {
            // SAFETY: plain Win32 call posting to this very thread.
            unsafe { PostThreadMessageW(GetCurrentThreadId(), MSG_SETTINGS, 0, 0) };
        }
    });
}

unsafe extern "system" fn on_win_event(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    match event {
        EVENT_SYSTEM_FOREGROUND => with_tracker(|t| t.update(true)),
        EVENT_SYSTEM_MINIMIZESTART | EVENT_SYSTEM_MINIMIZEEND => with_tracker(|t| t.update(false)),
        EVENT_OBJECT_LOCATIONCHANGE if object == OBJID_WINDOW => with_tracker(|t| {
            if t.game.as_ref().is_some_and(|g| g.window == hwnd) {
                t.update(false);
            }
        }),
        _ => {}
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: forwards the message unchanged to the default handler.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

/// The overlay window: a popup that is layered (per-pixel alpha), transparent to clicks, never
/// activated, always on top and, as a tool window, absent from the taskbar and alt-tab.
fn create_window() -> Option<HWND> {
    let class = wide("MumcordInGameOverlay");
    // SAFETY: plain Win32 calls; `class` outlives both calls.
    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let wc = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            ..std::mem::zeroed()
        };
        if RegisterClassW(&wc) == 0 {
            return None;
        }
        let window = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            class.as_ptr(),
            class.as_ptr(),
            WS_POPUP,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        );
        (!window.is_null()).then_some(window)
    }
}

/// The foreground window last looked at and its game key (`None`: not a game).
struct Game {
    window: HWND,
    key: Option<GameKey>,
    /// Follows this window's moves and resizes; hooked for its process only.
    location_hook: HWINEVENTHOOK,
}

/// What is on screen now, to skip redundant redraws while the game moves.
struct Shown {
    config: Config,
    scene: Scene,
    scale_bits: u32,
    size: (i32, i32),
    position: (i32, i32),
}

struct Tracker {
    shared: Arc<Shared>,
    window: HWND,
    game: Option<Game>,
    shown: Option<Shown>,
}

impl Tracker {
    fn new(shared: Arc<Shared>, window: HWND) -> Self {
        Self {
            shared,
            window,
            game: None,
            shown: None,
        }
    }

    /// Brings the overlay in line with the foreground window. `reclassify` looks at which game it
    /// is again (foreground or settings changed); otherwise only position and scene are updated.
    fn update(&mut self, reclassify: bool) {
        // SAFETY: plain Win32 call.
        let foreground = unsafe { GetForegroundWindow() };
        let changed = self.game.as_ref().is_none_or(|g| g.window != foreground);
        if reclassify || changed {
            self.classify(foreground);
        }
        let key = self.game.as_ref().and_then(|g| g.key.clone());
        let Some(key) = key else {
            self.hide();
            return;
        };
        let config = self.shared.config_for(&key);
        let target = follow::target(observe(foreground), config.enabled);
        let Some(area) = target else {
            self.hide();
            return;
        };
        let scene = self.shared.scene_for(&key);
        let scale = follow::display_scale_for(area.height());
        self.show(config, scene, scale, area);
    }

    fn classify(&mut self, foreground: HWND) {
        if let Some(old) = self.game.take()
            && !old.location_hook.is_null()
        {
            // SAFETY: the hook came from `SetWinEventHook` and is unhooked once.
            unsafe { UnhookWinEvent(old.location_hook) };
        }
        let settings = self.shared.ingame_settings();
        // While the overlay is off, programs are not even looked at.
        if settings.mode == mumcord_engine::store::InGameOverlayMode::Off || foreground.is_null() {
            return;
        }
        let mut pid = 0;
        // SAFETY: plain Win32 call with a valid out pointer.
        unsafe { GetWindowThreadProcessId(foreground, &mut pid) };
        let Some(path) = exe_path(pid) else {
            return;
        };
        let key = follow::game_key(&path, &settings, running_steam_app());
        let mut location_hook = std::ptr::null_mut();
        if let Some(key) = &key {
            self.shared.record_seen(SeenGame {
                key: key.clone(),
                exe: mumcord_engine::now_gaming::detect::exe_name(&path).to_owned(),
            });
            // SAFETY: plain Win32 call; `on_win_event` matches `WINEVENTPROC`.
            location_hook = unsafe {
                SetWinEventHook(
                    EVENT_OBJECT_LOCATIONCHANGE,
                    EVENT_OBJECT_LOCATIONCHANGE,
                    std::ptr::null_mut(),
                    Some(on_win_event),
                    pid,
                    0,
                    WINEVENT_OUTOFCONTEXT,
                )
            };
            if location_hook.is_null() {
                tracing::warn!("following the game window failed");
            }
        }
        self.game = Some(Game {
            window: foreground,
            key,
            location_hook,
        });
    }

    fn show(&mut self, config: Config, scene: Scene, scale: f32, area: Rect) {
        let same_look = self.shown.as_ref().is_some_and(|s| {
            s.config == config && s.scene == scene && s.scale_bits == scale.to_bits()
        });
        if same_look && let Some(size) = self.shown.as_ref().map(|s| s.size) {
            let Some(position) = follow::panel_position(config.corner, size, area) else {
                self.hide();
                return;
            };
            if let Some(shown) = self.shown.as_mut()
                && shown.position != position
            {
                shown.position = position;
                place(self.window, position, size);
            }
            return;
        }
        let Some(font) = render::font() else {
            self.hide();
            return;
        };
        let Some(image) = render::render(&config, &scene, &font, scale) else {
            self.hide();
            return;
        };
        let (Ok(width), Ok(height)) = (i32::try_from(image.width), i32::try_from(image.height))
        else {
            self.hide();
            return;
        };
        let size = (width, height);
        let Some(position) = follow::panel_position(config.corner, size, area) else {
            self.hide();
            return;
        };
        if !paint(self.window, &image, position) {
            tracing::warn!("drawing the in-game overlay window failed");
            self.hide();
            return;
        }
        place(self.window, position, size);
        self.shown = Some(Shown {
            config,
            scene,
            scale_bits: scale.to_bits(),
            size,
            position,
        });
    }

    fn hide(&mut self) {
        if self.shown.take().is_some() {
            // SAFETY: plain Win32 call on the window this thread owns.
            unsafe { ShowWindow(self.window, SW_HIDE) };
        }
    }
}

/// The foreground window's state, or `None` when it has no readable client area.
fn observe(window: HWND) -> Option<Foreground> {
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    let mut origin = POINT { x: 0, y: 0 };
    // SAFETY: plain Win32 calls with valid out pointers.
    unsafe {
        if GetClientRect(window, &mut rect) == 0 || ClientToScreen(window, &mut origin) == 0 {
            return None;
        }
        let mut state = 0;
        let exclusive_fullscreen =
            SHQueryUserNotificationState(&mut state) >= 0 && state == QUNS_RUNNING_D3D_FULL_SCREEN;
        Some(Foreground {
            client: Rect {
                left: origin.x,
                top: origin.y,
                right: origin.x + rect.right,
                bottom: origin.y + rect.bottom,
            },
            minimized: IsIconic(window) != 0,
            exclusive_fullscreen,
        })
    }
}

/// Moves the window and shows it on top without activating it.
fn place(window: HWND, position: (i32, i32), size: (i32, i32)) {
    // SAFETY: plain Win32 call on the window this thread owns.
    let ok = unsafe {
        SetWindowPos(
            window,
            HWND_TOPMOST,
            position.0,
            position.1,
            size.0,
            size.1,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        )
    };
    if ok == 0 {
        tracing::warn!("placing the in-game overlay window failed");
    }
}

/// Puts `image` (premultiplied RGBA) into the layered window at `position`.
fn paint(window: HWND, image: &Image, position: (i32, i32)) -> bool {
    let (Ok(width), Ok(height)) = (i32::try_from(image.width), i32::try_from(image.height)) else {
        return false;
    };
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            // Negative: rows top to bottom, as the renderer writes them.
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..Default::default()
        },
        ..Default::default()
    };
    // SAFETY: plain GDI calls; every object created here is released before returning, and the
    // DIB has exactly `image.pixels.len()` bytes (32 bits per pixel, same size).
    unsafe {
        let screen = GetDC(std::ptr::null_mut());
        let memory = CreateCompatibleDC(screen);
        let mut bits = std::ptr::null_mut();
        let bitmap = CreateDIBSection(
            memory,
            &info,
            DIB_RGB_COLORS,
            &mut bits,
            std::ptr::null_mut(),
            0,
        );
        let ok = if bitmap.is_null() || bits.is_null() {
            false
        } else {
            let dib = std::slice::from_raw_parts_mut(bits.cast::<u8>(), image.pixels.len());
            // GDI wants premultiplied BGRA.
            let (dst, _) = dib.as_chunks_mut::<4>();
            let (src, _) = image.pixels.as_chunks::<4>();
            for (dst, &[r, g, b, a]) in dst.iter_mut().zip(src) {
                *dst = [b, g, r, a];
            }
            let previous = SelectObject(memory, bitmap);
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let ok = UpdateLayeredWindow(
                window,
                screen,
                &POINT {
                    x: position.0,
                    y: position.1,
                },
                &SIZE {
                    cx: width,
                    cy: height,
                },
                memory,
                &POINT { x: 0, y: 0 },
                0,
                &blend,
                ULW_ALPHA,
            ) != 0;
            SelectObject(memory, previous);
            ok
        };
        if !bitmap.is_null() {
            DeleteObject(bitmap);
        }
        DeleteDC(memory);
        ReleaseDC(std::ptr::null_mut(), screen);
        ok
    }
}

/// Full path of the program running as `pid`, read without touching the process.
fn exe_path(pid: u32) -> Option<String> {
    let mut buffer = [0u16; 1024];
    let mut len = buffer.len() as u32;
    // SAFETY: plain Win32 calls; the handle is closed once, the buffer length is passed along.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return None;
        }
        let ok =
            QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, buffer.as_mut_ptr(), &mut len);
        CloseHandle(process);
        if ok == 0 {
            return None;
        }
    }
    Some(String::from_utf16_lossy(buffer.get(..len as usize)?))
}

/// The game Steam reports as running (`HKCU\Software\Valve\Steam\RunningAppID`), if any.
fn running_steam_app() -> Option<u32> {
    let key = wide(r"Software\Valve\Steam");
    let value = wide("RunningAppID");
    let mut id: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY: plain Win32 call; `id` is a DWORD buffer of `size` bytes.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&raw mut id).cast(),
            &mut size,
        )
    };
    (status == 0 && id > 0).then_some(id)
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}
