# Fixture API 1.0 (`test-api10`)

Backwards-compatibility fixture of the 1.1 plugin platform (spec 09, FR-9.8, AC-9.25). It is built
with the 0.6.2 SDK (WIT 1.0.0, `api: "^1.0"`). `test-api10.mcplugin` is committed so that later SDK
changes cannot alter the binary; the Cargo dependency is pinned to the `v0.6.2` tag for the same
reason. Rebuild only if the fixture itself has to change.
