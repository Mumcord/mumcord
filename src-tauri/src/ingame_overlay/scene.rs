//! Builds the overlay [`Scene`] (who is talking in the user's channel) from the engine's
//! snapshot plus the set of sessions currently talking, which the app tracks from
//! `TalkStateChanged` events.

use std::collections::HashSet;

use mumcord_engine::store::GameOverlayEntry;
use mumcord_overlay_ipc::wire::{Person, Scene};

/// The user's channel, the people in it and who is talking, as the engine reports it now.
pub struct SceneInput<'a> {
    pub self_session: u32,
    pub channel_name: &'a str,
    pub people: Vec<ScenePerson<'a>>,
}

pub struct ScenePerson<'a> {
    pub session: u32,
    pub name: &'a str,
    pub muted: bool,
    pub deafened: bool,
}

/// Up to [`mumcord_overlay_ipc::wire::MAX_PEOPLE`] people are shown; talkers come first.
pub fn build(input: &SceneInput<'_>, talking: &HashSet<u32>, entry: &GameOverlayEntry) -> Scene {
    let mut people: Vec<Person> = input
        .people
        .iter()
        .filter_map(|p| {
            let is_talking = talking.contains(&p.session);
            // When not showing the whole channel, only talkers appear (the user included).
            if !entry.show_whole_channel && !is_talking {
                return None;
            }
            Some(Person {
                name: p.name.to_owned(),
                talking: is_talking,
                muted: p.muted,
                deafened: p.deafened,
                is_self: p.session == input.self_session,
            })
        })
        .collect();
    // Talkers first, then by the channel order already in `people`.
    people.sort_by_key(|p| !p.talking);
    Scene {
        channel: if entry.show_channel_name {
            input.channel_name.to_owned()
        } else {
            String::new()
        },
        people,
    }
}

/// Whether two scenes differ enough to resend (any field; `Scene` is `PartialEq`).
#[cfg_attr(not(test), allow(dead_code))]
pub fn changed(a: &Scene, b: &Scene) -> bool {
    a != b
}

#[cfg(test)]
mod tests {
    use super::*;
    use mumcord_engine::store::GameKey;

    fn entry(show_whole: bool, show_channel: bool) -> GameOverlayEntry {
        GameOverlayEntry {
            key: GameKey::Exe("g".into()),
            show_whole_channel: show_whole,
            show_channel_name: show_channel,
            ..GameOverlayEntry::default()
        }
    }

    fn input() -> SceneInput<'static> {
        SceneInput {
            self_session: 1,
            channel_name: "Lobby",
            people: vec![
                ScenePerson {
                    session: 1,
                    name: "Me",
                    muted: false,
                    deafened: false,
                },
                ScenePerson {
                    session: 2,
                    name: "Ana",
                    muted: false,
                    deafened: false,
                },
                ScenePerson {
                    session: 3,
                    name: "Bob",
                    muted: true,
                    deafened: false,
                },
            ],
        }
    }

    #[test]
    fn only_talkers_unless_whole_channel() {
        let talking: HashSet<u32> = [2].into_iter().collect();
        let scene = build(&input(), &talking, &entry(false, true));
        assert_eq!(scene.channel, "Lobby");
        assert_eq!(scene.people.len(), 1);
        assert_eq!(scene.people[0].name, "Ana");
        assert!(scene.people[0].talking);

        let whole = build(&input(), &talking, &entry(true, false));
        assert_eq!(whole.channel, "");
        assert_eq!(whole.people.len(), 3);
        // Ana (talking) is sorted before the silent ones.
        assert_eq!(whole.people[0].name, "Ana");
        assert!(whole.people.iter().any(|p| p.name == "Me" && p.is_self));
        assert!(whole.people.iter().any(|p| p.name == "Bob" && p.muted));
    }

    #[test]
    fn empty_when_nobody_talks() {
        let scene = build(&input(), &HashSet::new(), &entry(false, true));
        assert!(scene.people.is_empty());
        assert!(changed(
            &scene,
            &build(&input(), &[2].into_iter().collect(), &entry(false, true))
        ));
    }
}
