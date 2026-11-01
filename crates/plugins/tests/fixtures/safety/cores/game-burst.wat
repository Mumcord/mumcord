;; Like the spammer, but asks for a game burst on the first tick and again after 12 s. Each
;; request logs whether it was granted: the second must be refused, because a plugin may burst
;; only once per minute (AC-6.12).
(module
  (import "mumcord:plugin/log@1.0.0" "write" (func $log (param i32 i32 i32)))
  (import "mumcord:plugin/timers@1.0.0" "start" (func $timer_start (param i32 i32 i32)))
  (import "mumcord:plugin/peers@1.0.0" "send" (func $send (param i32 i32 i32 i32 i32 i32)))
  (import "mumcord:plugin/peers@1.0.0" "start-game-burst" (func $burst (param i32)))
  {{PRELUDE}}
  (data (i32.const 200) "ping")
  (data (i32.const 208) "game-burst-started")
  (data (i32.const 240) "game-burst-refused")
  (global $tick (mut i32) (i32.const 0))

  (func $request_burst
    (call $burst (i32.const 128))
    (if (i32.eqz (i32.load8_u (i32.const 128)))
      (then (call $log_info (i32.const 208) (i32.const 18)))
      (else (call $log_info (i32.const 240) (i32.const 18)))))

  (func (export "start") (param i32 i32 i32 i32 i32) (result i32)
    (call $timer_start (i32.const 100) (i32.const 1) (i32.const 128))
    (call $start_ok))
  (func (export "on-event") (param $ev i32)
    (if (i32.eq (i32.load8_u (local.get $ev)) (i32.const {{EVENT_CASE:timer}}))
      (then
        (global.set $tick (i32.add (global.get $tick) (i32.const 1)))
        (if (i32.or (i32.eq (global.get $tick) (i32.const 1)) (i32.eq (global.get $tick) (i32.const 120)))
          (then (call $request_burst)))
        (call $send (i32.const 0) (i32.const 0) (i32.const 1) (i32.const 200) (i32.const 4) (i32.const 128)))))
)
