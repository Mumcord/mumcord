//! Matching a shared window to the app whose sound it should carry (FR-7.82, Linux).
//!
//! Desktop portals do not say which app owns the shared window, so the hint (the window's app
//! id or the app name the portal or compositor reports) is compared with the running PipeWire
//! playback streams. The match is clear only when exactly one running app fits; everything
//! else asks the user ("Which app's sound?", FR-7.83). Pure logic, no platform calls.

/// One running PipeWire playback stream, reduced to what matching needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoundStream {
    pub app_name: Option<String>,
    pub binary: Option<String>,
    pub desktop_id: Option<String>,
    /// The freedesktop icon name (`application.icon-name`), only for the list's icon.
    pub icon_name: Option<String>,
    pub pid: Option<u32>,
}

/// An app that matched: its streams share a process id or a binary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchedApp {
    pub app_name: Option<String>,
    pub binary: Option<String>,
    pub icon_name: Option<String>,
    /// Every process id seen on the app's streams, sorted and without duplicates.
    pub pids: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SoundMatch {
    /// Exactly one app fits: use it silently.
    Clear(MatchedApp),
    /// No hint, no app or several apps fit: show the list.
    Unclear,
}

/// Matches `hints` (window app id, reported app name) against `streams`, ignoring streams of
/// `own_pids` (Mumcord and its web view) and comparing case-insensitively.
pub fn match_sound_app(hints: &[&str], streams: &[SoundStream], own_pids: &[u32]) -> SoundMatch {
    let hints: Vec<String> = hints
        .iter()
        .map(|h| h.trim().to_lowercase())
        .filter(|h| !h.is_empty())
        .collect();
    if hints.is_empty() {
        return SoundMatch::Unclear;
    }

    let fits = |stream: &SoundStream| {
        [&stream.app_name, &stream.binary, &stream.desktop_id]
            .into_iter()
            .flatten()
            .any(|field| hints.contains(&field.trim().to_lowercase()))
    };
    let mut matching = group(streams, own_pids, fits)
        .into_iter()
        .filter(|(_, fits)| *fits)
        .map(|(app, _)| app);
    match (matching.next(), matching.next()) {
        (Some(app), None) => SoundMatch::Clear(app),
        _ => SoundMatch::Unclear,
    }
}

/// The running apps behind `streams`, without Mumcord's (`own_pids`): what the "Which app's
/// sound?" list offers (FR-7.83).
pub fn sound_apps(streams: &[SoundStream], own_pids: &[u32]) -> Vec<MatchedApp> {
    group(streams, own_pids, |_| false)
        .into_iter()
        .map(|(app, _)| app)
        .collect()
}

/// Joins `streams` into apps, each with whether one of its streams `fits`.
fn group(
    streams: &[SoundStream],
    own_pids: &[u32],
    fits: impl Fn(&SoundStream) -> bool,
) -> Vec<(MatchedApp, bool)> {
    let mut apps: Vec<(MatchedApp, bool)> = Vec::new();
    for stream in streams {
        if stream.pid.is_some_and(|pid| own_pids.contains(&pid)) {
            continue;
        }
        let fits = fits(stream);
        // A browser's several streams are one app: join by process id, else by binary.
        match apps.iter_mut().find(|(app, _)| same_app(app, stream)) {
            Some((app, matched)) => {
                *matched |= fits;
                merge(app, stream);
            }
            None => {
                let mut app = MatchedApp {
                    app_name: None,
                    binary: None,
                    icon_name: None,
                    pids: Vec::new(),
                };
                merge(&mut app, stream);
                apps.push((app, fits));
            }
        }
    }
    apps
}

