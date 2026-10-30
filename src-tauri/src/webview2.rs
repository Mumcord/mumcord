//! The Microsoft Edge WebView2 runtime that draws Mumcord's window on Windows. The installer
//! sets it up when it is missing; the portable exe cannot, and without it Tauri fails before
//! any window exists, so the user would see nothing at all. This check runs first and explains
//! the problem in a native message box instead (issue #181).

use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    IDYES, MB_ICONERROR, MB_YESNO, MessageBoxW, SW_SHOWNORMAL,
};

/// Microsoft's download of the Evergreen WebView2 runtime installer.
const DOWNLOAD_URL: &str = "https://go.microsoft.com/fwlink/p/?LinkId=2124703";

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Returns `false` after telling the user that WebView2 is missing; the caller then exits.
pub fn ensure_available() -> bool {
    match tauri::webview_version() {
        Ok(_) => true,
        Err(e) => {
            eprintln!("WebView2 runtime not found: {e}");
            let text = format!(
                "Mumcord needs the Microsoft Edge WebView2 Runtime, which is not installed on \
                 this PC.\n\nInstall it from Microsoft (free), \
                 then start Mumcord again:\n{DOWNLOAD_URL}\n\nOpen the download now?"
            );
            // SAFETY: the strings are NUL-terminated and outlive the calls; no owner window.
            unsafe {
                let answer = MessageBoxW(
                    std::ptr::null_mut(),
                    wide(&text).as_ptr(),
                    wide("Mumcord").as_ptr(),
                    MB_YESNO | MB_ICONERROR,
                );
                if answer == IDYES {
                    ShellExecuteW(
                        std::ptr::null_mut(),
                        wide("open").as_ptr(),
                        wide(DOWNLOAD_URL).as_ptr(),
                        std::ptr::null(),
                        std::ptr::null(),
                        SW_SHOWNORMAL,
                    );
                }
            }
            false
        }
    }
}
