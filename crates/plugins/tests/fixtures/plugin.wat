;; Core module of the test plugin, written against the canonical ABI of `wit/world.wit`.
;; The tests turn it into a component with wit-component after replacing the START and STOP
;; placeholders with the function bodies each test needs.
(module
  (import "mumcord:plugin/log@1.0.0" "write" (func $log (param i32 i32 i32)))
  (memory (export "memory") 1)
  (global $heap (mut i32) (i32.const 4096))
  (data (i32.const 32) "nope")

  ;; Bump allocator: the host copies strings (the start info) into the plugin with it.
  (func (export "cabi_realloc") (param $old i32) (param $old_size i32) (param $align i32) (param $size i32) (result i32)
    (local $p i32)
    (local.set $p
      (i32.and
        (i32.add (global.get $heap) (i32.sub (local.get $align) (i32.const 1)))
        (i32.xor (i32.sub (local.get $align) (i32.const 1)) (i32.const -1))))
    (global.set $heap (i32.add (local.get $p) (local.get $size)))
    (local.get $p))

  ;; start(info: start-info) -> result<_, string>
  ;; Params: api-version (ptr, len), previous-version (is-some, ptr, len).
  ;; Returns a pointer to the result: discriminant at +0, error string at +4/+8.
  (func (export "start") (param $api_ptr i32) (param $api_len i32) (param $has_prev i32) (param $prev_ptr i32) (param $prev_len i32) (result i32)
    {{START}})

  (func (export "stop")
    {{STOP}})
)
