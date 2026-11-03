//! E2E test plugin for the media area (spec 09 §10.2): a tree activity started from the Apps menu
//! (AC-9.11), the gesture rule (AC-9.13), the activity limits (AC-9.14) and leaving on a channel
//! change (AC-9.15). `test-stage-official` builds the same code under its own id. The stage and
//! the panel show an event log so the E2E test can read what the plugin received.

use std::cell::RefCell;
use std::collections::{BTreeSet, VecDeque};

use mumcord_plugin_sdk::events::Event;
use mumcord_plugin_sdk::types::StartInfo;
use mumcord_plugin_sdk::ui::{self, Slot};
use mumcord_plugin_sdk::{Plugin, log, stage, timers};

/// Each build gets its own title, so two variants side by side are told apart.
const TITLE: &str = if env!("CARGO_PKG_NAME").len() == "test-stage".len() {
    "Test Stage"
} else {
    "Test Stage Official"
};
/// Past the 8 s gesture window, so the delayed start must fail with `not-permitted`.
const LATE_START_MS: u32 = 10_000;
const MAX_LOG: usize = 12;

#[derive(Default)]
struct State {
    running: bool,
    /// The activity seen last, own or someone else's.
    activity: Option<String>,
    late_timer: Option<u32>,
    participants: BTreeSet<u32>,
    log: VecDeque<String>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn note(line: String) {
    log::info(&line);
    STATE.with_borrow_mut(|s| {
        if s.log.len() == MAX_LOG {
            s.log.pop_front();
        }
        s.log.push_back(line);
    });
}

fn outcome<T, E: std::fmt::Debug>(action: &str, result: Result<T, E>) -> Option<T> {
    match result {
        Ok(value) => {
            note(format!("{action}: ok"));
            Some(value)
        }
        Err(e) => {
            note(format!("{action}: {e:?}"));
            None
        }
    }
}

fn log_list() -> ui::El {
    STATE.with_borrow(|s| ui::list("Events").children(s.log.iter().map(|line| ui::text(line))))
}

fn board() -> ui::El {
    let participants = STATE.with_borrow(|s| s.participants.len());
    ui::column()
        .child(ui::heading(TITLE))
        .child(ui::text(&format!("Participants: {participants}")))
        .child(ui::button("ping", "Ping"))
        .child(log_list())
}

fn panel() -> ui::El {
    let buttons = [
        ("start", "Start now"),
        ("late", "Start in 10 s"),
        ("join", "Join last activity"),
        ("leave", "Leave"),
        ("end", "End"),
    ];
    let mut column = ui::column().child(ui::heading(TITLE));
    for (action, label) in buttons {
        column = column.child(ui::button(action, label));
    }
    column.child(log_list())
}

fn redraw() {
    if let Err(e) = ui::show(Slot::Panel, panel()) {
        log::warn(&format!("could not draw the panel: {e:?}"));
    }
    if STATE.with_borrow(|s| s.running)
        && let Err(e) = stage::show_tree(board())
    {
        log::warn(&format!("could not draw the stage: {e:?}"));
    }
}

fn start(action: &str) {
    if let Some(id) = outcome(action, stage::start_tree(TITLE, board())) {
        STATE.with_borrow_mut(|s| {
            s.running = true;
            s.activity = Some(id);
        });
    }
}

fn act(action: &str) {
    match action {
        "start" => start("start"),
        "late" => {
            if let Some(id) = outcome("late-timer", timers::start(LATE_START_MS, false)) {
                STATE.with_borrow_mut(|s| s.late_timer = Some(id));
            }
        }
        "join" => match STATE.with_borrow(|s| s.activity.clone()) {
            Some(id) => {
                if outcome("join", stage::join(&id)).is_some() {
                    STATE.with_borrow_mut(|s| s.running = true);
                }
            }
            None => note("join: no activity".to_owned()),
        },
        "leave" => {
            stage::leave();
            STATE.with_borrow_mut(|s| s.running = false);
            note("leave: ok".to_owned());
        }
        "end" => {
            stage::end();
            note("end: ok".to_owned());
        }
        "ping" => note("ping".to_owned()),
        _ => {}
    }
}

struct TestStage;

impl Plugin for TestStage {
    fn start(_info: StartInfo) -> Result<(), String> {
        redraw();
        Ok(())
    }

    fn on_event(event: Event) {
        match event {
            Event::UiAction(action) => act(&action.action),
            Event::AppMenuActivated(item) => {
                note(format!("app-menu: {item}"));
                act(&item);
            }
            Event::Timer(id) => {
                let late = STATE.with_borrow_mut(|s| s.late_timer.take_if(|t| *t == id));
                if late.is_some() {
                    start("late-start");
                }
            }
            Event::StageActivityStarted((peer, id)) => {
                note(format!("started: {} {id}", peer.name));
                STATE.with_borrow_mut(|s| s.activity = Some(id));
            }
            Event::StageActivityEnded(id) => {
                note(format!("ended: {id}"));
                STATE.with_borrow_mut(|s| {
                    s.participants.clear();
                    if s.activity.as_deref() == Some(id.as_str()) {
                        s.activity = None;
                        s.running = false;
                    }
                });
            }
            Event::StageJoined(peer) => {
                note(format!("joined: {}", peer.name));
                STATE.with_borrow_mut(|s| {
                    s.participants.insert(peer.session);
                    s.running = true;
                });
            }
            Event::StageLeft(peer) => {
                note(format!("left: {}", peer.name));
                STATE.with_borrow_mut(|s| {
                    s.participants.remove(&peer.session);
                    // The last one out is this client: the board can no longer be drawn.
                    s.running &= !s.participants.is_empty();
                });
            }
            Event::StageVisibilityChanged(visible) => note(format!("visible: {visible}")),
            _ => return,
        }
        redraw();
    }

    fn stop() {
        if STATE.with_borrow(|s| s.running) {
            stage::end();
        }
    }
}

mumcord_plugin_sdk::export!(TestStage);
