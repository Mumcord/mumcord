# Test Surface (`test-surface`)

E2E fixture of the 1.1 plugin platform (spec 09 §10.2). Served by the third-party fixture
repository. `v1.1.1/` builds the same code with one extra website (`update.plugins.test`) for the
update gate (AC-9.36); its folders link to this one.

- Apps menu: `start` starts the web stage, `side` opens the side panel.
- Panel buttons (user gestures): open each web view place, run all probes, busy loop, memory hog,
  frame tone without a click, play the bundled or an undeclared sound, `net.http` to the declared
  and the undeclared origin and through the redirect, disable or enable the `side` item.
- Pages `web/{stage,side,dialog,panel}.html` share `web/probe.js`: one button per probe
  (`#probe-<name>`), results in `#result-<name>[data-status]` and on the panel as
  `<place>/<probe>: <status>`.
- `side.html` also has "Start stage from page": the page asks the plugin to start the web stage,
  a gesture-bound call that works only after a trusted click in the page (FR-9.45, AC-9.13). The
  panel log shows `side-start-stage: ok` or `side-start-stage: not-permitted`. Status `blocked`, `leaked`, `ok` (allowed action worked) or `sent`
  (only the HTTPS fixture's log can tell).
- Fixed addresses: HTTPS fixture on port 47443 (`declared.plugins.test`, `undeclared.plugins.test`),
  STUN on 47478.
