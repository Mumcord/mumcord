# Test Stage (`test-stage`)

E2E fixture of the 1.1 plugin platform (spec 09 §10.2), served by the third-party fixture
repository. A tree activity in the media area; `max_participants` is 2.

- Apps menu: `start` starts the activity at once (the click is the gesture), `late` starts it after
  10 s, past the 8 s gesture window, so the start fails with `NotPermitted` (AC-9.13).
- Panel buttons: start now, start in 10 s, join the last seen activity, leave, end. A second start
  while one runs shows the limit error (AC-9.14).
- The stage and the panel list the events the plugin received (`started`, `joined`, `left`,
  `ended`, `visible`) and the outcome of each call, e.g. `start: ok` or `late-start: NotPermitted`.

`test-stage-official` builds this source under its own id and name.
