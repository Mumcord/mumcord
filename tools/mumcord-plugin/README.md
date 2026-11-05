# mumcord-plugin

The command-line tool for authors of [Mumcord](https://github.com/Mumcord/mumcord) plugins and for maintainers of plugin repositories.

```sh
cargo install mumcord-plugin
```

- `mumcord-plugin new <id>` creates a plugin project that uses [`mumcord-plugin-sdk`](https://crates.io/crates/mumcord-plugin-sdk). `--template stage-tree` or `--template stage-web` creates an API 1.1 stage plugin; see the [API 1.1 author guide](../../docs/plugins/api-1-1.md).
- `mumcord-plugin build` builds the plugin into a `.mcplugin` package (`--reproducible` for byte-identical builds).
- `mumcord-plugin check <package>` checks a package the way Mumcord does before installing it.
- `mumcord-plugin test` runs the plugin against YAML scenarios in a mock host that uses the client's own runtime.
- `mumcord-plugin preview` renders the UI the plugin shows into a static HTML page.
- `mumcord-plugin keygen` and `mumcord-plugin sign` create an author key and sign packages.

`mumcord-plugin --help` lists all commands, including the ones for repository maintainers.

## Licence

Apache-2.0. The tool includes the plugin runtime `mumcord-plugins`, which is AGPL-3.0-only.
