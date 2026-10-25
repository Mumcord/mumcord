//! Runs a system tool and returns what it printed (Windows `tasklist`/`reg`, macOS `ps`).

use std::process::Command;

/// The tool's output, or `None` when it could not run or failed (e.g. a registry value that
/// does not exist).
pub fn output(program: &str, args: &[&str]) -> Option<String> {
    let mut command = Command::new(program);
    command.args(args);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        // No console window flashing up every few seconds.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    match command.output() {
        Ok(out) if out.status.success() => Some(String::from_utf8_lossy(&out.stdout).into_owned()),
        Ok(_) => None,
        Err(e) => {
            tracing::debug!(error = %e, program, "running a system tool failed");
            None
        }
    }
}
