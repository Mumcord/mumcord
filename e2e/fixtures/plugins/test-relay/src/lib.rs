//! E2E test plugin for relay plugin data (spec 09 §10.2, AC-9.18 … AC-9.21): 64 KiB and small
//! messages to the channel or one person, with the Mumble fallback on or off. Its stage shows the
//! relay status chip of the host; the stage and the panel show the relay status, every send
//! outcome and every received message so the E2E test can read them. Malformed and oversized
//! sends cover the Bovnar error paths (AC-9.42).

mod messages;

use std::cell::RefCell;
use std::collections::VecDeque;

use mumcord_plugin_sdk::bovnar::{self, Destination};
use mumcord_plugin_sdk::events::Event;
use mumcord_plugin_sdk::types::{Session, StartInfo};
use mumcord_plugin_sdk::ui::{self, Slot};
use mumcord_plugin_sdk::{Plugin, log, relay, stage};

const TITLE: &str = "Test Relay";
const BIG: usize = 64 * 1024;
/// Fits the Mumble plugin-data fallback (at most 997 bytes).
const SMALL: usize = 900;
const MAX_LOG: usize = 12;

#[derive(Default)]
struct State {
    running: bool,
    seq: u32,
    /// The person direct messages go to: the last one who showed they run this plugin.
    peer: Option<(Session, String)>,
    received: u32,
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

fn view(with_buttons: bool) -> ui::El {
    let buttons = [
        ("channel-big", "Channel 64 KiB"),
        ("channel-big-fallback", "Channel 64 KiB, fallback"),
        ("channel-small", "Channel small"),
        ("channel-small-fallback", "Channel small, fallback"),
        ("direct-big", "Direct 64 KiB"),
        ("direct-small-fallback", "Direct small, fallback"),
        ("channel-malformed", "Channel malformed bytes"),
        ("channel-oversized", "Channel oversized document"),
        ("channel-oversized-raw", "Channel oversized bytes"),
    ];
    STATE.with_borrow(|s| {
        let mut column = ui::column()
            .child(ui::heading(TITLE))
            .child(ui::text(&format!("Relay: {:?}", relay::status())))
            .child(ui::text(&format!("Received: {}", s.received)))
            .child(ui::text(&format!(
                "Peer: {}",
                s.peer.as_ref().map_or("none", |(_, name)| name.as_str())
            )));
        if with_buttons {
            column = column.child(ui::button("start", "Start stage"));
            for (action, label) in buttons {
                column = column.child(ui::button(action, label));
            }
        }
        column.child(ui::list("Log").children(s.log.iter().map(|line| ui::text(line))))
    })
}

fn redraw() {
    if let Err(e) = ui::show(Slot::Panel, view(true)) {
        log::warn(&format!("could not draw the panel: {e:?}"));
    }
    if STATE.with_borrow(|s| s.running)
        && let Err(e) = stage::show_tree(view(false))
    {
        log::warn(&format!("could not draw the stage: {e:?}"));
    }
}

fn send(action: &str, size: usize, fallback: bool, direct: bool) {
    let (seq, peer) = STATE.with_borrow_mut(|s| {
        s.seq = s.seq.wrapping_add(1);
        (s.seq, s.peer.as_ref().map(|(session, _)| *session))
    });
    let Some(message) = messages::message(seq, size) else {
        note(format!("{action}: cannot build {size} B"));
        return;
    };
    let result = if direct {
        match peer {
            Some(peer) => {
                bovnar::send_document(Destination::RelayDirect { peer, fallback }, &message)
            }
            None => {
                note(format!("{action}: no peer"));
                return;
            }
        }
    } else {
        bovnar::send_document(Destination::RelayChannel { fallback }, &message)
    };
    match result {
        Ok(()) => note(format!("{action}: sent #{seq} {size} B")),
        // The host's own refusals by their WIT names (`relay-unavailable`).
        Err(bovnar::Error::Host(e)) => note(format!("{action}: {}", e.name())),
        Err(e) => note(format!("{action}: {e}")),
    }
}

/// Raw bytes that are not a Bovnar document: the receiver's SDK drops them, the host must still
/// deliver them unchanged.
fn send_malformed(action: &str) {
    let data = messages::malformed();
    match relay::send_channel(&data, true) {
        Ok(()) => note(format!("{action}: sent {} B", data.len())),
        Err(e) => note(format!("{action}: {}", e.name())),
    }
}

/// A document above the relay limit (AC-9.42): the SDK refuses to encode it, so nothing leaves
/// the machine.
fn send_oversized_document(action: &str) {
    let seq = STATE.with_borrow_mut(|s| {
        s.seq = s.seq.wrapping_add(1);
        s.seq
    });
    match bovnar::send_document(
        Destination::RelayChannel { fallback: false },
        &messages::oversized(seq),
    ) {
        Ok(()) => note(format!("{action}: sent #{seq}")),
        Err(bovnar::Error::Host(e)) => note(format!("{action}: {}", e.name())),
        Err(e) => note(format!("{action}: {e}")),
    }
}

/// Raw bytes one above the relay limit, past the SDK helpers: the host itself refuses them.
fn send_oversized_raw(action: &str) {
    match relay::send_channel(&vec![0; bovnar::RELAY_LIMIT + 1], false) {
        Ok(()) => note(format!("{action}: sent {} B", bovnar::RELAY_LIMIT + 1)),
        Err(e) => note(format!("{action}: {}", e.name())),
    }
}

fn act(action: &str) {
    match action {
        "start" => match stage::start_tree(TITLE, view(false)) {
            Ok(_) => {
                STATE.with_borrow_mut(|s| s.running = true);
                note("start: ok".to_owned());
            }
            Err(e) => note(format!("start: {e:?}")),
        },
        "channel-big" => send(action, BIG, false, false),
        "channel-big-fallback" => send(action, BIG, true, false),
        "channel-small" => send(action, SMALL, false, false),
        "channel-small-fallback" => send(action, SMALL, true, false),
        "direct-big" => send(action, BIG, false, true),
        "direct-small-fallback" => send(action, SMALL, true, true),
        "channel-malformed" => send_malformed(action),
        "channel-oversized" => send_oversized_document(action),
        "channel-oversized-raw" => send_oversized_raw(action),
        _ => {}
    }
}

struct TestRelay;

impl Plugin for TestRelay {
    fn start(_info: StartInfo) -> Result<(), String> {
        redraw();
        Ok(())
    }

