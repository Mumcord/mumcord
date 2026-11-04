//! A complete 1.1 plugin: starts a stage activity from an Apps-menu item and shares its state
//! over the relay as a Bovnar document. The document carries the message version, and fields a
//! receiver does not know are ignored, so a later version of the plugin can add fields.

use mumcord_plugin_sdk::{
    Plugin,
    bovnar::{self, Destination, Message},
    events::Event,
    log, stage,
    types::StartInfo,
    ui::{self, El},
};
use serde::{Deserialize, Serialize};

/// The board state every player's plugin shows.
#[derive(Serialize, Deserialize)]
struct BoardState {
    text: String,
}

impl Message for BoardState {
    fn version() -> u32 {
        1
    }
}

struct StageBoard;

fn board(text: &str) -> El {
    ui::column()
        .child(ui::heading("Board"))
        .child(ui::text(text))
}

fn send_state(text: &str) {
    let state = BoardState {
        text: text.to_owned(),
    };
    if let Err(e) = bovnar::send_document(Destination::RelayChannel { fallback: true }, &state) {
        log::warn(&format!("could not send the board: {e}"));
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
                // on_document logs and drops a document that does not decode.
                let Some(received) = bovnar::on_document::<BoardState>(&data) else {
                    return;
                };
                if let Err(e) = stage::show_tree(board(&received.message.text)) {
                    log::warn(&format!("could not update the board: {e:?}"));
                }
            }
            _ => {}
        }
    }

    fn stop() {}
}

mumcord_plugin_sdk::export!(StageBoard);
