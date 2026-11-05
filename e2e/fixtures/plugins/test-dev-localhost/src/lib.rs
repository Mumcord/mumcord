//! E2E test plugin for the Developer-mode exception (spec 09 FR-9.26, AC-9.10): it declares
//! `http://localhost:47180`, which a developer plugin may reach and a packaged plugin may not
//! declare at all (hard stop). The panel button fetches the origin and shows the outcome.

use std::cell::RefCell;

use mumcord_plugin_sdk::events::Event;
use mumcord_plugin_sdk::http::{self, Body, HttpRequest, Method};
use mumcord_plugin_sdk::types::StartInfo;
use mumcord_plugin_sdk::ui::{self, Slot};
use mumcord_plugin_sdk::{Plugin, log};

const ORIGIN: &str = "http://localhost:47180";

thread_local! {
    static RESULT: RefCell<String> = RefCell::new("not fetched".to_owned());
}

fn show(result: String) {
    log::info(&format!("fetch: {result}"));
    RESULT.set(result);
    let panel = ui::column()
        .child(ui::heading("Test Dev Localhost"))
        .child(ui::button("fetch", "Fetch localhost"))
        .child(ui::text(&format!(
            "Result: {}",
            RESULT.with_borrow(Clone::clone)
        )));
    if let Err(e) = ui::show(Slot::Panel, panel) {
        log::warn(&format!("could not draw the panel: {e:?}"));
    }
}

struct TestDevLocalhost;

impl Plugin for TestDevLocalhost {
    fn start(_info: StartInfo) -> Result<(), String> {
        show("not fetched".to_owned());
        Ok(())
    }

    fn on_event(event: Event) {
        match event {
            Event::UiAction(action) if action.action == "fetch" => {
                let request = HttpRequest {
                    method: Method::Get,
                    url: format!("{ORIGIN}/"),
                    headers: Vec::new(),
                    body: Body::Empty,
                };
                match http::request(&request) {
                    Ok(_) => show("pending".to_owned()),
                    Err(e) => show(format!("{e:?}")),
                }
            }
            Event::HttpResponse(result) => match result.outcome {
                Ok(response) => show(format!("{}", response.status)),
                Err(e) => show(format!("{e:?}")),
            },
            _ => {}
        }
    }

    fn stop() {}
}

mumcord_plugin_sdk::export!(TestDevLocalhost);
