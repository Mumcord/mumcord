//! The running programs, handed to `mumble_initPositionalData` so a plugin can look for its
//! game. Names are executable file names without the path, like the official client passes
//! them (Wine games appear under their `.exe` name).

use std::ffi::CString;

/// (program name, process id) of every running program that could be read.
pub(crate) fn running() -> Vec<(CString, u64)> {
    platform::running()
        .into_iter()
        .filter_map(|(name, pid)| Some((CString::new(name).ok()?, pid)))
        .filter(|(name, _)| !name.as_bytes().is_empty())
        .collect()
}

/// The file name part of a program path, with `/` or `\` (Wine) as separator.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

#[cfg(target_os = "linux")]
mod platform {
    use super::file_name;

    pub(super) fn running() -> Vec<(String, u64)> {
        let Ok(entries) = std::fs::read_dir("/proc") else {
            tracing::warn!("listing the running programs failed: /proc is not readable");
            return Vec::new();
        };
        entries
            .filter_map(|e| {
                let pid: u64 = e.ok()?.file_name().to_str()?.parse().ok()?;
                Some((name(pid)?, pid))
            })
            .collect()
    }

    /// The first command line argument (the full name; `comm` is cut at 15 bytes), else
    /// `comm`. Processes that end meanwhile are skipped.
    fn name(pid: u64) -> Option<String> {
        let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
        let first = cmdline.split(|&b| b == 0).next().unwrap_or_default();
        let first = String::from_utf8_lossy(first);
        let name = file_name(first.trim());
        if !name.is_empty() {
            return Some(name.to_owned());
        }
        let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
        Some(comm.trim().to_owned())
    }
}

#[cfg(target_os = "macos")]
mod platform {
    pub(super) fn running() -> Vec<(String, u64)> {
        let size_of_pid = std::mem::size_of::<libc::pid_t>();
        // SAFETY: a null buffer asks for the number of processes.
        let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
        let Ok(count) = usize::try_from(count) else {
            tracing::warn!("listing the running programs failed");
            return Vec::new();
        };
        // Room for programs started in between.
        let mut pids: Vec<libc::pid_t> = vec![0; count + 64];
        let bytes = i32::try_from(pids.len() * size_of_pid).unwrap_or(i32::MAX);
        // SAFETY: the buffer holds `bytes` bytes.
        let filled = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
        pids.truncate(usize::try_from(filled).unwrap_or(0).min(pids.len()));
        pids.into_iter()
            .filter(|&pid| pid > 0)
            .filter_map(|pid| {
                let mut buf = [0u8; 256];
                // SAFETY: the buffer holds `buf.len()` bytes.
                let len =
                    unsafe { libc::proc_name(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
                let len = usize::try_from(len).ok().filter(|&l| l > 0)?;
                let name = String::from_utf8_lossy(&buf[..len.min(buf.len())]).into_owned();
                Some((name, u64::try_from(pid).ok()?))
            })
            .collect()
    }
}

#[cfg(windows)]
mod platform {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };

    pub(super) fn running() -> Vec<(String, u64)> {
        // SAFETY: plain call; the handle is closed below.
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            tracing::warn!("listing the running programs failed");
            return Vec::new();
        }
        let mut out = Vec::new();
        // SAFETY: an all-zero PROCESSENTRY32W is valid; `dwSize` is set as required.
        let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        // SAFETY: valid snapshot handle and entry.
        let mut more = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
        while more {
            let len = entry
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szExeFile.len());
            let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
            out.push((name, u64::from(entry.th32ProcessID)));
            // SAFETY: as above.
            more = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
        }
        // SAFETY: the snapshot handle is owned here and closed once.
        unsafe { CloseHandle(snapshot) };
        out
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
mod platform {
    pub(super) fn running() -> Vec<(String, u64)> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_drop_unix_and_wine_paths() {
        assert_eq!(file_name("/usr/bin/game"), "game");
        assert_eq!(file_name(r"Z:\games\GTA5.exe"), "GTA5.exe");
        assert_eq!(file_name("plain"), "plain");
    }

    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    #[test]
    fn lists_this_test_program() {
        let me = std::env::current_exe().expect("exe");
        let me = me.file_name().and_then(|n| n.to_str()).expect("name");
        let pid = u64::from(std::process::id());
        let found = running()
            .into_iter()
            .find(|(_, p)| *p == pid)
            .expect("own process listed");
        // macOS keeps at most 32 characters of the name.
        assert!(
            me.starts_with(found.0.to_str().expect("utf-8")),
            "{found:?}"
        );
    }
}
