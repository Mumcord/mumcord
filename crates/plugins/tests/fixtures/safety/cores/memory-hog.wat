;; Grows memory 1 MiB at a time on the first event until the 32 MiB limit refuses (AC-6.9).
(module
  (import "mumcord:plugin/log@1.0.0" "write" (func $log (param i32 i32 i32)))
  {{PRELUDE}}
  (func (export "start") (param i32 i32 i32 i32 i32) (result i32) (call $start_ok))
  (func (export "on-event") (param i32)
    (loop $more
      (if (i32.eq (memory.grow (i32.const 16)) (i32.const -1))
        (then (unreachable)))
      (br $more)))
)
