//! E2E test plugin: shows a small panel, so the plugin gets an icon in the Plugins strip, with one
//! animated button for the reduced-motion check. It needs no logic of its own; the platform specs
//! only check what Mumcord does around a plugin.

use mumcord_plugin_sdk::{
    Plugin,
    events::Event,
    log,
    types::StartInfo,
    ui::{self, Animation, Slot},
};

struct Fixture;

impl Plugin for Fixture {
    fn start(_info: StartInfo) -> Result<(), String> {
        let tree = ui::column()
            .child(ui::heading("Fixture"))
            .child(ui::text("A test plugin of the Mumcord E2E suite."))
            .child(ui::button("wave", "Wave").animate(Animation::Wave));
        ui::show(Slot::Panel, tree).map_err(|e| format!("could not show the panel: {e}"))
    }

    fn on_event(_event: Event) {}

    fn stop() {
        log::info("stopped");
    }
}

mumcord_plugin_sdk::export!(Fixture);
