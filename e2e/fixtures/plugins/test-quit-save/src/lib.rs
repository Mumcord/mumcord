//! E2E test plugin: `stop` writes a marker to the plugin's storage. The spec ends Mumcord with
//! SIGTERM and reads the marker after the next start, so it proves that plugins get their `stop`
//! when the system ends the app (#748).

use mumcord_plugin_sdk::{
    Plugin,
    events::Event,
    log,
    storage::{self, Scope},
    types::StartInfo,
    ui::{self, Slot},
};

/// The storage key and value the spec looks for.
const KEY: &str = "stopped";
const VALUE: &[u8] = b"saved in stop";

struct Fixture;

impl Plugin for Fixture {
    fn start(_info: StartInfo) -> Result<(), String> {
        let tree = ui::column()
            .child(ui::heading("Quit Save"))
            .child(ui::text("A test plugin of the Mumcord E2E suite."));
        ui::show(Slot::Panel, tree).map_err(|e| format!("could not show the panel: {e}"))
    }

    fn on_event(_event: Event) {}

    fn stop() {
        if let Err(e) = storage::set(Scope::Global, KEY, VALUE) {
            log::warn(&format!("could not save in stop: {e:?}"));
        }
    }
}

mumcord_plugin_sdk::export!(Fixture);
