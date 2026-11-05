//! E2E test plugin for web content (spec 09 §10.2): pages in the stage, side panel, dialog and
//! panel popover with one probe per isolation check (AC-9.2, AC-9.3, S0), busy-loop and
//! memory-hog modes (AC-9.4), a bundled sound (AC-9.23), `net.http` calls (AC-9.7) and two
//! Apps-menu items (AC-9.17). The panel is the control surface: its buttons are user gestures, and
//! it shows every result so the E2E test can read them without entering a frame.

mod bridge;
mod messages;

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, VecDeque};

use mumcord_plugin_sdk::events::Event;
use mumcord_plugin_sdk::http::{Body, HttpRequest, Method};
use mumcord_plugin_sdk::types::{Error, StartInfo};
use mumcord_plugin_sdk::ui::{self, Slot};
use mumcord_plugin_sdk::webview::{self, Place};
use mumcord_plugin_sdk::{Plugin, app_menu, audio, http, log, stage};

use bridge::PageMessage;

const TITLE: &str = "Test Surface";
const DECLARED: &str = "https://declared.plugins.test:47443";
const UNDECLARED: &str = "https://undeclared.plugins.test:47443";
const SOUND: &str = "sounds/beep.wav";
const MAX_LOG: usize = 12;

#[derive(Default)]
struct State {
    /// `<place>/<probe>` → status reported by a page.
    results: BTreeMap<String, String>,
    /// Pending `net.http` requests and the panel action that sent them.
    requests: HashMap<u32, String>,
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

/// Records the outcome of a host call under the action's name, e.g. `play-missing: not-found`.
/// Only the WIT error name: its documentation text would make the line too long for the panel.
fn outcome<T>(action: &str, result: Result<T, Error>) {
    match result {
        Ok(_) => note(format!("{action}: ok")),
        Err(e) => note(format!("{action}: {}", e.name())),
    }
}

/// `NotFound` → `not-found`, so the panel shows the WIT error names.
fn kebab(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('-');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

fn panel() -> ui::El {
    let buttons = [
        ("start-stage", "Start stage"),
        ("open-side", "Open side panel"),
        ("open-dialog", "Open dialog"),
        ("open-popover", "Open popover"),
        ("close-web", "Close web view"),
        ("run-all", "Run all probes"),
        ("busy", "Busy loop"),
        ("hog", "Memory hog"),
        ("tone", "Frame tone without click"),
        ("play-sound", "Play sound"),
        ("play-missing", "Play undeclared sound"),
        ("http-declared", "HTTP declared"),
        ("http-undeclared", "HTTP undeclared"),
        ("http-redirect", "HTTP redirect"),
        ("item-disable", "Disable panel item"),
        ("item-enable", "Enable panel item"),
        ("bovnar-check", "Bovnar round trip"),
    ];
    let mut controls = ui::list("Controls");
    for (action, label) in buttons {
        controls = controls.child(ui::button(action, label));
    }
    STATE.with_borrow(|s| {
        let results = ui::list("Probe results").children(
            s.results
                .iter()
                .map(|(probe, status)| ui::text(&format!("{probe}: {status}"))),
        );
        let log = ui::list("Log").children(s.log.iter().map(|line| ui::text(line)));
        ui::column()
            .child(ui::heading(TITLE))
            .child(controls)
            .child(results)
            .child(log)
    })
}

fn redraw() {
    if let Err(e) = ui::show(Slot::Panel, panel()) {
        log::warn(&format!("could not draw the panel: {e:?}"));
    }
}

fn get(action: &str, url: &str) {
    let request = HttpRequest {
        method: Method::Get,
        url: url.to_owned(),
        headers: Vec::new(),
        body: Body::Empty,
    };
    match http::request(&request) {
        Ok(id) => {
            STATE.with_borrow_mut(|s| s.requests.insert(id, action.to_owned()));
        }
        Err(e) => outcome::<()>(action, Err(e)),
    }
}

fn post(action: &str, message: &str) {
    outcome(action, webview::post(message));
}

fn act(action: &str) {
    match action {
        "start-stage" => outcome(action, stage::start_web(TITLE, "stage.html")),
        "open-side" => outcome(action, webview::show(Place::SidePanel, "side.html")),
        "open-dialog" => {
            outcome("open-dialog-tree", ui::open_dialog(TITLE));
            outcome(action, webview::show(Place::Dialog, "dialog.html"));
        }
        "open-popover" => outcome(action, webview::show(Place::PanelPopover, "panel.html")),
        "close-web" => {
            webview::hide();
            note(format!("{action}: ok"));
        }
        "run-all" => post(action, "run:*:all"),
        "busy" => post(action, "busy:*"),
        "hog" => post(action, "hog:*"),
        "tone" => post(action, "tone:*"),
        "play-sound" => outcome(action, audio::play(SOUND, None)),
        "play-missing" => outcome(action, audio::play("sounds/missing.wav", None)),
        "http-declared" => get(action, &format!("{DECLARED}/")),
        "http-undeclared" => get(action, &format!("{UNDECLARED}/")),
        // The declared host answers /redirect with a redirect to the undeclared one.
        "http-redirect" => get(action, &format!("{DECLARED}/redirect")),
        "item-disable" => outcome(action, app_menu::disable("side", "Disabled by the test")),
        "item-enable" => outcome(action, app_menu::enable("side")),
        "bovnar-check" => note(format!("{action}: {}", messages::round_trip(1))),
        _ => {}
    }
}

fn on_page(message: &str) {
    match bridge::parse(message) {
        Some(PageMessage::Ready(place)) => note(format!("ready: {place}")),
        Some(PageMessage::Result {
            place,
            probe,
            status,
        }) => {
            log::info(&format!("probe {place}/{probe}: {status}"));
            STATE.with_borrow_mut(|s| s.results.insert(format!("{place}/{probe}"), status));
        }
        None => note("page: unexpected message dropped".to_owned()),
    }
}

struct TestSurface;

impl Plugin for TestSurface {
    fn start(_info: StartInfo) -> Result<(), String> {
        redraw();
        Ok(())
    }

    fn on_event(event: Event) {
        match event {
            Event::UiAction(action) => act(&action.action),
            Event::AppMenuActivated(item) => match item.as_str() {
                "start" => act("start-stage"),
                "side" => act("open-side"),
                _ => return,
            },
            Event::WebviewMessage(message) => on_page(&message),
            Event::HttpResponse(result) => {
                let action = STATE.with_borrow_mut(|s| s.requests.remove(&result.id));
                let Some(action) = action else { return };
                match result.outcome {
                    Ok(response) => note(format!("{action}: {}", response.status)),
                    Err(e) => note(format!("{action}: {}", kebab(&format!("{e:?}")))),
                }
            }
            Event::SurfaceClosed(place) => {
                note(format!("closed: {}", kebab(&format!("{place:?}"))))
            }
            Event::DialogClosed => note("closed: dialog-tree".to_owned()),
            Event::StageActivityEnded(_) => note("stage: ended".to_owned()),
            _ => return,
        }
        redraw();
    }

    fn stop() {
        audio::stop_all();
    }
}

mumcord_plugin_sdk::export!(TestSurface);
