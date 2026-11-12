//! Whether camera sharing is available on a server (spec 08, FR-8.2 – FR-8.7). A camera reaches
//! its viewers over the relay broadcast or, between 0.9.0 clients, over a direct stream, so it is
//! available when either path can work: the relay path needs a relay that is usable, offers
//! broadcasts and allows `cam`, the direct path only a P2P mode that permits it (FR-8.96). Either
//! way the user needs a client certificate and the mode must not be `off`. Only when no path is
//! left does a reason name why (FR-8.4) — a relay problem alone keeps the camera available.

use mumcord_store::{Consent, P2pMode, ServerId};

use crate::peer::{RelayInfo, RelayStatus, RelayUse};
use crate::session::{Engine, EngineEvent};

/// Why camera sharing is unavailable; the order is the order of the checks (FR-8.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraUnavailable {
    /// "This server has no relay, so camera sharing isn't available." Only `relay_only` mode
    /// leaves no path without the relay (FR-8.94). Also covers a relay that is announced but
    /// cannot be used right now (untrusted, conflicting, down, bot left).
    NoRelay,
    /// "This server's relay is too old for camera sharing." It offers no broadcasts (`bc`).
    RelayTooOld,
    /// "This server's relay doesn't allow camera sharing." `u` lacks `cam`, or there is no `u`
    /// at all: a bot without `u` can't handle camera broadcasts.
    RelayDisallows,
    /// "Camera sharing is turned off. Change it in Privacy & Connections." The P2P mode is
    /// `off`. The variant keeps the name the UI bindings know; a relay problem no longer
    /// reports it (FR-8.4).
    NeedsRelayMode,
    /// "Camera sharing needs a client certificate."
    NoCertificate,
    /// "Camera sharing needs direct connections. Change it in Privacy & Connections." The mode
    /// leaves only the direct path and the user turned direct connections off (FR-4.3, FR-8.4).
    NeedsDirect,
    /// "Camera sharing is not available on FreeBSD yet." This platform has no camera backend
    /// (FR-13.27), so the entry stays visible but disabled.
    UnsupportedPlatform,
}

/// Whether the relay can carry camera broadcasts at all (FR-8.2): it must be usable, offer
/// broadcasts and allow `cam` — a bot without `u` can't handle camera broadcasts. A missing `mv`
/// only means the old media format (FR-7.96), not a missing path.
fn relay_carries_cameras(relay: &RelayInfo) -> Result<(), CameraUnavailable> {
    if relay.status != RelayStatus::Available {
        return Err(CameraUnavailable::NoRelay);
    }
    if relay.broadcast_max.is_none() {
        return Err(CameraUnavailable::RelayTooOld);
    }
    if !relay.uses.contains(&RelayUse::Camera) {
        return Err(CameraUnavailable::RelayDisallows);
    }
    Ok(())
}

/// Whether this build has a camera backend at all.
const CAMERA_BACKEND: bool = cfg!(any(target_os = "linux", target_os = "macos", windows, test));

/// The reason camera sharing is unavailable, or `None` when it is available.
pub(crate) fn evaluate(
    relay: &RelayInfo,
    mode: P2pMode,
    has_certificate: bool,
    direct_allowed: bool,
) -> Option<CameraUnavailable> {
    // Without a camera backend no mode and no relay can help (FR-13.27).
    if !CAMERA_BACKEND {
        return Some(CameraUnavailable::UnsupportedPlatform);
    }
    // `relay_only` leaves the relay as the only path, so the three relay reasons come first
    // (FR-8.4). Every other mode keeps the direct path, and with it the camera (FR-8.2).
    if mode == P2pMode::RelayOnly
        && let Err(reason) = relay_carries_cameras(relay)
    {
        return Some(reason);
    }
    if mode == P2pMode::Off {
        return Some(CameraUnavailable::NeedsRelayMode);
    }
    if !has_certificate {
        return Some(CameraUnavailable::NoCertificate);
    }
    // The mode and the relay leave the direct path only, and the user turned it off: there is
    // nothing left to carry the camera (FR-8.4).
    if Paths::of(relay, mode, direct_allowed).is_none() {
        return Some(CameraUnavailable::NeedsDirect);
    }
    None
}

/// The ways the own camera reaches viewers (FR-8.94): one relay broadcast, and one direct stream
/// per admitted 0.9.0 viewer. With an old relay both run at the same time — the old-format
/// broadcast serves 0.8.x viewers while 0.9.0 viewers get direct streams (FR-8.95).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Paths {
    /// The camera opens a relay broadcast.
    pub relay: bool,
    /// The camera opens one `STREAM_CAMERA` per admitted 0.9.0 viewer.
    pub direct: bool,
    /// The relay broadcast carries the Bovnar media profile (announced `mv`, FR-7.96).
    pub media: bool,
}

