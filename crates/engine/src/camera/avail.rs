//! Whether camera sharing is available on a server (spec 08, FR-8.2 – FR-8.7). Cameras only ever
//! travel as relay broadcasts, so everything here asks the relay: it must be usable, offer
//! broadcasts and allow `cam`; the P2P mode must permit the relay; and the user needs a client
//! certificate. The first failing check names the reason the UI shows (FR-8.4).

use mumcord_store::{P2pMode, ServerId};

use crate::peer::{RelayInfo, RelayStatus, RelayUse};
use crate::session::{Engine, EngineEvent};

/// Why camera sharing is unavailable; the order is the order of the checks (FR-8.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraUnavailable {
    /// "This server has no relay, so camera sharing isn't available." Also covers a relay that
    /// is announced but cannot be used right now (untrusted, conflicting, down, bot left).
    NoRelay,
    /// "This server's relay is too old for camera sharing." It offers no broadcasts (`bc`).
    RelayTooOld,
    /// "This server's relay doesn't allow camera sharing." `u` lacks `cam`, or there is no `u`
    /// at all: a bot without `u` can't handle camera broadcasts.
    RelayDisallows,
    /// "Camera sharing needs the relay. Change it in Privacy & Connections." The P2P mode is
    /// `off` or `direct_only`.
    NeedsRelayMode,
    /// "Camera sharing needs a client certificate."
    NoCertificate,
    /// "Camera sharing is not available on FreeBSD yet." This platform has no camera backend
    /// (FR-13.27), so the entry stays visible but disabled.
    UnsupportedPlatform,
}

/// Whether this build has a camera backend at all.
const CAMERA_BACKEND: bool = cfg!(any(target_os = "linux", target_os = "macos", windows, test));

/// The reason camera sharing is unavailable, or `None` when it is available.
pub(crate) fn evaluate(
    relay: &RelayInfo,
    mode: P2pMode,
    has_certificate: bool,
) -> Option<CameraUnavailable> {
    if !CAMERA_BACKEND {
        return Some(CameraUnavailable::UnsupportedPlatform);
    }
    if relay.status != RelayStatus::Available {
        return Some(CameraUnavailable::NoRelay);
    }
    if relay.broadcast_max.is_none() {
        return Some(CameraUnavailable::RelayTooOld);
    }
    if !relay.uses.contains(&RelayUse::Camera) {
        return Some(CameraUnavailable::RelayDisallows);
    }
    if matches!(mode, P2pMode::Off | P2pMode::DirectOnly) {
        return Some(CameraUnavailable::NeedsRelayMode);
    }
    if !has_certificate {
        return Some(CameraUnavailable::NoCertificate);
    }
    None
}

impl Engine {
    /// Why camera sharing is unavailable on `server_id` right now; `None` when it is available.
    /// Changes arrive as [`EngineEvent::CameraAvailabilityChanged`].
    pub fn camera_unavailable(&self, server_id: ServerId) -> Option<CameraUnavailable> {
        let relay = self.relays().info(server_id);
        let mode = self.p2p_mode_of(server_id);
        // Before the own user is known nothing says the certificate is missing.
        let has_certificate = self
            .session_ctx_for(server_id)
            .map(|ctx| {
                ctx.handle.with_state(|s| {
                    s.session
                        .and_then(|me| s.users.get(&me))
                        .is_none_or(|me| me.cert_hash.as_ref().is_some_and(|h| !h.is_empty()))
                })
            })
            .unwrap_or(true);
        evaluate(&relay, mode, has_certificate)
    }

    /// Recomputes the availability of `server_id` and emits the event when it changed. Called
    /// wherever an input changes: the relay, the P2P mode.
    pub(crate) fn camera_availability_refresh(&self, server_id: ServerId) {
        let reason = self.camera_unavailable(server_id);
        if self.relays().camera_changed(server_id, reason) {
            if reason.is_some() {
                self.camera_relay_unavailable(server_id);
            }
            self.events()
                .for_server(server_id)
                .emit(EngineEvent::CameraAvailabilityChanged(reason));
        }
    }

