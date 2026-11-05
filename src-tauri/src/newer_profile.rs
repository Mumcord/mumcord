//! A profile that a newer Mumcord has already upgraded (issue #510). This app cannot read it
//! and must not change it, so start-up stops with a native message before Tauri, the web
//! view, audio or plugins start. A failure later, in Tauri's setup hook, would end in a
//! bare panic message from Tauri.
//!
//! When the newer app stored how to undo its changes, the message also offers to roll the
//! profile back to this version (issue #521): a full backup copy is saved first, the newer
//! changes are undone and start-up continues.
//!
//! The message is English only: the UI language is a setting inside the very database this
//! app cannot read, and the web view (with the translations) does not run yet.

use std::path::Path;

use mumcord_engine::store::{self, Error};
use rfd::{MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};

use crate::{browser, cache_dirs};

const DOWNLOAD_URL: &str = "https://github.com/Mumcord/mumcord/releases/latest";
const DOWNLOAD: &str = "Download the latest version";
const QUIT: &str = "Quit";
const ROLL_BACK: &str = "Roll back the profile to this version";

#[derive(Debug, PartialEq, Eq)]
struct NewerProfile {
    found: i64,
    supported: i64,
    /// The schema version a rollback would leave, when one is possible.
    rollback_to: Option<i64>,
}

