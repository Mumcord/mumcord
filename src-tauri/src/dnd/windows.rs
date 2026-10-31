//! Reads Windows' Focus Assist / Do not disturb state.
//!
//! Windows has no public API for it. The shell publishes the active quiet-hours profile as a
//! WNF state (0 = off, 1 = priority only, 2 = alarms only), which is what other desktop apps
//! read as well; it is polled because subscribing needs further undocumented calls.

use std::ffi::c_void;
use std::time::Duration;

use tauri::AppHandle;

use super::set_system;
use crate::commands::dnd::SystemDnd;

/// `WNF_SHEL_QUIETHOURS_ACTIVE_PROFILE_CHANGED`.
const QUIET_HOURS_PROFILE: u64 = 0x0D83_063E_A3BF_1C75;
const POLL: Duration = Duration::from_secs(3);

#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtQueryWnfStateData(
        state_name: *const u64,
        type_id: *const c_void,
        explicit_scope: *const c_void,
        change_stamp: *mut u32,
        buffer: *mut c_void,
        buffer_size: *mut u32,
    ) -> i32;
}

/// The active quiet-hours profile; `None` when Windows doesn't answer.
fn quiet_hours_profile() -> Option<u32> {
    let mut stamp = 0u32;
    let mut profile = 0u32;
    let mut size = u32::try_from(size_of::<u32>()).ok()?;
    // SAFETY: every pointer refers to a live local of the size passed; the call writes at most
    // `size` bytes into `profile`.
    let status = unsafe {
        NtQueryWnfStateData(
            &QUIET_HOURS_PROFILE,
            std::ptr::null(),
            std::ptr::null(),
            &mut stamp,
            (&raw mut profile).cast(),
            &mut size,
        )
    };
    // A state that was never written has no data: Focus Assist was never turned on.
    (status >= 0).then_some(if size == 0 { 0 } else { profile })
}

fn system_state(profile: Option<u32>) -> SystemDnd {
    profile.map_or_else(SystemDnd::default, |profile| SystemDnd {
        desktop: Some("Windows".into()),
        on: Some(profile != 0),
    })
}

/// Polls the state for the app's lifetime.
pub fn watch(app: AppHandle) {
    let spawned = std::thread::Builder::new()
        .name("dnd-watch".into())
        .spawn(move || {
            loop {
                set_system(&app, system_state(quiet_hours_profile()));
                std::thread::sleep(POLL);
            }
        });
    if let Err(e) = spawned {
        tracing::warn!(error = %e, "starting the Do Not Disturb reader failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn any_quiet_hours_profile_counts_as_on() {
        assert_eq!(system_state(Some(0)).on, Some(false));
        assert_eq!(system_state(Some(1)).on, Some(true));
        assert_eq!(system_state(Some(2)).on, Some(true));
        assert_eq!(system_state(None), SystemDnd::default());
    }
}
