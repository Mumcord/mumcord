;; Starts fine, then never returns from on-event. The host sends settings-changed right after
;; start, so the loop begins at once (AC-6.8).
(module
  (import "mumcord:plugin/log@1.0.0" "write" (func $log (param i32 i32 i32)))
  {{PRELUDE}}
  (func (export "start") (param i32 i32 i32 i32 i32) (result i32) (call $start_ok))
  (func (export "on-event") (param i32)
    (loop $forever (br $forever)))
)