impl NewerProfile {
    fn message(&self) -> String {
        let mut message = format!(
            "This profile was created by a newer version of Mumcord (profile version {}, this \
             app supports up to {}). Please install the latest version of Mumcord to use it.\n\n\
             Your profile has not been changed.",
            self.found, self.supported
        );
        if let Some(to) = self.rollback_to {
            message.push_str(&format!(
                "\n\nYou can also roll the profile back to this version. Everything the newer \
                 versions added to it (profile versions {} to {}) will be lost; your servers, \
                 identities, history and settings from this version stay. A full backup copy of \
                 the profile is saved next to it first.",
                to + 1,
                self.found
            ));
        }
        message
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Choice {
    Download,
    RollBack,
    Quit,
}

/// Ends the process with the message if the profile is too new; returns otherwise. Must run
/// before anything opens the database.
/// Returns as well after the user rolled the profile back.
pub fn stop_if_newer(identifier: &str) {
    let Some(dirs) = cache_dirs::before_start(identifier) else {
        return;
    };
    let Some(newer) = detect(&dirs.data) else {
        return;
    };
    // Logging starts later; stderr also reaches users who start Mumcord from a terminal.
    eprintln!("{}", newer.message());
    if !can_show_dialog() {
        std::process::exit(1);
    }
    match ask(&newer) {
        Choice::Download => {
            if let Err(e) = browser::open_url_before_exit(DOWNLOAD_URL) {
                eprintln!("the download page could not be opened: {e}");
            }
        }
        Choice::RollBack => match store::roll_back(&dirs.data) {
            Ok(_) => {
                eprintln!("the profile was rolled back; a backup copy was saved next to it");
                return;
            }
            Err(e) => {
                let message = format!(
                    "Rolling the profile back failed: {e}\n\nYour profile has not been changed."
                );
                eprintln!("{message}");
                MessageDialog::new()
                    .set_level(MessageLevel::Error)
                    .set_title("Mumcord could not roll back the profile")
                    .set_description(message)
                    .set_buttons(MessageButtons::Ok)
                    .show();
            }
        },
        Choice::Quit => {}
    }
    std::process::exit(1);
}

/// The newer profile in `data_dir`, if it is one. Other read errors are left to the normal
/// start-up, which reports them.
fn detect(data_dir: &Path) -> Option<NewerProfile> {
    match store::check_schema(data_dir) {
        Ok(()) => None,
        Err(Error::SchemaTooNew { found, supported }) => {
            let rollback_to = store::rollback_plan(data_dir)
                .unwrap_or_else(|e| {
                    eprintln!("checking whether the profile can be rolled back failed: {e}");
                    None
                })
                .map(|plan| plan.to);
            Some(NewerProfile {
                found,
                supported,
                rollback_to,
            })
        }
        Err(e) => {
            eprintln!("checking the profile version failed: {e}");
            None
        }
    }
}

/// Without a display the dialog's GTK thread never starts and the dialog would wait forever;
/// the message on stderr is all there is then.
#[cfg(target_os = "linux")]
fn can_show_dialog() -> bool {
    ["WAYLAND_DISPLAY", "DISPLAY"]
        .iter()
        .any(|var| std::env::var_os(var).is_some_and(|value| !value.is_empty()))
}

#[cfg(not(target_os = "linux"))]
fn can_show_dialog() -> bool {
    true
}

fn ask(newer: &NewerProfile) -> Choice {
    let result = MessageDialog::new()
        .set_level(MessageLevel::Warning)
        .set_title("Mumcord needs an update")
        .set_description(newer.message())
        .set_buttons(if newer.rollback_to.is_some() {
            MessageButtons::YesNoCancelCustom(DOWNLOAD.into(), ROLL_BACK.into(), QUIT.into())
        } else {
            MessageButtons::OkCancelCustom(DOWNLOAD.into(), QUIT.into())
        })
        .show();
    choice(&result)
}

/// Closing the dialog any other way (Escape, the window's close button) means Quit.
fn choice(result: &MessageDialogResult) -> Choice {
    match result {
        MessageDialogResult::Ok => Choice::Download,
        MessageDialogResult::Custom(label) if label == DOWNLOAD => Choice::Download,
        MessageDialogResult::Custom(label) if label == ROLL_BACK => Choice::RollBack,
        _ => Choice::Quit,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mumcord_engine::store::{DB_FILE_NAME, Store};

    /// A profile a newer Mumcord upgraded with a breaking migration.
    fn newer_profile(dir: &Path) -> i64 {
        let supported = Store::open(dir)
            .and_then(|store| store.schema_version())
            .expect("current profile");
        // The closed database holds everything in its file; `user_version` and the minimum
        // reader version (`application_id`) are big-endian header fields at bytes 60 and 68.
        let path = dir.join(DB_FILE_NAME);
        let mut bytes = std::fs::read(&path).expect("read");
        let found = i32::try_from(supported + 4).expect("small version");
        bytes[60..64].copy_from_slice(&found.to_be_bytes());
        bytes[68..72].copy_from_slice(&(found - 1).to_be_bytes());
        std::fs::write(&path, bytes).expect("write");
        supported
    }

    #[test]
    fn a_newer_profile_is_detected_and_left_byte_identical() {
        let dir = tempfile::tempdir().expect("tempdir");
        let supported = newer_profile(dir.path());
        let path = dir.path().join(DB_FILE_NAME);
        let before = std::fs::read(&path).expect("read");

        let newer = detect(dir.path()).expect("newer profile");
        assert_eq!(
            newer,
            NewerProfile {
                found: supported + 4,
                supported,
                rollback_to: None
            }
        );
        let message = newer.message();
        assert!(message.contains(&format!("profile version {}", supported + 4)));
        assert!(message.contains(&format!("supports up to {supported}")));
        assert!(!message.contains("roll the profile back"));

        assert_eq!(std::fs::read(&path).expect("read"), before);
        assert!(!dir.path().join(format!("{DB_FILE_NAME}-wal")).exists());
    }

    #[test]
    fn a_current_or_missing_profile_starts_normally() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(detect(dir.path()), None);
        drop(Store::open(dir.path()).expect("open"));
        assert_eq!(detect(dir.path()), None);
    }

    #[test]
    fn only_the_download_button_opens_the_download_page() {
        assert_eq!(
            choice(&MessageDialogResult::Custom(DOWNLOAD.into())),
            Choice::Download
        );
        assert_eq!(choice(&MessageDialogResult::Ok), Choice::Download);
        assert_eq!(
            choice(&MessageDialogResult::Custom(QUIT.into())),
            Choice::Quit
        );
        assert_eq!(choice(&MessageDialogResult::Cancel), Choice::Quit);
        assert_eq!(
            choice(&MessageDialogResult::Custom(ROLL_BACK.into())),
            Choice::RollBack
        );
    }

    #[test]
    fn the_message_explains_what_a_rollback_loses() {
        let newer = NewerProfile {
            found: 33,
            supported: 30,
            rollback_to: Some(31),
        };
        let message = newer.message();
        assert!(message.contains("roll the profile back to this version"));
        assert!(message.contains("(profile versions 32 to 33) will be lost"));
        assert!(message.contains("backup copy"));
    }
}
