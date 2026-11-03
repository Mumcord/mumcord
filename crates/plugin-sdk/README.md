# mumcord-plugin-sdk

The SDK for writing plugins for [Mumcord](https://github.com/Mumcord/mumcord), a modern voice chat client for standard Mumble servers. A plugin is a small WebAssembly component that runs sandboxed inside the client and can only use what the plugin API (`mumcord:plugin@1.0.0`) offers: its own storage, timers, a UI panel, chat, messages to other users of the same plugin and a few more interfaces, each behind a permission the user grants.

This crate contains the Rust bindings to that API (the WIT files ship with the crate) plus small helpers: a UI tree builder, JSON storage and peer messages, translations and logging.

## Getting started

Install the command-line tool and create a project:

```sh
cargo install mumcord-plugin
mumcord-plugin new my-plugin
cd my-plugin
mumcord-plugin build
```

A plugin implements `Plugin` and registers it with `export!`:

```rust,ignore
use mumcord_plugin_sdk::{events::Event, log, types::StartInfo, ui, Plugin};

struct Hello;

impl Plugin for Hello {
    fn start(_info: StartInfo) -> Result<(), String> {
        log::info("hello started");
        let tree = ui::column().child(ui::heading("Hello"));
        ui::show(ui::Slot::Panel, tree).map_err(|e| e.to_string())
    }

    fn on_event(_event: Event) {}

    fn stop() {}
}

mumcord_plugin_sdk::export!(Hello);
```

Plugins are built for `wasm32-unknown-unknown`; `mumcord-plugin build` turns the module into a component and packs it into a `.mcplugin` package. A complete example is `examples/hello.rs`.

## API 1.1

Plugins are written against API 1.0 by default. Enable the feature to use the 1.1 world:

```toml
mumcord-plugin-sdk = { version = "...", features = ["1.1"] }
```

With it, the SDK binds `mumcord:plugin@1.1.0` and adds typed wrappers: `stage`, `app_menu`, `relay`, `audio` and `files` (save, import-picture), `chat::post_image`, the `webview` places `SidePanel` and `Stage` with `set_size_hint`, the new `Event` cases and the new `Error` cases (`RelayUnavailable`, `OriginNotAllowed`, `NotFound`). The manifest must then declare `"api": "^1.1"` (`mumcord-plugin build` writes it from the imports). A complete example is `examples/stage_board.rs`; it versions its relay messages with a leading format byte, so a newer plugin can ignore data it does not understand.

`files.save`, `files.import-picture` and `chat.post-image` open Mumcord's own dialogs or post into the real chat, so the mock host of `mumcord-plugin test` always answers them with `not-permitted`; try them in Mumcord itself.

## Licence

Apache-2.0. Plugins built with the SDK can use any licence.
