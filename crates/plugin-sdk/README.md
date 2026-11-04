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

With it, the SDK binds `mumcord:plugin@1.1.0` and adds typed wrappers: `stage`, `app_menu`, `relay`, `audio` and `files` (save, import-picture), `chat::post_image`, the `webview` places `SidePanel` and `Stage` with `set_size_hint` and `set_status` (a plain-text status line under the side panel, at most 80 characters, at most 4 updates per second, the last one wins), the new `Event` cases and the new `Error` cases (`RelayUnavailable`, `OriginNotAllowed`, `NotFound`). The manifest must then declare `"api": "^1.1"` (`mumcord-plugin build` writes it from the imports). A complete example is `examples/stage_board.rs`; it sends its relay messages as Bovnar documents.

### Sending data to other players: Bovnar

With `1.1`, the `bovnar` module sends typed messages to other players' copies of your plugin as [Bovnar](https://github.com/sothis/bovnar) documents (a self-describing format: every value carries its type). Implement `bovnar::Message` (a `version()` plus serde) for your message type, send it with `bovnar::send_document` (Mumble plugin data via `Destination::Peers`, at most 997 bytes, or the relay via `Destination::RelayChannel`/`RelayDirect`, at most 64 KiB) and decode incoming payloads with `bovnar::on_document`. Oversized documents are refused before anything is sent; incoming bytes are untrusted and a bad document is dropped with the reason `malformed`, `too-large` or `wrong-type` (the SDK logs the reason and size only, never the content). Fields a receiver does not know are ignored, so a newer plugin can add fields.

The SDK pins exactly one Bovnar version (2.0.0); a new Bovnar version only comes with an SDK upgrade. For the plugin target `wasm32-unknown-unknown` the SDK bundles a prebuilt Bovnar C library (`bovnar-wasm/libbovnar_wasm.a`, MIT, see `bovnar-wasm/LICENSE`), so building a plugin needs no C compiler. Native builds of the SDK (its own tests) compile Bovnar from the crates.io crate and need a C compiler.

A plugin may have up to three pages open at once (stage, side panel, dialog or panel popover). `webview::post` goes to every open page that takes the message, and `Event::WebviewMessage` does not say which page sent it: if your pages need to be told apart, put that in your own payload (each page knows its own entry).

`files.save`, `files.import-picture` and `chat.post-image` open Mumcord's own dialogs or post into the real chat, so the mock host of `mumcord-plugin test` always answers them with `not-permitted`; try them in Mumcord itself.

## Licence

Apache-2.0. Plugins built with the SDK can use any licence.
