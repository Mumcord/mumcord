# Test Relay (`test-relay`)

E2E fixture of the 1.1 plugin platform (spec 09 §10.2), served by the third-party fixture
repository. Sends test messages (`src/messages.rs`: header with sequence and checksum, patterned
body) through `relay`.

- Apps menu `start` (or the panel button) starts a tree activity, so the stage header shows the
  relay connection chip (AC-9.18, AC-9.19).
- Panel buttons: 64 KiB or 900 B to the channel, with the fallback on or off; 64 KiB (fallback off)
  or 900 B (fallback on) to the last peer that announced this plugin.
- The panel and the stage show `Relay: <status>`, `Received: <n>` and a log: `<action>: sent #<seq>
<size> B` or the error (e.g. `RelayUnavailable`), and `received #<seq> <size> B intact from
<name>`.
