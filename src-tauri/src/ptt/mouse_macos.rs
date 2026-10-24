//! macOS: mouse buttons via a listen-only `CGEventTap` (FR-1.21, QA.4).
//!
//! If the system refuses the tap, Mumcord asks for the Accessibility permission (the system
//! shows its prompt) and reports it as missing. Events are never logged.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::runloop::{CFRunLoop, kCFRunLoopDefaultMode};
use core_foundation::string::{CFString, CFStringRef};
use core_graphics::event::{
    CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
    CallbackResult, EventField,
};

use super::mouse::{MouseButton, OnChange, StartError};

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    static kAXTrustedCheckOptionPrompt: CFStringRef;
    fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;
}

/// How often the tap thread checks whether it should stop.
const STOP_CHECK: Duration = Duration::from_millis(500);

/// macOS button numbers (0 = left, 1 = right).
fn mac_button(button: MouseButton) -> i64 {
    match button {
        MouseButton::Middle => 2,
        MouseButton::Back => 3,
        MouseButton::Forward => 4,
    }
}

/// Listens while alive; dropping it stops the tap's run loop.
pub struct Listener {
    run_loop: CFRunLoop,
    stopped: Arc<AtomicBool>,
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
        self.run_loop.stop();
    }
}

pub fn start(button: MouseButton, on_change: OnChange) -> Result<Listener, StartError> {
    let wanted = mac_button(button);
    let stopped = Arc::new(AtomicBool::new(false));
    let thread_stopped = stopped.clone();
    let (ready_tx, ready_rx) = mpsc::channel::<Option<CFRunLoop>>();
    let failed_tx = ready_tx.clone();
    std::thread::Builder::new()
        .name("ptt-mouse-tap".to_owned())
        .spawn(move || {
            let tapped = CGEventTap::with_enabled(
                CGEventTapLocation::Session,
                CGEventTapPlacement::HeadInsertEventTap,
                CGEventTapOptions::ListenOnly,
                vec![CGEventType::OtherMouseDown, CGEventType::OtherMouseUp],
                move |_proxy, event_type, event| {
                    if event.get_integer_value_field(EventField::MOUSE_EVENT_BUTTON_NUMBER)
                        == wanted
                    {
                        match event_type {
                            CGEventType::OtherMouseDown => on_change(true),
                            CGEventType::OtherMouseUp => on_change(false),
                            _ => {}
                        }
                    }
                    CallbackResult::Keep
                },
                || {
                    let _ = ready_tx.send(Some(CFRunLoop::get_current()));
                    while !thread_stopped.load(Ordering::SeqCst) {
                        // SAFETY: reading an immutable CoreFoundation constant.
                        let mode = unsafe { kCFRunLoopDefaultMode };
                        CFRunLoop::run_in_mode(mode, STOP_CHECK, false);
                    }
                },
            );
            if tapped.is_err() {
                let _ = failed_tx.send(None);
            }
        })
        .map_err(|err| StartError::Failed(err.to_string()))?;
    match ready_rx.recv() {
        Ok(Some(run_loop)) => Ok(Listener { run_loop, stopped }),
        Ok(None) | Err(_) if !request_accessibility() => Err(StartError::NeedsAccessibility),
        Ok(None) | Err(_) => Err(StartError::Failed("the event tap was refused".to_owned())),
    }
}

/// Whether Mumcord is trusted for Accessibility; if not, the system shows its prompt.
fn request_accessibility() -> bool {
    // SAFETY: `kAXTrustedCheckOptionPrompt` is a constant CFString owned by the framework.
    let key = unsafe { CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt) };
    let options = CFDictionary::from_CFType_pairs(&[(key, CFBoolean::true_value())]);
    // SAFETY: `options` is a valid CFDictionary for the duration of the call.
    unsafe { AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_to_mac_buttons() {
        assert_eq!(mac_button(MouseButton::Middle), 2);
        assert_eq!(mac_button(MouseButton::Back), 3);
        assert_eq!(mac_button(MouseButton::Forward), 4);
    }
}
