//! Windows: mouse buttons via a low-level mouse hook (`WH_MOUSE_LL`) (FR-1.21, QA.4).
//!
//! The hook only observes: every event is passed on with `CallNextHookEx`. Events are never
//! logged.

use std::sync::{Mutex, MutexGuard, PoisonError, mpsc};

use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, HC_ACTION, MSG, MSLLHOOKSTRUCT, PM_NOREMOVE, PeekMessageW,
    PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx, WH_MOUSE_LL, WM_MBUTTONDOWN,
    WM_MBUTTONUP, WM_QUIT, WM_USER, WM_XBUTTONDOWN, WM_XBUTTONUP, XBUTTON1, XBUTTON2,
};

use super::mouse::{MouseButton, OnChange, StartError};

struct Target {
    button: MouseButton,
    on_change: OnChange,
}

/// The hook procedure has no user data, so the wanted button lives here.
static TARGET: Mutex<Option<Target>> = Mutex::new(None);

fn target() -> MutexGuard<'static, Option<Target>> {
    TARGET.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Listens while alive; dropping it ends the hook thread.
pub struct Listener {
    thread_id: u32,
}

impl Drop for Listener {
    fn drop(&mut self) {
        *target() = None;
        // SAFETY: plain Win32 call; the thread id came from `GetCurrentThreadId`.
        unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0) };
    }
}

pub fn start(button: MouseButton, on_change: OnChange) -> Result<Listener, StartError> {
    *target() = Some(Target { button, on_change });
    let (ready_tx, ready_rx) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("ptt-mouse-hook".to_owned())
        .spawn(move || run_hook(&ready_tx));
    let result = match spawned {
        Ok(_) => ready_rx
            .recv()
            .unwrap_or_else(|_| Err("the hook thread ended".to_owned())),
        Err(err) => Err(err.to_string()),
    };
    match result {
        Ok(thread_id) => Ok(Listener { thread_id }),
        Err(err) => {
            *target() = None;
            Err(StartError::Failed(format!("mouse hook: {err}")))
        }
    }
}

/// Installs the hook and pumps messages until `WM_QUIT`.
fn run_hook(ready: &mpsc::Sender<Result<u32, String>>) {
    // SAFETY: plain Win32 calls; `hook_proc` matches `HOOKPROC`, `msg` is a valid MSG buffer.
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        // Create this thread's message queue so `PostThreadMessageW` cannot get lost.
        PeekMessageW(
            &mut msg,
            std::ptr::null_mut(),
            WM_USER,
            WM_USER,
            PM_NOREMOVE,
        );
        let hook = SetWindowsHookExW(
            WH_MOUSE_LL,
            Some(hook_proc),
            GetModuleHandleW(std::ptr::null()),
            0,
        );
        if hook.is_null() {
            let _ = ready.send(Err("SetWindowsHookExW failed".to_owned()));
            return;
        }
        let _ = ready.send(Ok(GetCurrentThreadId()));
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {}
        UnhookWindowsHookEx(hook);
    }
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && lparam != 0 {
        // SAFETY: for HC_ACTION, `lparam` points to an MSLLHOOKSTRUCT.
        let info = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
        if let Some((button, pressed)) = decode(wparam as u32, info.mouseData) {
            let on_change = target()
                .as_ref()
                .filter(|t| t.button == button)
                .map(|t| t.on_change.clone());
            if let Some(on_change) = on_change {
                on_change(pressed);
            }
        }
    }
    // SAFETY: passes the event on unchanged, as required for low-level hooks.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

/// Maps a mouse message to a button and its new state.
fn decode(message: u32, mouse_data: u32) -> Option<(MouseButton, bool)> {
    match message {
        WM_MBUTTONDOWN => Some((MouseButton::Middle, true)),
        WM_MBUTTONUP => Some((MouseButton::Middle, false)),
        WM_XBUTTONDOWN | WM_XBUTTONUP => {
            let button = match (mouse_data >> 16) as u16 {
                XBUTTON1 => MouseButton::Back,
                XBUTTON2 => MouseButton::Forward,
                _ => return None,
            };
            Some((button, message == WM_XBUTTONDOWN))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_mouse_messages() {
        assert_eq!(decode(WM_MBUTTONDOWN, 0), Some((MouseButton::Middle, true)));
        assert_eq!(decode(WM_MBUTTONUP, 0), Some((MouseButton::Middle, false)));
        assert_eq!(
            decode(WM_XBUTTONDOWN, u32::from(XBUTTON1) << 16),
            Some((MouseButton::Back, true))
        );
        assert_eq!(
            decode(WM_XBUTTONUP, u32::from(XBUTTON2) << 16),
            Some((MouseButton::Forward, false))
        );
        assert_eq!(decode(WM_XBUTTONDOWN, 3 << 16), None);
        assert_eq!(decode(WM_USER, 0), None);
    }
}
