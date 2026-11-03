//! Whether the machine runs on battery right now (spec 07 §9): Automatic then skips software
//! AV1, which would drain it.

use crate::test_hooks;

pub fn on_battery() -> bool {
    test_hooks::on_battery() || os_on_battery()
}

#[cfg(target_os = "linux")]
fn os_on_battery() -> bool {
    on_battery_in(std::path::Path::new("/sys/class/power_supply"))
}

/// A plugged-in charger wins; otherwise a discharging battery means battery power. Desktops
/// without any power supply entries count as plugged in.
#[cfg(any(target_os = "linux", test))]
fn on_battery_in(dir: &std::path::Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    let read = |path: std::path::PathBuf| {
        std::fs::read_to_string(path)
            .map(|s| s.trim().to_owned())
            .unwrap_or_default()
    };
    let mut discharging = false;
    for entry in entries.flatten() {
        let path = entry.path();
        match read(path.join("type")).as_str() {
            "Mains" | "USB" if read(path.join("online")) == "1" => return false,
            "Battery" if read(path.join("status")) == "Discharging" => discharging = true,
            _ => {}
        }
    }
    discharging
}

#[cfg(windows)]
fn os_on_battery() -> bool {
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    let mut status = SYSTEM_POWER_STATUS::default();
    // SAFETY: `status` is a valid, writable SYSTEM_POWER_STATUS.
    match unsafe { GetSystemPowerStatus(&mut status) } {
        // 0 = offline (battery), 1 = online, 255 = unknown.
        Ok(()) => status.ACLineStatus == 0,
        Err(e) => {
            tracing::debug!(error = %e, "power status not readable");
            false
        }
    }
}

#[cfg(target_os = "macos")]
fn os_on_battery() -> bool {
    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOPSGetTimeRemainingEstimate() -> f64;
    }
    /// `kIOPSTimeRemainingUnlimited`: attached to an unlimited power source.
    const UNLIMITED: f64 = -2.0;
    // SAFETY: no arguments, no preconditions.
    let remaining = unsafe { IOPSGetTimeRemainingEstimate() };
    remaining != UNLIMITED
}

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
fn os_on_battery() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn supply(dir: &std::path::Path, name: &str, files: &[(&str, &str)]) {
        let path = dir.join(name);
        std::fs::create_dir(&path).expect("dir");
        for (file, value) in files {
            std::fs::write(path.join(file), format!("{value}\n")).expect("write");
        }
    }

    #[test]
    fn a_discharging_battery_without_a_charger_is_battery_power() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(!on_battery_in(dir.path()));
        supply(dir.path(), "AC", &[("type", "Mains"), ("online", "0")]);
        supply(
            dir.path(),
            "BAT0",
            &[("type", "Battery"), ("status", "Discharging")],
        );
        assert!(on_battery_in(dir.path()));
        std::fs::write(dir.path().join("AC/online"), "1\n").expect("plug in");
        assert!(!on_battery_in(dir.path()));
    }

    #[test]
    fn a_charging_battery_or_no_supply_is_plugged_in() {
        let dir = tempfile::tempdir().expect("tempdir");
        supply(
            dir.path(),
            "BAT0",
            &[("type", "Battery"), ("status", "Charging")],
        );
        assert!(!on_battery_in(dir.path()));
        assert!(!on_battery_in(&dir.path().join("missing")));
    }
}
