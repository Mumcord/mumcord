//! E2E test plugin built with the 0.6.2 SDK: shows a small panel and nothing else. It checks that
//! a plugin written for API 1.0 keeps working unchanged on the 1.1 platform.

use mumcord_plugin_sdk::{
    Plugin,
    events::Event,
    log,
    types::StartInfo,
    ui::{self, Slot},
};

struct Fixture;

impl Plugin for Fixture {
    fn start(_info: StartInfo) -> Result<(), String> {
        let tree = ui::column()
            .child(ui::heading("API 1.0"))
            .child(ui::text("A test plugin of the Mumcord E2E suite."));
        ui::show(Slot::Panel, tree).map_err(|e| format!("could not show the panel: {e}"))
    }

    fn on_event(_event: Event) {}

    fn stop() {
        log::info("stopped");
    }
}

mumcord_plugin_sdk::export!(Fixture);
