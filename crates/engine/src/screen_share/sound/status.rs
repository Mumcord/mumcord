//! What the own share's sound carries and why it may carry nothing (FR-7.76 … FR-7.88).
//!
//! The system picker decides the sound: a whole screen shares the desktop sound, one window (or
//! a macOS app) only that app's sound. The engine asks the capture for it once at share start
//! with a [`SoundRequest`]; the capture answers with a [`SoundOutcome`]. The resulting
//! [`SoundStatus`] is what the sharer's UI shows. Logs only ever see the mode and a reason code
//! (FR-7.87): app names live in [`AppName`], whose `Debug` hides them, and have no `Display`.

use super::super::capture::{CaptureSource, SoundSource};
use super::super::signal::ShareKind;

/// What the share's sound is (FR-7.76).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShareSoundMode {
    /// Everything the computer plays, minus Mumcord (whole-screen share).
    Desktop,
    /// Only the sound of one app (window share, macOS app share).
    App,
    /// No sound: switched off, unavailable, or no app chosen.
    #[default]
    None,
}

impl ShareSoundMode {
    /// The code logs carry (FR-7.87).
    pub fn code(self) -> &'static str {
        match self {
            Self::Desktop => "desktop",
            Self::App => "app",
            Self::None => "none",
        }
    }
}

/// Why a share has no sound although the switch is on (FR-7.84).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundUnavailable {
    /// Below Windows 10 version 2004 or macOS 13.
    OsTooOld,
    /// Linux without a running PipeWire.
    NoPipeWire,
    /// The audio capture failed at share start or during the share (device lost, permission
    /// revoked); the video share goes on.
    CaptureFailed,
}

impl SoundUnavailable {
    /// The code logs carry (FR-7.87).
    pub fn code(self) -> &'static str {
        match self {
            Self::OsTooOld => "os_too_old",
            Self::NoPipeWire => "no_pipewire",
            Self::CaptureFailed => "capture_failed",
        }
    }
}

/// Why the own share sends no sound, as its live bar names it (FR-7.85).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoSoundReason {
    /// The Sound switch is off.
    Off,
    /// "No sound" (or Cancel) was the answer to "Which app's sound?" (FR-7.83).
    NotChosen,
    /// Sound cannot be shared (FR-7.84).
    Unavailable(SoundUnavailable),
}

/// The name of the app whose sound is shared, for the sharer's own live bar (FR-7.85). It stays
/// on this device (FR-7.87): `Debug` hides it and there is no `Display`, so it cannot slip into
/// a log line by accident.
#[derive(Clone, PartialEq, Eq)]
pub struct AppName(String);

impl AppName {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for AppName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AppName(..)")
    }
}

/// What the engine asks the capture for, from what the user picked (FR-7.76).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundRequest {
    /// A whole screen: the desktop sound without Mumcord.
    Display,
    /// One window or one app: only that app's sound.
    Window,
}

impl SoundRequest {
    /// The request for a pick. A pick the platform cannot classify counts as a window: that never
    /// shares more than one app's sound (FR-7.87).
    pub fn for_kind(kind: Option<ShareKind>) -> Self {
        match kind {
            Some(ShareKind::Screen) => Self::Display,
            Some(ShareKind::Window) | None => Self::Window,
        }
    }

    /// The mode a capture that honours this request delivers.
    pub fn mode(self) -> ShareSoundMode {
        match self {
            Self::Display => ShareSoundMode::Desktop,
            Self::Window => ShareSoundMode::App,
        }
    }
}

/// The capture's answer to a [`SoundRequest`].
pub enum SoundOutcome {
    /// Sound to share.
    Captured {
        source: Box<dyn SoundSource>,
        /// [`ShareSoundMode::Desktop`] or [`ShareSoundMode::App`].
        mode: ShareSoundMode,
        /// The shared app, where the capture knows it.
        app: Option<AppName>,
    },
    /// Nothing to share, and nothing went wrong: no single app behind the picked window, or
    /// "No sound" chosen.
    NoSound,
    /// The picked window has no clear app (Linux, FR-7.82): the user picks one of these in
    /// "Which app's sound?" (FR-7.83); the share starts without sound meanwhile.
    Choose(Vec<SoundApp>),
    /// Sound cannot be shared.
    Unavailable(SoundUnavailable),
}