impl Paths {
    /// The paths `mode` leaves with this relay, or `None` when neither is left: `relay_only`
    /// never opens a direct stream, `direct_only` never a relay broadcast, `off` uses neither
    /// (FR-8.94).
    pub(crate) fn of(relay: &RelayInfo, mode: P2pMode, direct_allowed: bool) -> Option<Self> {
        let broadcast = !matches!(mode, P2pMode::DirectOnly | P2pMode::Off)
            && relay_carries_cameras(relay).is_ok();
        let direct = direct_allowed && !matches!(mode, P2pMode::RelayOnly | P2pMode::Off);
        (broadcast || direct).then_some(Self {
            relay: broadcast,
            direct,
            media: broadcast && relay.media,
        })
    }
}

impl Engine {
    /// Why camera sharing is unavailable on `server_id` right now; `None` when it is available.
    /// Changes arrive as [`EngineEvent::CameraAvailabilityChanged`].
    pub fn camera_unavailable(&self, server_id: ServerId) -> Option<CameraUnavailable> {
        let relay = self.relays().info(server_id);
        let (mode, consent) = self.p2p_mode_and_consent_of(server_id);
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
        evaluate(&relay, mode, has_certificate, consent != Consent::No)
    }

    /// Recomputes the availability of `server_id` and emits the event when it changed. Called
    /// wherever an input changes: the relay, the P2P mode.
    pub(crate) fn camera_availability_refresh(&self, server_id: ServerId) {
        let reason = self.camera_unavailable(server_id);
        // A camera on air ends when its relay can no longer carry it (FR-8.7, FR-8.24), and when
        // no path is left at all.
        self.camera_path_lost(server_id, reason.is_some());
        if self.relays().camera_changed(server_id, reason) {
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

    /// A new relay that passes every relay-side check.
    fn good() -> RelayInfo {
        RelayInfo {
            status: RelayStatus::Available,
            uses: vec![RelayUse::Images, RelayUse::Files, RelayUse::Camera],
            broadcast_max: Some(8),
            media: true,
            ..RelayInfo::default()
        }
    }

    /// A relay that carries camera broadcasts in the old media format.
    fn old() -> RelayInfo {
        RelayInfo {
            media: false,
            ..good()
        }
    }

    fn check(relay: &RelayInfo) -> Option<CameraUnavailable> {
        evaluate(relay, P2pMode::RelayFirst, true, true)
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
            P2pMode::DirectOnly,
        ] {
            assert_eq!(evaluate(&good(), mode, true, true), None, "{mode:?}");
        }
    }

    #[test]
    fn a_relay_problem_alone_keeps_the_camera_available() {
        // The direct path carries the camera, so no relay reason is reported (FR-8.4).
        let relays = [
            RelayInfo::default(),
            RelayInfo {
                status: RelayStatus::Unavailable(RelayUnavailable::RelayDown),
                ..good()
            },
            RelayInfo {
                broadcast_max: None,
                ..good()
            },
            RelayInfo {
                uses: vec![RelayUse::Images, RelayUse::Files],
                ..good()
            },
            old(),
        ];
        for relay in &relays {
            assert_eq!(check(relay), None);
            assert_eq!(evaluate(relay, P2pMode::DirectRelay, true, true), None);
        }
    }

    #[test]
    fn relay_only_names_the_relay_problem() {
        let none = RelayInfo::default();
        assert_eq!(
            evaluate(&none, P2pMode::RelayOnly, true, true),
            Some(CameraUnavailable::NoRelay)
        );
        // Every earlier-failing input at once still reports the first reason.
        assert_eq!(
            evaluate(&none, P2pMode::RelayOnly, false, true),
            Some(CameraUnavailable::NoRelay)
        );
        for status in [
            RelayStatus::Unavailable(RelayUnavailable::RelayDown),
            RelayStatus::Untrusted {
                kept_blocked: false,
            },
            RelayStatus::Conflict,
        ] {
            let relay = RelayInfo { status, ..good() };
            assert_eq!(
                evaluate(&relay, P2pMode::RelayOnly, true, true),
                Some(CameraUnavailable::NoRelay)
            );
        }
    }

    #[test]
    fn a_relay_without_broadcasts_is_too_old_whatever_u_says() {
        let relay = RelayInfo {
            broadcast_max: None,
            ..good()
        };
        assert_eq!(
            evaluate(&relay, P2pMode::RelayOnly, true, true),
            Some(CameraUnavailable::RelayTooOld)
        );
        let old_bot = RelayInfo {
            broadcast_max: None,
            uses: Vec::new(),
            ..good()
        };
        assert_eq!(
            evaluate(&old_bot, P2pMode::RelayOnly, true, true),
            Some(CameraUnavailable::RelayTooOld)
        );
    }

    #[test]
    fn a_relay_without_cam_in_u_does_not_allow_cameras() {
        let relay = RelayInfo {
            uses: vec![RelayUse::Images, RelayUse::Files, RelayUse::ScreenShare],
            ..good()
        };
        assert_eq!(
            evaluate(&relay, P2pMode::RelayOnly, true, true),
            Some(CameraUnavailable::RelayDisallows)
        );
    }

    #[test]
    fn an_old_relay_still_serves_relay_only() {
        // The old-format broadcast serves 0.8.x viewers as today (FR-8.94).
        assert_eq!(evaluate(&old(), P2pMode::RelayOnly, true, true), None);
    }

    #[test]
    fn the_mode_off_is_the_fourth_reason() {
        assert_eq!(
            evaluate(&good(), P2pMode::Off, true, true),
            Some(CameraUnavailable::NeedsRelayMode)
        );
        // A relay problem does not hide the mode's own reason.
        assert_eq!(
            evaluate(&RelayInfo::default(), P2pMode::Off, true, true),
            Some(CameraUnavailable::NeedsRelayMode)
        );
    }

    #[test]
    fn a_missing_certificate_is_the_last_reason() {
        for mode in [
            P2pMode::DirectRelay,
            P2pMode::RelayFirst,
            P2pMode::DirectOnly,
            P2pMode::RelayOnly,
        ] {
            assert_eq!(
                evaluate(&good(), mode, false, true),
                Some(CameraUnavailable::NoCertificate),
                "{mode:?}"
            );
        }
        assert_eq!(
            evaluate(&good(), P2pMode::Off, false, true),
            Some(CameraUnavailable::NeedsRelayMode)
        );
    }

    #[test]
    fn the_consent_names_the_reason_when_the_direct_path_is_the_only_one() {
        // `direct_only` with direct connections turned off leaves nothing (FR-4.3, FR-8.4).
        assert_eq!(
            evaluate(&good(), P2pMode::DirectOnly, true, false),
            Some(CameraUnavailable::NeedsDirect)
        );
        // The same holds for a mode that falls back to direct while no relay can carry a camera.
        assert_eq!(
            evaluate(&RelayInfo::default(), P2pMode::RelayFirst, true, false),
            Some(CameraUnavailable::NeedsDirect)
        );
        // A missing certificate is the earlier reason, and the relay path is unaffected.
        assert_eq!(
            evaluate(&good(), P2pMode::DirectOnly, false, false),
            Some(CameraUnavailable::NoCertificate)
        );
        assert_eq!(evaluate(&good(), P2pMode::RelayFirst, true, false), None);
        assert_eq!(evaluate(&good(), P2pMode::RelayOnly, true, false), None);
        assert_eq!(evaluate(&old(), P2pMode::RelayOnly, true, false), None);
    }

    /// The paths as `(relay, direct, media)`.
    fn paths(relay: &RelayInfo, mode: P2pMode) -> Option<(bool, bool, bool)> {
        Paths::of(relay, mode, true).map(|p| (p.relay, p.direct, p.media))
    }

    #[test]
    fn a_new_relay_is_the_broadcast_path() {
        // Relay first, and the broadcast carries the media profile (FR-8.94).
        for mode in [
            P2pMode::DirectRelay,
            P2pMode::RelayFirst,
            P2pMode::RelayOnly,
        ] {
            assert_eq!(
                paths(&good(), mode),
                Some((true, mode != P2pMode::RelayOnly, true))
            );
        }
        // `direct_only` never opens a relay broadcast.
        assert_eq!(
            paths(&good(), P2pMode::DirectOnly),
            Some((false, true, false))
        );
    }

    #[test]
    fn an_old_relay_keeps_the_broadcast_for_old_viewers_and_adds_the_direct_path() {
        // The old-format broadcast serves 0.8.x viewers while 0.9.0 viewers go direct (FR-8.95).
        assert_eq!(
            paths(&old(), P2pMode::RelayFirst),
            Some((true, true, false))
        );
        // `relay_only` never opens a direct stream, so only 0.8.x viewers can be served.
        assert_eq!(
            paths(&old(), P2pMode::RelayOnly),
            Some((true, false, false))
        );
        assert_eq!(
            paths(&old(), P2pMode::DirectOnly),
            Some((false, true, false))
        );
    }

    #[test]
    fn without_a_usable_relay_only_the_direct_path_is_left() {
        let none = RelayInfo::default();
        assert_eq!(
            paths(&none, P2pMode::RelayFirst),
            Some((false, true, false))
        );
        assert_eq!(
            paths(&none, P2pMode::DirectOnly),
            Some((false, true, false))
        );
        // `relay_only` and `off` leave no path at all (FR-8.4).
        assert_eq!(paths(&none, P2pMode::RelayOnly), None);
        assert_eq!(paths(&good(), P2pMode::Off), None);
        // Turned-off direct connections take the direct path away and leave the relay broadcast.
        assert_eq!(
            Paths::of(&good(), P2pMode::RelayFirst, false).map(|p| (p.relay, p.direct, p.media)),
            Some((true, false, true))
        );
        assert_eq!(Paths::of(&good(), P2pMode::DirectOnly, false), None);
        assert_eq!(
            Paths::of(&RelayInfo::default(), P2pMode::RelayFirst, false),
            None
        );
    }
}