    /// [`Self::camera_availability_refresh`] for every connected server (a global setting
    /// changed).
    pub(crate) fn camera_availability_refresh_all(&self) {
        for ctx in self.session_ctxs() {
            self.camera_availability_refresh(ctx.server_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::peer::RelayUnavailable;

    /// A relay that passes every relay-side check.
    fn good() -> RelayInfo {
        RelayInfo {
            status: RelayStatus::Available,
            uses: vec![RelayUse::Images, RelayUse::Files, RelayUse::Camera],
            broadcast_max: Some(8),
            ..RelayInfo::default()
        }
    }

    fn check(relay: &RelayInfo) -> Option<CameraUnavailable> {
        evaluate(relay, P2pMode::RelayFirst, true)
    }

    /// A platform without a camera backend names that first, whatever the relay says (FR-13.27).
    #[test]
    fn a_platform_without_a_camera_backend_says_so() {
        if CAMERA_BACKEND {
            assert_eq!(check(&good()), None);
        } else {
            assert_eq!(check(&good()), Some(CameraUnavailable::UnsupportedPlatform));
        }
    }

    #[test]
    fn everything_in_place_is_available() {
        assert_eq!(check(&good()), None);
        for mode in [
            P2pMode::DirectRelay,
            P2pMode::RelayFirst,
            P2pMode::RelayOnly,
        ] {
            assert_eq!(evaluate(&good(), mode, true), None);
        }
    }

    #[test]
    fn no_relay_is_the_first_reason() {
        let none = RelayInfo::default();
        assert_eq!(check(&none), Some(CameraUnavailable::NoRelay));
        // Every earlier-failing input at once still reports the first reason.
        assert_eq!(
            evaluate(&none, P2pMode::Off, false),
            Some(CameraUnavailable::NoRelay)
        );
    }

    #[test]
    fn an_unusable_relay_counts_as_no_relay() {
        for status in [
            RelayStatus::Unavailable(RelayUnavailable::RelayDown),
            RelayStatus::Untrusted {
                kept_blocked: false,
            },
            RelayStatus::Conflict,
        ] {
            let relay = RelayInfo { status, ..good() };
            assert_eq!(check(&relay), Some(CameraUnavailable::NoRelay));
        }
    }

    #[test]
    fn a_relay_without_broadcasts_is_too_old_whatever_u_says() {
        let relay = RelayInfo {
            broadcast_max: None,
            ..good()
        };
        assert_eq!(check(&relay), Some(CameraUnavailable::RelayTooOld));
        let old_bot = RelayInfo {
            broadcast_max: None,
            uses: Vec::new(),
            ..good()
        };
        assert_eq!(check(&old_bot), Some(CameraUnavailable::RelayTooOld));
    }

    #[test]
    fn a_relay_without_cam_in_u_does_not_allow_cameras() {
        let relay = RelayInfo {
            uses: vec![RelayUse::Images, RelayUse::Files, RelayUse::ScreenShare],
            ..good()
        };
        assert_eq!(check(&relay), Some(CameraUnavailable::RelayDisallows));
    }

    #[test]
    fn p2p_modes_without_the_relay_need_a_mode_change() {
        for mode in [P2pMode::Off, P2pMode::DirectOnly] {
            assert_eq!(
                evaluate(&good(), mode, true),
                Some(CameraUnavailable::NeedsRelayMode)
            );
        }
    }

    #[test]
    fn a_missing_certificate_is_the_last_reason() {
        assert_eq!(
            evaluate(&good(), P2pMode::RelayFirst, false),
            Some(CameraUnavailable::NoCertificate)
        );
        assert_eq!(
            evaluate(&good(), P2pMode::Off, false),
            Some(CameraUnavailable::NeedsRelayMode)
        );
    }
}
