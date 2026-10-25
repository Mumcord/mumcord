//! End-to-end check of the OpenGL preload path without a window.
//!
//! Renders blue frames into an EGL pbuffer (surfaceless platform), calls `eglSwapBuffers` (which
//! the preloaded library intercepts) and reads the pixels back. Run it with the library in
//! `LD_PRELOAD` while `headless_check serve` plays Mumcord with the same `MUMCORD_OVERLAY_SOCKET`.
//! Exit code 0: the overlay was drawn.

#[cfg(target_os = "linux")]
fn main() {
    match linux::run() {
        Ok(()) => println!("OK: the overlay was drawn into the GL frame"),
        Err(e) => {
            eprintln!("FAILED: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("Linux only");
}

#[cfg(target_os = "linux")]
mod linux {
    use std::ffi::{c_int, c_uint, c_void};
    use std::time::{Duration, Instant};

    const WIDTH: c_int = 640;
    const HEIGHT: c_int = 360;
    const EGL_PLATFORM_SURFACELESS_MESA: c_uint = 0x31DD;
    const EGL_NONE: c_int = 0x3038;

    // Linked directly (not through dlsym) so `LD_PRELOAD` interposes `eglSwapBuffers`.
    #[link(name = "EGL")]
    unsafe extern "C" {
        fn eglGetPlatformDisplay(
            platform: c_uint,
            native: *mut c_void,
            attribs: *const isize,
        ) -> *mut c_void;
        fn eglInitialize(display: *mut c_void, major: *mut c_int, minor: *mut c_int) -> c_uint;
        fn eglBindAPI(api: c_uint) -> c_uint;
        fn eglChooseConfig(
            d: *mut c_void,
            attribs: *const c_int,
            configs: *mut *mut c_void,
            size: c_int,
            n: *mut c_int,
        ) -> c_uint;
        fn eglCreateContext(
            d: *mut c_void,
            config: *mut c_void,
            share: *mut c_void,
            attribs: *const c_int,
        ) -> *mut c_void;
        fn eglCreatePbufferSurface(
            d: *mut c_void,
            config: *mut c_void,
            attribs: *const c_int,
        ) -> *mut c_void;
        fn eglMakeCurrent(
            d: *mut c_void,
            draw: *mut c_void,
            read: *mut c_void,
            ctx: *mut c_void,
        ) -> c_uint;
        fn eglSwapBuffers(d: *mut c_void, surface: *mut c_void) -> c_uint;
        fn eglGetProcAddress(name: *const u8) -> *const c_void;
    }

    unsafe fn gl<T>(name: &str) -> Result<T, String> {
        let c = format!("{name}\0");
        let p = unsafe { eglGetProcAddress(c.as_ptr()) };
        if p.is_null() {
            return Err(format!("{name} missing"));
        }
        Ok(unsafe { std::mem::transmute_copy::<*const c_void, T>(&p) })
    }

    pub fn run() -> Result<(), String> {
        // Safety: plain EGL/GL calls on this thread's current context.
        unsafe {
            let display = eglGetPlatformDisplay(
                EGL_PLATFORM_SURFACELESS_MESA,
                std::ptr::null_mut(),
                std::ptr::null(),
            );
            if display.is_null()
                || eglInitialize(display, std::ptr::null_mut(), std::ptr::null_mut()) == 0
            {
                return Err("no EGL display".into());
            }
            // EGL_OPENGL_API
            eglBindAPI(0x30A2);
            let attribs = [
                0x3033, 0x1, // EGL_SURFACE_TYPE: PBUFFER
                0x3040, 0x8, // EGL_RENDERABLE_TYPE: OPENGL
                0x3024, 8, 0x3023, 8, 0x3022, 8, 0x3021, 8, // RGBA 8
                EGL_NONE,
            ];
            let mut config = std::ptr::null_mut();
            let mut n = 0;
            if eglChooseConfig(display, attribs.as_ptr(), &mut config, 1, &mut n) == 0 || n == 0 {
                return Err("no EGL config".into());
            }
            let context =
                eglCreateContext(display, config, std::ptr::null_mut(), [EGL_NONE].as_ptr());
            let surface = eglCreatePbufferSurface(
                display,
                config,
                [0x3057, WIDTH, 0x3056, HEIGHT, EGL_NONE].as_ptr(),
            );
            if context.is_null()
                || surface.is_null()
                || eglMakeCurrent(display, surface, surface, context) == 0
            {
                return Err("no EGL context".into());
            }
            let clear_color: unsafe extern "C" fn(f32, f32, f32, f32) = gl("glClearColor")?;
            let clear: unsafe extern "C" fn(c_uint) = gl("glClear")?;
            let finish: unsafe extern "C" fn() = gl("glFinish")?;
            let read: unsafe extern "C" fn(
                c_int,
                c_int,
                c_int,
                c_int,
                c_uint,
                c_uint,
                *mut c_void,
            ) = gl("glReadPixels")?;
            let started = Instant::now();
            while started.elapsed() < Duration::from_secs(2) {
                clear_color(0.0, 0.0, 1.0, 1.0);
                clear(0x4000);
                eglSwapBuffers(display, surface);
                std::thread::sleep(Duration::from_millis(16));
            }
            finish();
            let at = |x: c_int, y_top: c_int| {
                let mut px = [0u8; 4];
                // GL_RGBA, GL_UNSIGNED_BYTE; GL rows start at the bottom.
                read(
                    x,
                    HEIGHT - 1 - y_top,
                    1,
                    1,
                    0x1908,
                    0x1401,
                    px.as_mut_ptr().cast(),
                );
                (px[0], px[1], px[2])
            };
            let dot = at(16 + 13, 16 + 21);
            let far = at(WIDTH - 20, HEIGHT - 20);
            println!("pixel at the talking dot: rgb{dot:?}, far away: rgb{far:?}");
            if far != (0, 0, 255) {
                return Err("the frame was not cleared to blue".into());
            }
            if !(dot.1 > 150 && dot.2 < 160 && dot.0 < 120) {
                return Err("no overlay at the expected place (is the library preloaded?)".into());
            }
            Ok(())
        }
    }
}