/// The own share's sound as the sharer sees it (FR-7.84, FR-7.85).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SoundStatus {
    /// What the capture delivers, whether or not the switch is on.
    pub mode: ShareSoundMode,
    /// The shared app, only with [`ShareSoundMode::App`] and only where known.
    pub app: Option<AppName>,
    /// Why there is no sound; `None` while there is some or none was asked for.
    pub unavailable: Option<SoundUnavailable>,
    /// The apps offered in "Which app's sound?" while the user has not answered it yet
    /// (Linux, FR-7.83); `None` when nothing is asked.
    pub choice: Option<Vec<SoundApp>>,
    /// The Sound switch is off. Kept apart from the share's `sound`, which is also off when
    /// there is no source, so the live bar can tell the switch from a missing source (FR-7.85).
    pub off: bool,
    /// "No sound" was the answer to "Which app's sound?" (FR-7.83).
    pub declined: bool,
}

/// One app offered in "Which app's sound?" (FR-7.83). Stays on the sharer's device (FR-7.87).
#[derive(Clone, PartialEq, Eq)]
pub struct SoundApp {
    /// What the answer names; stays the same while the app keeps playing.
    pub id: String,
    pub name: AppName,
    /// The app's own icon, where the system has one (Linux).
    pub icon: Option<AppIcon>,
}

/// An app icon file, ready to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppIcon {
    /// `image/svg+xml` or `image/png`.
    pub mime: &'static str,
    pub bytes: Vec<u8>,
}

impl std::fmt::Debug for SoundApp {
    /// The id holds the binary or a process id, which never reach a log (FR-7.87).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SoundApp(..)")
    }
}

impl SoundStatus {
    /// A source to share sound from exists.
    pub fn available(&self) -> bool {
        self.mode != ShareSoundMode::None
    }

    /// Why no sound is sent; `None` while sound is sent or nothing explains its absence (e.g.
    /// no single app behind the picked window, or the question still open).
    pub fn no_sound_reason(&self) -> Option<NoSoundReason> {
        if let Some(reason) = self.unavailable {
            Some(NoSoundReason::Unavailable(reason))
        } else if self.off {
            Some(NoSoundReason::Off)
        } else if self.declined {
            Some(NoSoundReason::NotChosen)
        } else {
            None
        }
    }

    /// What is sent with the switch at `sound`.
    pub fn effective(&self, sound: bool) -> ShareSoundMode {
        if sound {
            self.mode
        } else {
            ShareSoundMode::None
        }
    }

    /// The status a test sound source gives (test hook): desktop sound.
    pub fn test_source() -> Self {
        Self {
            mode: ShareSoundMode::Desktop,
            ..Self::default()
        }
    }

    /// The capture failed during the share (FR-7.84): no sound any more, with the reason.
    pub fn capture_failed(&mut self) {
        *self = Self {
            unavailable: Some(SoundUnavailable::CaptureFailed),
            ..Self::default()
        };
    }

    /// Mode and reason code at `info`, never the app (FR-7.87).
    pub fn log(&self, sound: bool) {
        tracing::info!(
            mode = self.effective(sound).code(),
            reason = self.unavailable.map_or("none", SoundUnavailable::code),
            "screen share sound"
        );
    }
}

impl SoundOutcome {
    /// Splits the answer into the source to share and the status the UI shows.
    pub fn into_parts(self) -> (Option<Box<dyn SoundSource>>, SoundStatus) {
        match self {
            Self::Captured { source, mode, app } => {
                let app = if mode == ShareSoundMode::App {
                    app
                } else {
                    None
                };
                (
                    Some(source),
                    SoundStatus {
                        mode,
                        app,
                        ..SoundStatus::default()
                    },
                )
            }
            Self::NoSound => (None, SoundStatus::default()),
            Self::Choose(apps) => (
                None,
                SoundStatus {
                    choice: Some(apps),
                    ..SoundStatus::default()
                },
            ),
            Self::Unavailable(reason) => (
                None,
                SoundStatus {
                    unavailable: Some(reason),
                    ..SoundStatus::default()
                },
            ),
        }
    }
}

