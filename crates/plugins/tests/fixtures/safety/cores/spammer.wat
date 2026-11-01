;; Sends one 4-byte message to the channel on every 100 ms tick. The host must let at most one
;; through per 2 s (AC-6.12).
(module
  (import "mumcord:plugin/log@1.0.0" "write" (func $log (param i32 i32 i32)))
  (import "mumcord:plugin/timers@1.0.0" "start" (func $timer_start (param i32 i32 i32)))
  (import "mumcord:plugin/peers@1.0.0" "send" (func $send (param i32 i32 i32 i32 i32 i32)))
  {{PRELUDE}}
  (data (i32.const 200) "ping")
  (func (export "start") (param i32 i32 i32 i32 i32) (result i32)
    (call $timer_start (i32.const 100) (i32.const 1) (i32.const 128))
    (call $start_ok))
  (func (export "on-event") (param $ev i32)
    (if (i32.eq (i32.load8_u (local.get $ev)) (i32.const {{EVENT_CASE:timer}}))
      (then
        ;; target channel (case 0), kind 1, payload at 200
        (call $send (i32.const 0) (i32.const 0) (i32.const 1) (i32.const 200) (i32.const 4) (i32.const 128)))))
)
