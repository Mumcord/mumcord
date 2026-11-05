//! What a Linux share's sound is made of, decided when the share starts (FR-7.81 – FR-7.83).
//!
//! A whole screen shares the desktop sound. A window shares the one app that clearly matches
//! the window's hints; without a clear match the user picks the app in "Which app's sound?"
//! and the share runs without sound until then. Both the Wayland and the X11 capture use this.
//!
//! Logs carry reason codes only, never app names, binaries or process ids (FR-7.87).

use super::linux::SoundTarget;
use super::matcher::{MatchedApp, SoundMatch, SoundStream, match_sound_app, sound_apps};
use super::{AppName, SoundApp, SoundRequest, SoundUnavailable};

/// The decision for one share's sound.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Capture this right away.
    Start(SoundTarget),
    /// Ask the user which of these apps to share.
    Choose(Vec<MatchedApp>),
    Unavailable(SoundUnavailable),
}

/// Decides the sound for `request`. `hints` are what the desktop tells about the picked window
/// (its app id, app name or binary); `streams` lists the running playback streams and is only
/// asked for a window.
pub fn decide(
    request: SoundRequest,
    hints: &[&str],
    streams: impl FnOnce() -> Result<Vec<SoundStream>, SoundUnavailable>,
    own_pids: &[u32],
) -> Decision {
    if request == SoundRequest::Display {
        return Decision::Start(SoundTarget::Desktop);
    }
    let streams = match streams() {
        Ok(streams) => streams,
        Err(reason) => return Decision::Unavailable(reason),
    };
    match match_sound_app(hints, &streams, own_pids) {
        SoundMatch::Clear(app) => Decision::Start(SoundTarget::App(app)),
        SoundMatch::Unclear => {
            tracing::info!(
                reason = "no_clear_app",
                "screen share sound needs the user's pick"
            );
            Decision::Choose(sound_apps(&streams, own_pids))
        }
    }
}

/// The name an app is shown with: its own name, else its binary.
fn name(app: &MatchedApp) -> String {
    app.app_name
        .clone()
        .or_else(|| app.binary.clone())
        .unwrap_or_default()
}

/// The answer key of `app`: its binary, which stays the same while the app opens and closes
/// streams, else its lowest process id.
fn id(app: &MatchedApp) -> String {
    match (&app.binary, app.pids.first()) {
        (Some(binary), _) => format!("bin:{binary}"),
        (None, Some(pid)) => format!("pid:{pid}"),
        (None, None) => String::new(),
    }
}

/// The list entries for `apps`, sorted by name (FR-7.83).
pub fn entries(apps: &[MatchedApp]) -> Vec<SoundApp> {
    let mut entries: Vec<SoundApp> = apps
        .iter()
        .map(|app| SoundApp {
            id: id(app),
            name: AppName::new(name(app)),
        })
        .collect();
    entries.sort_by_cached_key(|e| e.name.as_str().to_lowercase());
    entries
}

/// The app among `apps` the user's answer `id` names.
pub fn find(apps: Vec<MatchedApp>, answer: &str) -> Option<MatchedApp> {
    apps.into_iter().find(|app| id(app) == answer)
}

#[cfg(target_os = "linux")]
pub use live::{list, start_chosen, take};

#[cfg(target_os = "linux")]
mod live {
    use super::super::linux::{PipeWireSound, SoundTarget, own_pids, playback_streams};
    use super::super::matcher::sound_apps;
    use super::super::{AppName, ShareSoundMode, SoundApp, SoundOutcome, SoundRequest};
    use super::{Decision, decide, entries, find, name};
    use crate::screen_share::sound::SoundUnavailable;

    /// The share's sound for `request` with the window's `hints` (FR-7.81, FR-7.82).
    pub fn take(request: SoundRequest, hints: &[&str]) -> SoundOutcome {
        match decide(request, hints, playback_streams, &own_pids()) {
            Decision::Start(target) => start(target),
            Decision::Choose(apps) => SoundOutcome::Choose(entries(&apps)),
            Decision::Unavailable(reason) => SoundOutcome::Unavailable(reason),
        }
    }

