# Test Dev Localhost (`test-dev-localhost`)

E2E fixture of the 1.1 plugin platform (spec 09 FR-9.26, AC-9.10). It declares
`http://localhost:47180`; the E2E test serves plain HTTP there.

- Developer mode: load the unpacked package (unzip the built `.mcplugin`: `manifest.json`,
  `plugin.wasm`, `i18n/`). The panel button "Fetch localhost" shows `Result: 200`.
- Packaged: `mumcord-plugin check` and the install refuse it (hard stop), so `e2e-fixture.json`
  says `"check": "fail"` and it is never served by a fixture registry.
