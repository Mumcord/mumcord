//! A small but complete plugin: a panel with a wave button that counts waves, stores the count
//! and tells the other users in the channel.

use mumcord_plugin_sdk::{
    Plugin,
    events::Event,
    log,
    peers::{self, Target},
    storage::{self, Scope},
    types::StartInfo,
    ui::{self, Animation, Slot},
};

const WAVE: u8 = 1;
const COUNT_KEY: &str = "waves";

struct Hello;

fn waves() -> u32 {
    storage::get_json(Scope::Identity, COUNT_KEY)
        .unwrap_or_else(|e| {
            log::warn(&format!("could not read the wave count: {e}"));
            None
        })
        .unwrap_or(0)
}

fn render(waves: u32) {
    let tree = ui::column()
        .child(ui::heading("Hello"))
        .child(ui::text(&format!("Waves: {waves}")))
        .child(ui::button("wave", "Wave").animate(Animation::Wave));
    if let Err(e) = ui::show(Slot::Panel, tree) {
        log::error(&format!("could not show the panel: {e}"));
    }
}

impl Plugin for Hello {
    fn start(_info: StartInfo) -> Result<(), String> {
        render(waves());
        Ok(())
    }

    fn on_event(event: Event) {
        match event {
            Event::UiAction(action) if action.action == "wave" => {
                let count = waves().saturating_add(1);
                if let Err(e) = storage::set_json(Scope::Identity, COUNT_KEY, &count) {
                    log::warn(&format!("could not store the wave count: {e}"));
                }
                // Nobody else in the channel is a normal case, not an error worth logging.
                let _ = peers::send_json(Target::Channel, WAVE, &());
                render(count);
            }
            Event::PeerMessage(message) if message.kind == WAVE => {
                log::info("someone waved");
            }
            _ => {}
        }
    }

    fn stop() {}
}

mumcord_plugin_sdk::export!(Hello);
