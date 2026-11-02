;; Calls notify.send five times in a row. Only the first may become a notification (AC-6.15);
;; each accepted call is logged.
(module
  (import "mumcord:plugin/log@1.0.0" "write" (func $log (param i32 i32 i32)))
  (import "mumcord:plugin/notify@1.0.0" "send" (func $notify (param i32 i32 i32 i32 i32)))
  {{PRELUDE}}
  (data (i32.const 200) "Fixture")
  (data (i32.const 216) "Ping")
  (data (i32.const 232) "notify-delivered")
  (func (export "start") (param i32 i32 i32 i32 i32) (result i32)
    (local $i i32)
    (loop $again
      (call $notify (i32.const 200) (i32.const 7) (i32.const 216) (i32.const 4) (i32.const 128))
      (if (i32.eqz (i32.load8_u (i32.const 128)))
        (then (call $log_info (i32.const 232) (i32.const 16))))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br_if $again (i32.lt_u (local.get $i) (i32.const 5))))
    (call $start_ok))
  (func (export "on-event") (param i32))
)