/// Takes the share's sound from `capture` for what the user picked, unless this system cannot
/// share sound at all (`unavailable`), in which case the capture is not asked.
pub(crate) fn take(
    capture: &mut dyn CaptureSource,
    unavailable: Option<SoundUnavailable>,
) -> (Option<Box<dyn SoundSource>>, SoundStatus) {
    let outcome = match unavailable {
        Some(reason) => SoundOutcome::Unavailable(reason),
        None => capture.take_sound(SoundRequest::for_kind(capture.kind())),
    };
    outcome.into_parts()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::super::super::capture::TestTone;
    use super::*;
    use mumcord_screenshare::I420Frame;

    #[test]
    fn a_capture_that_fails_during_the_share_leaves_no_sound_with_the_reason() {
        let mut status = SoundStatus {
            mode: ShareSoundMode::App,
            app: Some(AppName::new("Spotify")),
            ..SoundStatus::default()
        };
        status.capture_failed();
        assert!(!status.available());
        assert_eq!(status.effective(true), ShareSoundMode::None);
        assert_eq!(status.app, None);
        assert_eq!(status.unavailable, Some(SoundUnavailable::CaptureFailed));
    }

    #[test]
    fn the_reason_tells_the_switch_from_a_declined_choice() {
        let unavailable = SoundStatus {
            unavailable: Some(SoundUnavailable::NoPipeWire),
            off: true,
            ..SoundStatus::default()
        };
        assert_eq!(
            unavailable.no_sound_reason(),
            Some(NoSoundReason::Unavailable(SoundUnavailable::NoPipeWire)),
            "a missing system part wins over the switch"
        );
        let off = SoundStatus {
            mode: ShareSoundMode::Desktop,
            off: true,
            ..SoundStatus::default()
        };
        assert_eq!(off.no_sound_reason(), Some(NoSoundReason::Off));
        let declined = SoundStatus {
            declined: true,
            ..SoundStatus::default()
        };
        assert_eq!(declined.no_sound_reason(), Some(NoSoundReason::NotChosen));
        assert_eq!(SoundStatus::test_source().no_sound_reason(), None);
        assert_eq!(SoundStatus::default().no_sound_reason(), None);
    }

    #[test]
    fn the_pick_decides_the_request() {
        assert_eq!(
            SoundRequest::for_kind(Some(ShareKind::Screen)),
            SoundRequest::Display
        );
        assert_eq!(
            SoundRequest::for_kind(Some(ShareKind::Window)),
            SoundRequest::Window
        );
        assert_eq!(
            SoundRequest::for_kind(None),
            SoundRequest::Window,
            "an unknown pick never shares the desktop"
        );
        assert_eq!(SoundRequest::Display.mode(), ShareSoundMode::Desktop);
        assert_eq!(SoundRequest::Window.mode(), ShareSoundMode::App);
    }

    #[test]
    fn codes_are_stable() {
        let modes = [
            ShareSoundMode::Desktop,
            ShareSoundMode::App,
            ShareSoundMode::None,
        ];
        assert_eq!(modes.map(ShareSoundMode::code), ["desktop", "app", "none"]);
        let reasons = [
            SoundUnavailable::OsTooOld,
            SoundUnavailable::NoPipeWire,
            SoundUnavailable::CaptureFailed,
        ];
        assert_eq!(
            reasons.map(SoundUnavailable::code),
            ["os_too_old", "no_pipewire", "capture_failed"]
        );
    }

    #[test]
    fn the_app_name_never_shows_in_debug_output() {
        let status = SoundStatus {
            mode: ShareSoundMode::App,
            app: Some(AppName::new("Secret Player")),
            ..SoundStatus::default()
        };
        assert!(!format!("{status:?}").contains("Secret"));
        let asking = SoundOutcome::Choose(vec![SoundApp {
            id: "bin:secret-player".into(),
            name: AppName::new("Secret Player"),
            icon: None,
        }]);
        let (source, asking) = asking.into_parts();
        assert!(source.is_none());
        assert!(!asking.available(), "no sound until the user picks");
        assert!(!format!("{asking:?}").to_lowercase().contains("secret"));
        assert_eq!(status.app.unwrap().as_str(), "Secret Player");
    }

    #[test]
    fn the_switch_turns_the_effective_mode_off() {
        let status = SoundStatus::test_source();
        assert!(status.available());
        assert_eq!(status.effective(true), ShareSoundMode::Desktop);
        assert_eq!(status.effective(false), ShareSoundMode::None);
        assert!(!SoundStatus::default().available());
    }

    fn tone() -> Box<dyn SoundSource> {
        Box::new(TestTone::new(440.0, 0.1))
    }

    #[test]
    fn outcomes_become_status() {
        let (source, status) = SoundOutcome::Captured {
            source: tone(),
            mode: ShareSoundMode::App,
            app: Some(AppName::new("Player")),
        }
        .into_parts();
        assert!(source.is_some());
        assert_eq!(status.mode, ShareSoundMode::App);
        assert_eq!(status.app, Some(AppName::new("Player")));
        assert_eq!(status.unavailable, None);

        let (_, status) = SoundOutcome::Captured {
            source: tone(),
            mode: ShareSoundMode::Desktop,
            app: Some(AppName::new("Player")),
        }
        .into_parts();
        assert_eq!(status.app, None, "the desktop has no app name");

        let (source, status) = SoundOutcome::NoSound.into_parts();
        assert!(source.is_none());
        assert_eq!(status, SoundStatus::default());

        let (source, status) =
            SoundOutcome::Unavailable(SoundUnavailable::CaptureFailed).into_parts();
        assert!(source.is_none());
        assert_eq!(status.mode, ShareSoundMode::None);
        assert_eq!(status.unavailable, Some(SoundUnavailable::CaptureFailed));
    }

    /// A capture that answers every request with desktop sound and remembers what it was asked.
    struct Recorder {
        kind: Option<ShareKind>,
        asked: Option<SoundRequest>,
    }

    impl CaptureSource for Recorder {
        fn frame(&mut self, _: u32, _: u32) -> Option<I420Frame> {
            None
        }

        fn kind(&self) -> Option<ShareKind> {
            self.kind
        }

        fn take_sound(&mut self, request: SoundRequest) -> SoundOutcome {
            self.asked = Some(request);
            SoundOutcome::Captured {
                source: tone(),
                mode: request.mode(),
                app: None,
            }
        }
    }

    #[test]
    fn the_capture_is_asked_for_what_was_picked() {
        let mut screen = Recorder {
            kind: Some(ShareKind::Screen),
            asked: None,
        };
        let (source, status) = take(&mut screen, None);
        assert!(source.is_some());
        assert_eq!(screen.asked, Some(SoundRequest::Display));
        assert_eq!(status.mode, ShareSoundMode::Desktop);

        let mut window = Recorder {
            kind: Some(ShareKind::Window),
            asked: None,
        };
        let (_, status) = take(&mut window, None);
        assert_eq!(window.asked, Some(SoundRequest::Window));
        assert_eq!(status.mode, ShareSoundMode::App);
    }

    #[test]
    fn an_unavailable_system_does_not_ask_the_capture() {
        let mut capture = Recorder {
            kind: Some(ShareKind::Screen),
            asked: None,
        };
        let (source, status) = take(&mut capture, Some(SoundUnavailable::OsTooOld));
        assert!(source.is_none());
        assert_eq!(capture.asked, None);
        assert_eq!(status.unavailable, Some(SoundUnavailable::OsTooOld));
        assert!(!status.available());
    }
}
