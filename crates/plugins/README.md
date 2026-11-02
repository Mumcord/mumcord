# mumcord-plugins

The plugin host of [Mumcord](https://github.com/Mumcord/mumcord), a modern voice chat client for standard Mumble servers: the sandboxed WebAssembly runtime (wasmtime, no WASI), the host side of the plugin API `mumcord:plugin@1.0.0`, manifest and `.mcplugin` package handling, signatures and the plugin registry client.

It is published so that the `mumcord-plugin` command-line tool can run plugins in exactly the runtime the client uses. Plugin authors do not depend on it; they use [`mumcord-plugin-sdk`](https://crates.io/crates/mumcord-plugin-sdk).

## Licence

AGPL-3.0-only.