fn same_app(app: &MatchedApp, stream: &SoundStream) -> bool {
    if let Some(pid) = stream.pid
        && app.pids.contains(&pid)
    {
        return true;
    }
    match (&app.binary, &stream.binary) {
        (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
        _ => false,
    }
}

fn merge(app: &mut MatchedApp, stream: &SoundStream) {
    if app.app_name.is_none() {
        app.app_name.clone_from(&stream.app_name);
    }
    if app.binary.is_none() {
        app.binary.clone_from(&stream.binary);
    }
    if app.icon_name.is_none() {
        app.icon_name.clone_from(&stream.icon_name);
    }
    if let Some(pid) = stream.pid
        && let Err(at) = app.pids.binary_search(&pid)
    {
        app.pids.insert(at, pid);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(name: &str, bin: &str, desktop: &str, pid: u32) -> SoundStream {
        let opt = |s: &str| (!s.is_empty()).then(|| s.to_owned());
        SoundStream {
            app_name: opt(name),
            binary: opt(bin),
            desktop_id: opt(desktop),
            icon_name: None,
            pid: Some(pid),
        }
    }

    fn clear_pids(m: SoundMatch) -> Vec<u32> {
        match m {
            SoundMatch::Clear(app) => app.pids,
            SoundMatch::Unclear => panic!("expected a clear match"),
        }
    }

    #[test]
    fn the_list_joins_an_apps_streams_and_leaves_mumcord_out() {
        let streams = [
            stream("Firefox", "firefox", "", 20),
            stream("", "firefox", "", 21),
            stream("Spotify", "spotify", "", 10),
            stream("Mumcord", "mumcord", "", 99),
        ];
        let apps = sound_apps(&streams, &[99]);
        let listed: Vec<_> = apps
            .iter()
            .map(|a| (a.app_name.as_deref(), a.pids.clone()))
            .collect();
        assert_eq!(
            listed,
            [(Some("Firefox"), vec![20, 21]), (Some("Spotify"), vec![10])]
        );
    }

    #[test]
    fn matches_by_app_name_binary_and_desktop_id() {
        let streams = [stream("Spotify", "spotify", "com.spotify.Client", 10)];
        for hint in ["Spotify", "spotify", "com.spotify.Client"] {
            assert_eq!(
                clear_pids(match_sound_app(&[hint], &streams, &[])),
                vec![10]
            );
        }
    }

    #[test]
    fn ignores_case_and_surrounding_space() {
        let streams = [stream("VLC media player", "vlc", "org.videolan.VLC", 4)];
        assert_eq!(
            clear_pids(match_sound_app(&[" ORG.VIDEOLAN.vlc "], &streams, &[])),
            vec![4]
        );
        assert_eq!(
            clear_pids(match_sound_app(&["VLC"], &streams, &[])),
            vec![4]
        );
    }

    #[test]
    fn partial_names_do_not_match() {
        let streams = [stream("Spotify", "spotify", "", 10)];
        assert_eq!(
            match_sound_app(&["spot"], &streams, &[]),
            SoundMatch::Unclear
        );
    }

    #[test]
    fn no_hint_is_unclear() {
        let streams = [stream("Spotify", "spotify", "", 10)];
        assert_eq!(match_sound_app(&[], &streams, &[]), SoundMatch::Unclear);
        assert_eq!(
            match_sound_app(&["", "  "], &streams, &[]),
            SoundMatch::Unclear
        );
    }

    #[test]
    fn no_match_and_no_streams_are_unclear() {
        let streams = [stream("Spotify", "spotify", "", 10)];
        assert_eq!(
            match_sound_app(&["mpv"], &streams, &[]),
            SoundMatch::Unclear
        );
        assert_eq!(match_sound_app(&["mpv"], &[], &[]), SoundMatch::Unclear);
    }

    #[test]
    fn two_apps_fitting_are_unclear() {
        let streams = [
            stream("Player", "mpv", "", 1),
            stream("Player", "vlc", "", 2),
        ];
        assert_eq!(
            match_sound_app(&["player"], &streams, &[]),
            SoundMatch::Unclear
        );
    }

    #[test]
    fn one_fitting_app_among_others_is_clear() {
        let streams = [
            stream("Spotify", "spotify", "", 1),
            stream("mpv", "mpv", "", 2),
        ];
        assert_eq!(
            clear_pids(match_sound_app(&["mpv"], &streams, &[])),
            vec![2]
        );
    }

    #[test]
    fn any_hint_may_match() {
        let streams = [stream("Spotify", "spotify", "", 1)];
        assert_eq!(
            clear_pids(match_sound_app(&["unknown", "Spotify"], &streams, &[])),
            vec![1]
        );
    }

    #[test]
    fn browser_with_several_streams_is_one_app() {
        let streams = [
            stream("Firefox", "firefox", "firefox", 50),
            stream("Firefox", "firefox", "firefox", 50),
            stream("Firefox", "firefox", "", 51),
        ];
        assert_eq!(
            clear_pids(match_sound_app(&["firefox"], &streams, &[])),
            vec![50, 51]
        );
    }

    #[test]
    fn streams_without_binary_join_by_pid() {
        let streams = [stream("Game", "", "", 7), stream("Game", "", "", 7)];
        assert_eq!(
            clear_pids(match_sound_app(&["game"], &streams, &[])),
            vec![7]
        );
    }

    #[test]
    fn own_process_is_excluded() {
        let streams = [
            stream("Mumcord", "mumcord", "", 99),
            stream("Mumcord", "mumcord", "", 100),
        ];
        assert_eq!(
            match_sound_app(&["mumcord"], &streams, &[99, 100]),
            SoundMatch::Unclear
        );
    }

    #[test]
    fn own_process_does_not_make_a_match_ambiguous() {
        let streams = [
            stream("Firefox", "firefox", "", 99),
            stream("Firefox", "firefox", "", 50),
        ];
        // The web view helper (pid 99) shares the name but is Mumcord's own.
        assert_eq!(
            clear_pids(match_sound_app(&["firefox"], &streams, &[99])),
            vec![50]
        );
    }
}
