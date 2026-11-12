# Fixture Quit Save (`test-quit-save`)

A test plugin of the Mumcord E2E suite (see `e2e/README.md`). Its `stop()` writes the value
`saved in stop` under the key `stopped` to the plugin's global storage, and nothing else writes
it. `e2e/specs/plugins/quit-signal.e2e.ts` ends Mumcord with SIGTERM and reads the value after the
next start (#748).
