//! Updates on FreeBSD (FR-3.144–FR-3.148): the package manager owns the install.
//!
//! A Mumcord installed through `pkg` is never replaced by the app itself — the same rule as for
//! the Linux packages (FR-3.147), and the reason this module replaces `updater.rs`: everything
//! the in-app updater needs (the Tauri updater plugin, installer formats, signature checks) does
//! not exist for FreeBSD, and a package installed as root must not be swapped by the user's
//! session. The commands keep their shape so the UI and its bindings stay the same: they report
//! [`InstallKind::Pkg`] with `can_install: false`, and Settings tells the user to run
//! `pkg upgrade mumcord`.

use mumcord_engine::Engine;
use mumcord_engine::EngineResult;
use tauri::AppHandle;

use crate::commands::updates::{InstallKind, UpdatePhase, UpdateStatus};

/// Managed as Tauri state, like the real updater, so the commands stay identical.
#[derive(Clone)]
pub struct Updater;

impl Updater {
    pub fn new(_app: AppHandle, _engine: Engine) -> Self {
        Self
    }

    /// The state the UI shows: nothing to check, nothing to offer, a package install.
    pub fn status(&self) -> UpdateStatus {
        UpdateStatus {
            current_version: crate::version::DISPLAY.to_string(),
            install: InstallKind::Pkg,
            can_install: false,
            dev_build: !crate::version::is_release(),
            phase: UpdatePhase::Idle,
            offer: None,
            progress: None,
            error: None,
            last_check_ms: None,
        }
    }

    /// "Check now" contacts nothing: `pkg upgrade` is the only way to update.
    pub async fn check(&self, _force: bool) -> EngineResult<UpdateStatus> {
        Ok(self.status())
    }

    /// No download happens; the UI hides the button when `can_install` is false.
    pub fn download(&self) -> bool {
        false
    }

    pub fn cancel(&self) {}

    pub fn restart(&self, _app: &AppHandle) -> bool {
        false
    }

    /// There is no background check to start.
    pub fn start(&self) {}
}
