// Prevents an additional console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // FreeBSD's i915 driver hangs the GPU ("GPU HANG: context reset") when WebKit asks for a
    // hardware GL context, which freezes the whole desktop, not just Mumcord. Software rendering
    // (llvmpipe) is slower but works on every driver; a working setup can opt back in by setting
    // LIBGL_ALWAYS_SOFTWARE=0 before starting the app. Must happen before GTK and WebKit start
    // and before any thread exists, hence here and not later.
    #[cfg(target_os = "freebsd")]
    if std::env::var_os("LIBGL_ALWAYS_SOFTWARE").is_none() {
        // SAFETY: no other thread is running yet.
        unsafe { std::env::set_var("LIBGL_ALWAYS_SOFTWARE", "1") };
    }
    mumcord_lib::run();
}
