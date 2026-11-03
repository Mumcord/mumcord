//! A complete 1.1 plugin: starts a stage activity from an Apps-menu item and shares its state
//! over the relay. Relay messages carry a leading format byte, so a later version of the plugin
//! can change the payload and old versions drop what they do not understand.

use mumcord_plugin_sdk::{
    Plugin,
    events::Event,
    log, relay, stage,
    types::StartInfo,
    ui::{self, El},
};

/// Format byte of the relay payload; bump it when the JSON layout changes.
const FORMAT: u8 = 1;

struct StageBoard;

fn board(text: &str) -> El {
    ui::column()
        .child(ui::heading("Board"))
        .child(ui::text(text))
}

fn send_state(text: &str) {
    let mut data = vec![FORMAT];
    data.extend_from_slice(text.as_bytes());
    if let Err(e) = relay::send_channel(&data, true) {
        log::warn(&format!("could not send the board: {e:?}"));
    }
}

fn decode_state(data: &[u8]) -> Option<&str> {
    match data.split_first() {
        Some((&FORMAT, rest)) => std::str::from_utf8(rest).ok(),
        _ => None,
    }
}

impl Plugin for StageBoard {
    fn start(_info: StartInfo) -> Result<(), String> {
        Ok(())
    }

    fn on_event(event: Event) {
        match event {
            Event::AppMenuActivated(item) if item == "open-board" => {
                match stage::start_tree("Board", board("Empty")) {
                    Ok(_) => send_state("Empty"),
                    Err(e) => log::error(&format!("could not start the activity: {e:?}")),
                }
            }
            Event::RelayMessage((_peer, data)) => {
                if let Some(Err(e)) = decode_state(&data).map(|text| stage::show_tree(board(text)))
                {
                    log::warn(&format!("could not update the board: {e:?}"));
                }
            }
            _ => {}
        }
    }

    fn stop() {}
}

mumcord_plugin_sdk::export!(StageBoard);
