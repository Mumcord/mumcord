;; Starts and ignores every event: stands in for plugins whose only interesting part is the manifest.
(module
  (import "mumcord:plugin/log@1.0.0" "write" (func $log (param i32 i32 i32)))
  {{PRELUDE}}
  (func (export "start") (param i32 i32 i32 i32 i32) (result i32) (call $start_ok))
  (func (export "on-event") (param i32))
)
