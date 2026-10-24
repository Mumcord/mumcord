//! X11: mouse buttons via XInput2 raw button events on the root window (FR-1.21, QA.4).
//!
//! Raw events arrive regardless of focus and without a grab, so other programs keep receiving
//! the button. Events are never logged.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use x11rb::connection::{Connection, RequestConnection as _};
use x11rb::protocol::Event;
use x11rb::protocol::xinput::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{
    AtomEnum, ClientMessageEvent, ConnectionExt as _, CreateWindowAux, EventMask, Window,
    WindowClass,
};
use x11rb::rust_connection::RustConnection;

use super::mouse::{MouseButton, OnChange, StartError};

/// `XIAllMasterDevices`.
const ALL_MASTER_DEVICES: xinput::DeviceId = 1;

/// Core X11 button numbers.
fn x11_button(button: MouseButton) -> u32 {
    match button {
        MouseButton::Middle => 2,
        MouseButton::Back => 8,
        MouseButton::Forward => 9,
    }
}

/// Listens while alive; dropping it wakes and ends the event thread.
pub struct Listener {
    conn: Arc<RustConnection>,
    /// Our own input-only window, used to wake the event thread.
    wakeup: Window,
    stopped: Arc<AtomicBool>,
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
        let event = ClientMessageEvent::new(32, self.wakeup, AtomEnum::NONE, [0u32; 5]);
        let sent = self
            .conn
            .send_event(false, self.wakeup, EventMask::NO_EVENT, event)
            .map(drop)
            .and_then(|()| self.conn.flush());
        if let Err(err) = sent {
            tracing::debug!("waking the X11 mouse thread failed: {err}");
        }
    }
}

fn failed(err: impl std::fmt::Display) -> StartError {
    StartError::Failed(format!("X11: {err}"))
}

pub fn start(button: MouseButton, on_change: OnChange) -> Result<Listener, StartError> {
    let (conn, screen) = RustConnection::connect(None).map_err(failed)?;
    let root = conn
        .setup()
        .roots
        .get(screen)
        .ok_or_else(|| failed("no screen"))?
        .root;
    if conn
        .extension_information(xinput::X11_EXTENSION_NAME)
        .map_err(failed)?
        .is_none()
    {
        return Err(failed("XInput extension missing"));
    }
    // Raw events without an active grab need XInput 2.1.
    let version = conn
        .xinput_xi_query_version(2, 2)
        .map_err(failed)?
        .reply()
        .map_err(failed)?;
    if (version.major_version, version.minor_version) < (2, 1) {
        return Err(failed("XInput 2.1 required"));
    }
    let mask = xinput::EventMask {
        deviceid: ALL_MASTER_DEVICES,
        mask: vec![xinput::XIEventMask::RAW_BUTTON_PRESS | xinput::XIEventMask::RAW_BUTTON_RELEASE],
    };
    conn.xinput_xi_select_events(root, &[mask])
        .map_err(failed)?
        .check()
        .map_err(failed)?;
    let wakeup = conn.generate_id().map_err(failed)?;
    conn.create_window(
        0,
        wakeup,
        root,
        0,
        0,
        1,
        1,
        0,
        WindowClass::INPUT_ONLY,
        x11rb::COPY_FROM_PARENT,
        &CreateWindowAux::new(),
    )
    .map_err(failed)?
    .check()
    .map_err(failed)?;

    let conn = Arc::new(conn);
    let stopped = Arc::new(AtomicBool::new(false));
    let thread_conn = conn.clone();
    let thread_stopped = stopped.clone();
    let wanted = x11_button(button);
    std::thread::Builder::new()
        .name("ptt-mouse-x11".to_owned())
        .spawn(move || {
            while let Ok(event) = thread_conn.wait_for_event() {
                if thread_stopped.load(Ordering::SeqCst) {
                    break;
                }
                match event {
                    Event::XinputRawButtonPress(e) if e.detail == wanted => on_change(true),
                    Event::XinputRawButtonRelease(e) if e.detail == wanted => on_change(false),
                    _ => {}
                }
            }
        })
        .map_err(failed)?;
    Ok(Listener {
        conn,
        wakeup,
        stopped,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    #[test]
    fn maps_to_x11_buttons() {
        assert_eq!(x11_button(MouseButton::Middle), 2);
        assert_eq!(x11_button(MouseButton::Back), 8);
        assert_eq!(x11_button(MouseButton::Forward), 9);
    }

    /// X11 end-to-end with `xdotool`; use a private server (see `ptt::tests::x11_press_and_release`):
    /// `DISPLAY=:57 cargo test -p mumcord mouse_x11 -- --ignored`.
    #[test]
    #[ignore = "needs an X11 display and xdotool"]
    fn x11_side_button_press_and_release() {
        let (tx, rx) = mpsc::channel();
        let listener = start(
            MouseButton::Back,
            Arc::new(move |pressed| {
                let _ = tx.send(pressed);
            }),
        )
        .expect("start X11 listener");
        let xdotool = |action: &str, button: &str| {
            let status = std::process::Command::new("xdotool")
                .args([action, button])
                .status()
                .expect("run xdotool");
            assert!(status.success());
        };
        xdotool("mousedown", "9");
        xdotool("mouseup", "9");
        xdotool("mousedown", "8");
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)), Ok(true));
        xdotool("mouseup", "8");
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)), Ok(false));
        drop(listener);
    }
}
