# Test Relay (`test-relay`)

E2E fixture of the 1.1 plugin platform (spec 09 §10.2), served by the third-party fixture
repository. Sends test messages (`src/messages.rs`: a versioned Bovnar document with sequence, checksum and
patterned body, built with the SDK's `bovnar` helpers) through `relay`.

- Apps menu `start` (or the panel button) starts a tree activity, so the stage header shows the
  relay connection chip (AC-9.18, AC-9.19).
- Panel buttons: 64 KiB or 900 B to the channel, with the fallback on or off; 64 KiB (fallback off)
  or 900 B (fallback on) to the last peer that announced this plugin. "Channel malformed bytes"
  sends 300 raw bytes that are no Bovnar document (AC-9.42): the receiver's SDK drops them and
  logs the reason and size, the host delivers them unchanged. "Channel oversized document" sends a
  document above the relay limit, which the SDK refuses with `too-large` before anything leaves
  the machine; "Channel oversized bytes" sends 64 KiB + 1 raw bytes, which the host refuses with
  `too-large`.
- The panel and the stage show `Relay: <status>`, `Received: <n>` and a log: `<action>: sent #<seq>
<size> B` or the error by its WIT name (e.g. `relay-unavailable`), `received #<seq> <size> B
intact from <name>`, `received malformed 300 B dropped, bytes unchanged from <name>`.