    fn on_event(event: Event) {
        match event {
            Event::UiAction(action) => act(&action.action),
            Event::AppMenuActivated(item) => act(&item),
            Event::PeerReady(peer) => {
                note(format!("peer: {}", peer.name));
                STATE.with_borrow_mut(|s| s.peer = Some((peer.session, peer.name)));
            }
            Event::RelayMessage((peer, data)) => match messages::decode(&data) {
                Some(m) => {
                    let check = if m.intact { "intact" } else { "damaged" };
                    note(format!(
                        "received #{} {} B {check} from {}",
                        m.seq, m.size, peer.name
                    ));
                    STATE.with_borrow_mut(|s| {
                        s.received = s.received.saturating_add(1);
                        s.peer.get_or_insert((peer.session, peer.name));
                    });
                }
                // The SDK has logged the reason and size; the host passes the bytes unchanged.
                None if data == messages::malformed() => note(format!(
                    "received malformed {} B dropped, bytes unchanged from {}",
                    data.len(),
                    peer.name
                )),
                None => note(format!(
                    "received {} B unknown from {}",
                    data.len(),
                    peer.name
                )),
            },
            Event::RelayStatusChanged(status) => note(format!("relay: {status:?}")),
            Event::StageActivityEnded(_) => {
                STATE.with_borrow_mut(|s| s.running = false);
                note("stage: ended".to_owned());
            }
            // Also sent when the host joins someone else's activity for this client.
            Event::StageJoined(peer) => {
                STATE.with_borrow_mut(|s| s.running = true);
                note(format!("joined: {}", peer.name));
            }
            Event::StageLeft(peer) => note(format!("left: {}", peer.name)),
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

mumcord_plugin_sdk::export!(TestRelay);
