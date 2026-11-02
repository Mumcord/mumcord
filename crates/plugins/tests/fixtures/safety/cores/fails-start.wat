;; start returns an error: the host must roll an update back and show the reason (AC-6.19).
(module
  (import "mumcord:plugin/log@1.0.0" "write" (func $log (param i32 i32 i32)))
  {{PRELUDE}}
  (data (i32.const 200) "fixture refuses to start")
  (func (export "start") (param i32 i32 i32 i32 i32) (result i32)
    ;; result error: discriminant 1, then the string pointer and length
    (i32.store8 (i32.const 64) (i32.const 1))
    (i32.store (i32.const 68) (i32.const 200))
    (i32.store (i32.const 72) (i32.const 24))
    (i32.const 64))
  (func (export "on-event") (param i32))
)
