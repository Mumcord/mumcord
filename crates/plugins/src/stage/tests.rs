#![allow(clippy::unwrap_used)]

use super::*;
use crate::messaging::announce::AnnounceEntry;

const SERVER: ServerId = 1;
const HERE: Place = Place {
    server: SERVER,
    channel: 5,
};
const ID: &str = "0123456789abcdef";
const ID2: &str = "1123456789abcdef";

fn entry(id: &str, source: SourceTag) -> AnnounceEntry {
    AnnounceEntry {
        id: id.into(),
        api_major: 1,
        source,
    }
}

/// One person in the channel as another host sees them.
#[derive(Clone)]
struct Person {
    session: u32,
    friend: bool,
    capable: bool,
    plugins: Vec<AnnounceEntry>,
}

fn person(session: u32, plugins: &[(&str, SourceTag)]) -> Person {
    Person {
        session,
        friend: false,
        capable: true,
        plugins: plugins.iter().map(|(id, s)| entry(id, *s)).collect(),
    }
}

/// The channel from one user's view.
struct View {
    me: u32,
    others: Vec<Person>,
    capable: BTreeSet<u32>,
}

impl View {
    fn new(me: u32, others: Vec<Person>) -> Self {
        let capable = others
            .iter()
            .filter(|p| p.capable)
            .map(|p| p.session)
            .collect();
        Self {
            me,
            others,
            capable,
        }
    }

    fn members(&self) -> Vec<Member<'_>> {
        self.others
            .iter()
            .map(|p| Member {
                session: p.session,
                is_friend: p.friend,
                plugins: &p.plugins,
            })
            .collect()
    }
}

fn room<'a>(view: &'a View, members: &'a [Member<'a>]) -> Room<'a> {
    Room {
        place: HERE,
        me: view.me,
        members,
        capable: &view.capable,
    }
}

fn plugin(id: &str, source: SourceTag, visibility: Visibility) -> StagePlugin<'_> {
    StagePlugin {
        plugin: PluginRef {
            id,
            source,
            visibility,
        },
        name: "Board Brawl",
        max_participants: 2,
    }
}

fn brawl() -> StagePlugin<'static> {
    plugin("brawl", SourceTag::Official, Visibility::Everyone)
}

fn stages_at(me: u32) -> Stages {
    let mut stages = Stages::default();
    stages.moved(Some(HERE), Some(me), false);
    stages.take_changed();
    stages
}

/// Starts `ID` of `local` as `view.me`.
fn start(
    stages: &mut Stages,
    view: &View,
    local: &StagePlugin<'_>,
    id: &str,
) -> Result<Vec<Effect>, StartError> {
    let members = view.members();
    stages.start(&room(view, &members), local, id.into(), "Game".into(), None)
}

fn sends(effects: &[Effect]) -> Vec<(Vec<u32>, &Frame)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Send {
                receivers, frame, ..
            } => Some((receivers.clone(), frame)),
            _ => None,
        })
        .collect()
}