    /// The apps playing sound now, for "Which app's sound?" and its live refresh (FR-7.83).
    pub fn list() -> Result<Vec<SoundApp>, SoundUnavailable> {
        Ok(entries(&sound_apps(&playback_streams()?, &own_pids())))
    }

    /// Captures the app the user's answer `id` names; `None` when it stopped playing meanwhile.
    pub fn start_chosen(id: &str) -> Result<Option<SoundOutcome>, SoundUnavailable> {
        let apps = sound_apps(&playback_streams()?, &own_pids());
        Ok(find(apps, id).map(|app| start(SoundTarget::App(app))))
    }

    /// Starts capturing `target`.
    fn start(target: SoundTarget) -> SoundOutcome {
        let (mode, app) = match &target {
            SoundTarget::Desktop => (ShareSoundMode::Desktop, None),
            SoundTarget::App(app) => (ShareSoundMode::App, Some(AppName::new(name(app)))),
        };
        match PipeWireSound::start(target) {
            Ok(sound) => SoundOutcome::Captured {
                source: Box::new(sound),
                mode,
                app,
            },
            Err(reason) => {
                tracing::info!(reason = reason.code(), "screen share sound not started");
                SoundOutcome::Unavailable(reason)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(name: &str, binary: &str, pid: u32) -> SoundStream {
        SoundStream {
            app_name: Some(name.to_owned()),
            binary: Some(binary.to_owned()),
            desktop_id: None,
            pid: Some(pid),
        }
    }

    fn running() -> Result<Vec<SoundStream>, SoundUnavailable> {
        Ok(vec![
            stream("Spotify", "spotify", 10),
            stream("Firefox", "firefox", 20),
            stream("Firefox", "firefox", 21),
            stream("Mumcord", "mumcord", 99),
        ])
    }

    #[test]
    fn a_whole_screen_shares_the_desktop_without_listing_streams() {
        let decision = decide(
            SoundRequest::Display,
            &[],
            || panic!("a screen share never lists streams"),
            &[],
        );
        assert_eq!(decision, Decision::Start(SoundTarget::Desktop));
    }

    #[test]
    fn a_window_with_a_clear_match_shares_that_app() {
        match decide(SoundRequest::Window, &["firefox"], running, &[99]) {
            Decision::Start(SoundTarget::App(app)) => assert_eq!(app.pids, vec![20, 21]),
            other => panic!("expected the app, got {other:?}"),
        }
    }

    #[test]
    fn a_window_without_a_clear_match_asks_with_every_app_but_mumcord() {
        for hints in [&[][..], &["vlc"], &["spotify", "firefox"]] {
            let Decision::Choose(apps) = decide(SoundRequest::Window, hints, running, &[99]) else {
                panic!("expected a choice for {hints:?}");
            };
            let names: Vec<_> = entries(&apps).into_iter().map(|e| e.name).collect();
            assert_eq!(names, [AppName::new("Firefox"), AppName::new("Spotify")]);
        }
    }

    #[test]
    fn without_pipewire_a_window_has_no_sound_with_the_reason() {
        let decision = decide(
            SoundRequest::Window,
            &["firefox"],
            || Err(SoundUnavailable::NoPipeWire),
            &[],
        );
        assert_eq!(
            decision,
            Decision::Unavailable(SoundUnavailable::NoPipeWire)
        );
    }

    #[test]
    fn an_answer_finds_its_app_again() {
        let apps = sound_apps(&running().unwrap_or_default(), &[99]);
        let listed = entries(&apps);
        let firefox = listed.iter().find(|e| e.name.as_str() == "Firefox");
        let firefox = firefox.map(|e| e.id.clone()).unwrap_or_default();
        assert_eq!(firefox, "bin:firefox");
        assert_eq!(
            find(apps.clone(), &firefox).map(|a| a.pids),
            Some(vec![20, 21])
        );
        assert_eq!(find(apps, "bin:vlc"), None, "an app that stopped playing");
    }

    #[test]
    fn an_app_without_binary_is_named_and_keyed_by_what_it_has() {
        let app = MatchedApp {
            app_name: None,
            binary: None,
            pids: vec![7, 9],
        };
        assert_eq!(id(&app), "pid:7");
        assert_eq!(name(&app), "");
    }
}
