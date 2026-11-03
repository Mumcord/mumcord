//! The graphics driver fingerprint the probe results are cached under (FR-7.55): when it
//! changes, a driver was updated or swapped and the encoders are probed again.

/// A string that changes whenever a graphics driver the hardware encoders use changes.
pub fn driver_id() -> String {
    os_driver_id()
}

#[cfg(target_os = "linux")]
fn os_driver_id() -> String {
    use std::path::Path;

    let read = |path: &str| {
        std::fs::read_to_string(path)
            .map(|s| s.trim().to_owned())
            .unwrap_or_default()
    };
    let mut parts = vec![
        format!("kernel {}", read("/proc/sys/kernel/osrelease")),
        format!("nvidia {}", read("/sys/module/nvidia/version")),
    ];
    // Mesa and Intel's media driver update without a kernel change; their VA-API drivers'
    // size and modification time tell an update apart.
    let mut dirs: Vec<String> = std::env::var("LIBVA_DRIVERS_PATH")
        .map(|v| v.split(':').map(str::to_owned).collect())
        .unwrap_or_default();
    dirs.extend(
        [
            "/usr/lib/dri",
            "/usr/lib64/dri",
            "/usr/lib/x86_64-linux-gnu/dri",
            "/usr/lib/aarch64-linux-gnu/dri",
        ]
        .map(str::to_owned),
    );
    parts.extend(va_drivers(dirs.iter().map(Path::new)));
    parts.join("; ")
}

/// `name size mtime` of every `*_drv_video.so` in `dirs`, sorted.
#[cfg(any(target_os = "linux", test))]
fn va_drivers<'a>(dirs: impl Iterator<Item = &'a std::path::Path>) -> Vec<String> {
    let mut found = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.ends_with("_drv_video.so") {
                continue;
            }
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs());
            found.push(format!(
                "{} {} {modified}",
                entry.path().display(),
                meta.len()
            ));
        }
    }
    found.sort();
    found
}

#[cfg(windows)]
fn os_driver_id() -> String {
    // The display adapter class; each numbered subkey is one installed adapter driver.
    const CLASS: &str =
        r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}";
    let mut parts = Vec::new();
    for index in 0..16 {
        let Ok(key) = windows_registry::LOCAL_MACHINE.open(format!(r"{CLASS}\{index:04}")) else {
            continue;
        };
        let desc = key.get_string("DriverDesc").unwrap_or_default();
        let version = key.get_string("DriverVersion").unwrap_or_default();
        parts.push(format!("{desc} {version}"));
    }
    parts.join("; ")
}

/// macOS has no hardware AV1 or VP9 encoder to probe (FR-7.55); only app updates matter.
#[cfg(not(any(target_os = "linux", windows)))]
fn os_driver_id() -> String {
    std::env::consts::OS.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_id_is_stable_between_calls() {
        assert_eq!(driver_id(), driver_id());
    }

    #[test]
    fn a_changed_va_driver_changes_the_list() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("readme.txt"), "x").expect("write");
        assert!(va_drivers(std::iter::once(dir.path())).is_empty());
        let driver = dir.path().join("iHD_drv_video.so");
        std::fs::write(&driver, "v1").expect("write");
        let before = va_drivers(std::iter::once(dir.path()));
        assert_eq!(before.len(), 1);
        std::fs::write(&driver, "version 2").expect("update");
        assert_ne!(va_drivers(std::iter::once(dir.path())), before);
    }
}
