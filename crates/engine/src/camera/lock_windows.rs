//! Screen-lock events on Windows (FR-8.21): a message-only window registered for the session's
//! WTS notifications receives `WM_WTSSESSION_CHANGE` with `WTS_SESSION_LOCK`.

use std::sync::OnceLock;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::RemoteDesktop::{
    NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, HWND_MESSAGE, MSG,
    RegisterClassW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_WTSSESSION_CHANGE, WNDCLASSW,
    WTS_SESSION_LOCK,
};
use windows::core::w;

/// The callback of the one listener in this process.
static ON_LOCK: OnceLock<fn()> = OnceLock::new();

/// Starts the listener thread, which calls `on_lock` for each lock of this session.
pub(super) fn watch(on_lock: fn()) {
    if ON_LOCK.set(on_lock).is_err() {
        return;
    }
    let started = std::thread::Builder::new()
        .name("screen-lock".into())
        .spawn(|| {
            if let Err(e) = run() {
                tracing::info!(error = %e, "screen lock can't be followed (WTS)");
            }
        });
    if let Err(e) = started {
        tracing::warn!(error = %e, "screen lock thread not started");
    }
}

fn run() -> windows::core::Result<()> {
    let class = w!("MumcordScreenLock");
    // SAFETY: plain Win32 calls on this thread; the window class and the message-only window
    // live as long as the thread's message loop, which runs until the process ends.
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let wc = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance.into(),
            lpszClassName: class,
            ..Default::default()
        };
        if RegisterClassW(&wc) == 0 {
            return Err(windows::core::Error::from_thread());
        }
        let window = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            w!(""),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(instance.into()),
            None,
        )?;
        WTSRegisterSessionNotification(window, NOTIFY_FOR_THIS_SESSION)?;
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}

extern "system" fn window_proc(window: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_WTSSESSION_CHANGE
        && u32::try_from(wparam.0).is_ok_and(|event| event == WTS_SESSION_LOCK)
        && let Some(on_lock) = ON_LOCK.get()
    {
        on_lock();
    }
    // SAFETY: forwards the message this window received unchanged.
    unsafe { DefWindowProcW(window, msg, wparam, lparam) }
}
