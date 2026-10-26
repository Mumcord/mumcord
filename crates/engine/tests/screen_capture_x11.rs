//! X11 whole-screen capture against a private Xvfb; skipped when Xvfb is not installed.
#![cfg(target_os = "linux")]
#![allow(clippy::unwrap_used)]

use std::process::{Child, Command, Stdio};
use std::time::Duration;

use mumcord_engine::screen_share::CaptureSource;
use mumcord_engine::screen_share::capture_x11::X11Capture;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt, CreateWindowAux, WindowClass};

struct Xvfb(Child);

impl Drop for Xvfb {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn start_xvfb(display: &str) -> Option<Xvfb> {
    let child = Command::new("Xvfb")
        .args([display, "-screen", "0", "640x480x24", "-nolisten", "tcp"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let guard = Xvfb(child);
    for _ in 0..50 {
        if x11rb::rust_connection::RustConnection::connect(Some(display)).is_ok() {
            return Some(guard);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    None
}

#[test]
fn captures_the_whole_screen_scaled_to_the_box() {
    let display = ":97";
    let Some(_xvfb) = start_xvfb(display) else {
        eprintln!("skipped: Xvfb not available");
        return;
    };
    // SAFETY: this is the only test in this binary, so no other thread reads the environment.
    unsafe { std::env::set_var("DISPLAY", display) };

    // Paint the top-left quarter pure red with a window.
    let (conn, num) = x11rb::connect(Some(display)).unwrap();
    let screen = &conn.setup().roots[num];
    let win = conn.generate_id().unwrap();
    conn.create_window(
        screen.root_depth,
        win,
        screen.root,
        0,
        0,
        320,
        240,
        0,
        WindowClass::INPUT_OUTPUT,
        0,
        &CreateWindowAux::new().background_pixel(0x00ff_0000),
    )
    .unwrap();
    conn.map_window(win).unwrap();
    conn.flush().unwrap();
    std::thread::sleep(Duration::from_millis(300));

    let mut capture = X11Capture::new().unwrap();
    let full = capture.frame(640, 480).unwrap();
    assert_eq!((full.width(), full.height()), (640, 480));
    let small = capture.frame(320, 320).unwrap();
    assert_eq!((small.width(), small.height()), (320, 240));

    let (w, h) = (small.width() as usize, small.height() as usize);
    let u_at = |x: usize, y: usize| small.data()[w * h + (y / 2) * (w / 2) + x / 2];
    let v_at =
        |x: usize, y: usize| small.data()[w * h + (w / 2) * (h / 2) + (y / 2) * (w / 2) + x / 2];
    assert!(v_at(40, 40) > 200 && u_at(40, 40) < 128, "red corner");
    assert!((v_at(280, 200) as i32 - 128).abs() < 8, "rest is not red");
}