fn delivered(effects: &[Effect]) -> Vec<(&str, StageEvent)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Deliver { plugin, event } => Some((plugin.as_str(), event.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn start_announces_to_the_same_plugin_and_source_and_joins_the_starter() {
    let view = View::new(
        1,
        vec![
            person(2, &[("brawl", SourceTag::Official)]),
            person(3, &[("brawl", SourceTag::Dev)]),
            person(4, &[("other", SourceTag::Official)]),
        ],
    );
    let mut stages = stages_at(1);
    let effects = start(&mut stages, &view, &brawl(), ID).unwrap();
    let sent = sends(&effects);
    assert!(matches!(sent[0], (ref to, Frame::Start { .. }) if to == &vec![2]));
    // Official plugin: 4 has no brawl at all and gets the Get-plugin notice; 3 has it from
    // another source and gets nothing.
    assert!(matches!(sent[1], (ref to, Frame::Notice { .. }) if to == &vec![4]));
    assert_eq!(sent.len(), 2);
    assert_eq!(delivered(&effects), vec![("brawl", StageEvent::Joined(1))]);
    assert!(stages.take_changed());
    let snap = stages.snapshot();
    assert_eq!(snap.len(), 1);
    assert!(snap[0].joined && snap[0].started_by_me);
    assert_eq!(snap[0].participants, vec![1]);
}

#[test]
fn unofficial_plugins_send_no_notice() {
    let view = View::new(1, vec![person(4, &[])]);
    let mut stages = stages_at(1);
    let local = plugin("brawl", SourceTag::Repo([1, 2, 3, 4]), Visibility::Everyone);
    let effects = start(&mut stages, &view, &local, ID).unwrap();
    assert!(sends(&effects).is_empty());
}

#[test]
fn hidden_and_friends_only_plugins_respect_visibility() {
    let mut friend = person(2, &[("brawl", SourceTag::Official)]);
    friend.friend = true;
    let stranger = person(3, &[("brawl", SourceTag::Official)]);
    let lacking_stranger = person(4, &[]);
    let view = View::new(1, vec![friend, stranger, lacking_stranger]);

    let mut stages = stages_at(1);
    let friends = plugin("brawl", SourceTag::Official, Visibility::Friends);
    let sent = start(&mut stages, &view, &friends, ID).unwrap();
    assert_eq!(sends(&sent).len(), 1);
    assert_eq!(sends(&sent)[0].0, vec![2]);

    let mut stages = stages_at(1);
    let nobody = plugin("brawl", SourceTag::Official, Visibility::Nobody);
    let sent = start(&mut stages, &view, &nobody, ID).unwrap();
    assert!(sends(&sent).is_empty());
}

#[test]
fn blocked_users_are_not_members_and_get_nothing() {
    // The engine removes blocked users from the members before the stage sees them.
    let view = View::new(1, vec![]);
    let mut stages = stages_at(1);
    let effects = start(&mut stages, &view, &brawl(), ID).unwrap();
    assert!(sends(&effects).is_empty());
    // A frame from someone who is not a member (blocked or elsewhere) is ignored.
    let members = view.members();
    let effects = stages.receive(
        &room(&view, &members),
        "brawl",
        Some(&brawl()),
        9,
        Frame::Join { id: ID.into() },
    );
    assert!(effects.is_empty());
    assert_eq!(stages.snapshot()[0].participants, vec![1]);
}

#[test]
fn one_activity_per_plugin_and_three_per_channel() {
    let view = View::new(1, vec![]);
    let mut stages = stages_at(1);
    start(&mut stages, &view, &brawl(), ID).unwrap();
    assert_eq!(
        start(&mut stages, &view, &brawl(), ID2),
        Err(StartError::TooMany)
    );
    let b = plugin("b", SourceTag::Official, Visibility::Everyone);
    let c = plugin("c", SourceTag::Official, Visibility::Everyone);
    let d = plugin("d", SourceTag::Official, Visibility::Everyone);
    start(&mut stages, &view, &b, "2123456789abcdef").unwrap();
    start(&mut stages, &view, &c, "3123456789abcdef").unwrap();
    assert_eq!(
        start(&mut stages, &view, &d, "4123456789abcdef"),
        Err(StartError::TooMany)
    );
}

#[test]
fn someone_elses_activity_of_the_plugin_blocks_a_second_start() {
    let view = View::new(1, vec![person(2, &[("brawl", SourceTag::Official)])]);
    let mut stages = stages_at(1);
    let members = view.members();
    let info = Announced {
        title: "Game".into(),
        max: 4,
        web: None,
    };
    let effects = stages.receive(
        &room(&view, &members),
        "brawl",
        Some(&brawl()),
        2,
        Frame::Start {
            id: ID.into(),
            info,
        },
    );
    assert_eq!(
        delivered(&effects),
        vec![(
            "brawl",
            StageEvent::Started {
                starter: 2,
                activity: ID.into()
            }
        )]
    );
    assert_eq!(
        start(&mut stages, &view, &brawl(), ID2),
        Err(StartError::TooMany)
    );
}

#[test]
fn start_needs_a_channel() {
    let view = View::new(1, vec![]);
    let mut stages = Stages::default();
    assert_eq!(
        start(&mut stages, &view, &brawl(), ID),
        Err(StartError::NoChannel)
    );
}

#[test]
fn frames_from_another_plugin_or_source_or_hidden_plugin_are_ignored() {
    let start_frame = || Frame::Start {
        id: ID.into(),
        info: Announced {
            title: "Game".into(),
            max: 4,
            web: None,
        },
    };
    // 2 runs brawl from a third-party repository; ours is official.
    let view = View::new(1, vec![person(2, &[("brawl", SourceTag::Repo([9; 4]))])]);
    let members = view.members();
    let mut stages = stages_at(1);
    let r = room(&view, &members);
    assert!(
        stages
            .receive(&r, "brawl", Some(&brawl()), 2, start_frame())
            .is_empty()
    );
    // A plugin the user does not have: no activity (and no notice without a notice frame).
    assert!(
        stages
            .receive(&r, "brawl", None, 2, start_frame())
            .is_empty()
    );
    // Our copy is hidden from everyone.
    let view = View::new(1, vec![person(2, &[("brawl", SourceTag::Official)])]);
    let members = view.members();
    let r = room(&view, &members);
    let hidden = plugin("brawl", SourceTag::Official, Visibility::Nobody);
    assert!(
        stages
            .receive(&r, "brawl", Some(&hidden), 2, start_frame())
            .is_empty()
    );
    assert!(stages.snapshot().is_empty());
}

#[test]
fn notices_show_only_for_users_without_the_plugin() {
    let view = View::new(1, vec![person(2, &[("brawl", SourceTag::Official)])]);
    let members = view.members();
    let r = room(&view, &members);
    let notice = || Frame::Notice {
        id: ID.into(),
        title: "Game".into(),
        name: "Board Brawl".into(),
    };
    let mut stages = stages_at(1);
    stages.receive(&r, "brawl", Some(&brawl()), 2, notice());
    assert!(stages.snapshot().is_empty());
    stages.receive(&r, "brawl", None, 2, notice());
    let snap = stages.snapshot();
    assert_eq!(snap[0].kind, ActivityKind::GetPlugin);
    assert_eq!(snap[0].plugin_name, "Board Brawl");
    // Only the starter ends it.
    stages.receive(&r, "brawl", None, 3, Frame::NoticeEnd { id: ID.into() });
    assert_eq!(stages.snapshot().len(), 1);
    stages.receive(&r, "brawl", None, 2, Frame::NoticeEnd { id: ID.into() });
    assert!(stages.snapshot().is_empty());
}

#[test]
fn joining_a_full_activity_is_refused() {
    let view = View::new(
        3,
        vec![
            person(1, &[("brawl", SourceTag::Official)]),
            person(2, &[("brawl", SourceTag::Official)]),
        ],
    );
    let members = view.members();
    let r = room(&view, &members);
    let mut stages = stages_at(3);
    let info = Announced {
        title: "Game".into(),
        max: 2,
        web: None,
    };
    stages.receive(
        &r,
        "brawl",
        Some(&brawl()),
        1,
        Frame::Start {
            id: ID.into(),
            info,
        },
    );
    stages.receive(
        &r,
        "brawl",
        Some(&brawl()),
        2,
        Frame::Join { id: ID.into() },
    );
    assert_eq!(stages.join(Some(&r), &brawl(), ID), Err(JoinError::Full));
    assert_eq!(stages.join(Some(&r), &brawl(), ID2), Err(JoinError::Gone));
    // A third remote join beyond the limit is not taken on either.
    stages.receive(
        &r,
        "brawl",
        Some(&brawl()),
        3,
        Frame::Join { id: ID.into() },
    );
    assert_eq!(stages.snapshot()[0].participants, vec![1, 2]);
}

#[test]
fn joining_opens_web_content_and_tells_the_others() {
    let view = View::new(2, vec![person(1, &[("brawl", SourceTag::Official)])]);
    let members = view.members();
    let r = room(&view, &members);
    let mut stages = stages_at(2);
    let info = Announced {
        title: "Game".into(),
        max: 4,
        web: Some("board/index.html".into()),
    };
    stages.receive(
        &r,
        "brawl",
        Some(&brawl()),
        1,
        Frame::Start {
            id: ID.into(),
            info,
        },
    );
    let effects = stages.join(Some(&r), &brawl(), ID).unwrap();
    assert!(matches!(sends(&effects)[0], (ref to, Frame::Join { .. }) if to == &vec![1]));
    assert!(effects.contains(&Effect::ShowWeb {
        plugin: "brawl".into(),
        entry: "board/index.html".into()
    }));
    assert_eq!(delivered(&effects), vec![("brawl", StageEvent::Joined(2))]);
    assert!(!stages.shows_tree("brawl", 2));
}

#[test]
fn the_starter_leaving_does_not_end_it_but_the_last_one_does() {
    let view = View::new(2, vec![person(1, &[("brawl", SourceTag::Official)])]);
    let members = view.members();
    let r = room(&view, &members);
    let mut stages = stages_at(2);
    let info = Announced {
        title: "Game".into(),
        max: 4,
        web: None,
    };
    stages.receive(
        &r,
        "brawl",
        Some(&brawl()),
        1,
        Frame::Start {
            id: ID.into(),
            info,
        },
    );
    stages.join(Some(&r), &brawl(), ID).unwrap();
    let effects = stages.receive(
        &r,
        "brawl",
        Some(&brawl()),
        1,
        Frame::Leave { id: ID.into() },
    );
    assert_eq!(delivered(&effects), vec![("brawl", StageEvent::Left(1))]);
    assert_eq!(stages.snapshot()[0].participants, vec![2]);
    // Only the starter may end it; it already left.
    let effects = stages.end(Some(&r), &brawl().plugin);
    assert!(effects.is_empty());
    let effects = stages.leave(Some(&r), &brawl().plugin);
    assert_eq!(
        delivered(&effects),
        vec![
            ("brawl", StageEvent::Left(2)),
            ("brawl", StageEvent::Ended(ID.into()))
        ]
    );
    assert!(effects.contains(&Effect::CloseStage {
        plugin: "brawl".into()
    }));
    assert!(matches!(sends(&effects)[0], (ref to, Frame::Leave { .. }) if to == &vec![1]));
    assert!(stages.snapshot().is_empty());
}

#[test]
fn end_by_the_starter_ends_it_for_everyone() {
    let view = View::new(
        1,
        vec![person(2, &[("brawl", SourceTag::Official)]), person(4, &[])],
    );
    let members = view.members();
    let r = room(&view, &members);
    let mut stages = stages_at(1);
    start(&mut stages, &view, &brawl(), ID).unwrap();
    let effects = stages.end(Some(&r), &brawl().plugin);
    let sent = sends(&effects);
    assert!(matches!(sent[0], (ref to, Frame::End { .. }) if to == &vec![2]));
    assert!(matches!(sent[1], (ref to, Frame::NoticeEnd { .. }) if to == &vec![4]));
    assert_eq!(
        delivered(&effects),
        vec![("brawl", StageEvent::Ended(ID.into()))]
    );
    // An end from someone who is not the starter is ignored on the other side.
    let view2 = View::new(
        2,
        vec![
            person(1, &[("brawl", SourceTag::Official)]),
            person(3, &[("brawl", SourceTag::Official)]),
        ],
    );
    let members2 = view2.members();
    let r2 = room(&view2, &members2);
    let mut other = stages_at(2);
    let info = Announced {
        title: "Game".into(),
        max: 4,
        web: None,
    };
    other.receive(
        &r2,
        "brawl",
        Some(&brawl()),
        1,
        Frame::Start {
            id: ID.into(),
            info,
        },
    );
    assert!(
        other
            .receive(
                &r2,
                "brawl",
                Some(&brawl()),
                3,
                Frame::End { id: ID.into() }
            )
            .is_empty()
    );
    let effects = other.receive(
        &r2,
        "brawl",
        Some(&brawl()),
        1,
        Frame::End { id: ID.into() },
    );
    assert_eq!(
        delivered(&effects),
        vec![("brawl", StageEvent::Ended(ID.into()))]
    );
}

#[test]
fn a_participant_leaving_the_channel_leaves_and_may_end_the_activity() {
    let view = View::new(
        3,
        vec![
            person(1, &[("brawl", SourceTag::Official)]),
            person(2, &[("brawl", SourceTag::Official)]),
        ],
    );
    let members = view.members();
    let r = room(&view, &members);
    let mut stages = stages_at(3);
    let info = Announced {
        title: "Game".into(),
        max: 4,
        web: None,
    };
    stages.receive(
        &r,
        "brawl",
        Some(&brawl()),
        1,
        Frame::Start {
            id: ID.into(),
            info,
        },
    );
    stages.receive(
        &r,
        "brawl",
        Some(&brawl()),
        2,
        Frame::Join { id: ID.into() },
    );
    // The user is not in it: no stage-left, only the count changes.
    assert!(delivered(&stages.peer_left(&r, 1)).is_empty());
    let effects = stages.peer_left(&r, 2);
    assert_eq!(
        delivered(&effects),
        vec![("brawl", StageEvent::Ended(ID.into()))]
    );
    assert!(stages.snapshot().is_empty());
}

#[test]
fn moving_leaves_every_activity() {
    let view = View::new(1, vec![person(2, &[("brawl", SourceTag::Official)])]);
    let mut stages = stages_at(1);
    start(&mut stages, &view, &brawl(), ID).unwrap();
    // A channel change or disconnect: no frames, the others see the user go.
    let effects = stages.moved(
        Some(Place {
            server: SERVER,
            channel: 6,
        }),
        Some(1),
        false,
    );
    assert!(sends(&effects).is_empty());
    assert_eq!(
        delivered(&effects),
        vec![
            ("brawl", StageEvent::Left(1)),
            ("brawl", StageEvent::Ended(ID.into()))
        ]
    );
    assert!(stages.snapshot().is_empty());

    // A server switch tells the old channel.
    let mut stages = stages_at(1);
    start(&mut stages, &view, &brawl(), ID).unwrap();
    let effects = stages.moved(None, Some(1), true);
    assert!(matches!(sends(&effects)[0], (ref to, Frame::Leave { .. }) if to == &vec![2]));
    assert!(effects.contains(&Effect::CloseStage {
        plugin: "brawl".into()
    }));
}

#[test]
fn latecomers_get_the_state_from_the_lowest_participant_once() {
    let mut view = View::new(1, vec![person(5, &[("brawl", SourceTag::Official)])]);
    let mut stages = stages_at(1);
    start(&mut stages, &view, &brawl(), ID).unwrap();
    view.others
        .push(person(7, &[("brawl", SourceTag::Official)]));
    view.others.push(person(8, &[]));
    view.capable = [5, 7, 8].into();
    let members = view.members();
    let r = room(&view, &members);
    let locals = [brawl()];
    let effects = stages.member_announced(&r, 7, &locals);
    assert!(matches!(
        sends(&effects)[0],
        (ref to, Frame::State { participants, starter: 1, .. }) if to == &vec![7] && participants == &vec![1]
    ));
    assert!(stages.member_announced(&r, 7, &locals).is_empty());
    // Someone told at the start does not get it again.
    assert!(stages.member_announced(&r, 5, &locals).is_empty());
    // The newcomer without the plugin gets the Get-plugin notice from the starter.
    let effects = stages.member_announced(&r, 8, &locals);
    assert!(matches!(sends(&effects)[0], (ref to, Frame::Notice { .. }) if to == &vec![8]));
}

/// Two hosts exchange the frames their effects send.
#[test]
fn two_hosts_start_join_leave_and_end() {
    let a_view = View::new(1, vec![person(2, &[("brawl", SourceTag::Official)])]);
    let b_view = View::new(2, vec![person(1, &[("brawl", SourceTag::Official)])]);
    let (a_members, b_members) = (a_view.members(), b_view.members());
    let (a_room, b_room) = (room(&a_view, &a_members), room(&b_view, &b_members));
    let (mut a, mut b) = (stages_at(1), stages_at(2));

    let pass = |effects: Vec<Effect>, to: &mut Stages, r: &Room<'_>, from: u32| -> Vec<Effect> {
        effects
            .into_iter()
            .filter_map(|e| match e {
                Effect::Send { frame, plugin, .. } => {
                    let bytes = announce::encode(&frame).unwrap();
                    let (_, body) = crate::messaging::envelope::decode_host(&bytes).unwrap();
                    let frame = announce::decode(body).unwrap();
                    Some(to.receive(r, &plugin, Some(&brawl()), from, frame))
                }
                _ => None,
            })
            .flatten()
            .collect()
    };

    let started = start(&mut a, &a_view, &brawl(), ID).unwrap();
    let at_b = pass(started, &mut b, &b_room, 1);
    assert_eq!(
        delivered(&at_b),
        vec![(
            "brawl",
            StageEvent::Started {
                starter: 1,
                activity: ID.into()
            }
        )]
    );
    let joined = b.join(Some(&b_room), &brawl(), ID).unwrap();
    let at_a = pass(joined, &mut a, &a_room, 2);
    assert_eq!(delivered(&at_a), vec![("brawl", StageEvent::Joined(2))]);
    assert_eq!(a.snapshot()[0].participants, vec![1, 2]);
    assert_eq!(b.snapshot()[0].participants, vec![1, 2]);

    // The starter leaves; it keeps running for B.
    let left = a.leave(Some(&a_room), &brawl().plugin);
    let at_b = pass(left, &mut b, &b_room, 1);
    assert_eq!(delivered(&at_b), vec![("brawl", StageEvent::Left(1))]);
    assert_eq!(b.snapshot()[0].participants, vec![2]);
    assert_eq!(a.snapshot()[0].participants, vec![2]);

    // B, the last one, leaves: it ends on both sides.
    let left = b.leave(Some(&b_room), &brawl().plugin);
    assert!(delivered(&left).contains(&("brawl", StageEvent::Ended(ID.into()))));
    let at_a = pass(left, &mut a, &a_room, 2);
    assert_eq!(
        delivered(&at_a),
        vec![("brawl", StageEvent::Ended(ID.into()))]
    );
    assert!(a.snapshot().is_empty() && b.snapshot().is_empty());
}
