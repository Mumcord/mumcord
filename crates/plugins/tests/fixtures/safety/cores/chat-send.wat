;; Calls chat.send during start, which is never a user gesture, and logs the outcome. It must
;; answer not-permitted and nothing may reach the chat (AC-6.7).
(module
  (import "mumcord:plugin/log@1.0.0" "write" (func $log (param i32 i32 i32)))
  (import "mumcord:plugin/chat@1.0.0" "send" (func $chat_send (param i32 i32 i32)))
  {{PRELUDE}}
  (data (i32.const 200) "[Fixture] no gesture")
  (data (i32.const 232) "chat-send-ok")
  (data (i32.const 248) "chat-send-not-permitted")
  (data (i32.const 280) "chat-send-error")
  (func (export "start") (param i32 i32 i32 i32 i32) (result i32)
    (call $chat_send (i32.const 200) (i32.const 20) (i32.const 128))
    (if (i32.eqz (i32.load8_u (i32.const 128)))
      (then (call $log_info (i32.const 232) (i32.const 12)))
      (else
        ;; error enum: not-permitted is case 0
        (if (i32.eqz (i32.load8_u (i32.const 129)))
          (then (call $log_info (i32.const 248) (i32.const 23)))
          (else (call $log_info (i32.const 280) (i32.const 15))))))
    (call $start_ok))
  (func (export "on-event") (param i32))
)
