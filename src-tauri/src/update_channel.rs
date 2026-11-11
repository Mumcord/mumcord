//! Which release channel the settings show (issue #741).
//!
//! The rule is the same on every platform, including FreeBSD, where `pkg` owns the install and
//! nothing is downloaded in the app: the settings screen still names the channel, so this lives
//! on its own rather than in the updater module that only some platforms have.

use mumcord_engine::store;

use crate::commands::settings::UpdateChannel;

/// The channel updates come from: the user's choice, else the one that fits the installed
/// version (a release candidate follows release candidates, anything else stays on stable).
pub fn effective_update_channel(
    chosen: Option<store::UpdateChannel>,
    installed: &str,
) -> UpdateChannel {
    chosen.map(UpdateChannel::from).unwrap_or_else(|| {
        if is_prerelease(installed) {
            UpdateChannel::ReleaseCandidates
        } else {
            UpdateChannel::Stable
        }
    })
}

/// Whether `installed` names a pre-release (`0.8.0-rc.1`), which follows release candidates.
/// Build metadata does not count, and a development build (`dev~a1b2c3d`) is not one.
fn is_prerelease(installed: &str) -> bool {
    semver::Version::parse(installed.trim().trim_start_matches('v'))
        .is_ok_and(|version| !version.pre.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_prerelease_follows_prereleases_and_everything_else_stays_stable() {
        assert_eq!(
            effective_update_channel(None, "0.7.1"),
            UpdateChannel::Stable
        );
        assert_eq!(
            effective_update_channel(None, "0.8.0-rc.1"),
            UpdateChannel::ReleaseCandidates
        );
        assert_eq!(
            effective_update_channel(None, "dev~a1b2c3d"),
            UpdateChannel::Stable
        );
        assert_eq!(
            effective_update_channel(None, "v1.0.0+2"),
            UpdateChannel::Stable
        );
    }

    #[test]
    fn the_users_choice_wins_over_the_installed_version() {
        assert_eq!(
            effective_update_channel(Some(store::UpdateChannel::Stable), "0.8.0-rc.1"),
            UpdateChannel::Stable
        );
        assert_eq!(
            effective_update_channel(Some(store::UpdateChannel::ReleaseCandidates), "0.7.1"),
            UpdateChannel::ReleaseCandidates
        );
    }
}
