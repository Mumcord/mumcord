;; Shows a "B2" chip as member badge for every user that joins or is in the presence snapshot.
(module
  (import "mumcord:plugin/log@1.0.0" "write" (func $log (param i32 i32 i32)))
  (import "mumcord:plugin/ui@1.0.0" "set-tree" (func $set_tree (param i32 i32 i32 i32 i32 i32)))
  {{PRELUDE}}
  (data (i32.const 200) "B2")
  (func (export "start") (param i32 i32 i32 i32 i32) (result i32) (call $start_ok))
  (func (export "on-event") (param $ev i32)
    (if (i32.eq (i32.load8_u (local.get $ev)) (i32.const {{EVENT_CASE:joined}}))
      (then
        ;; One node at 512: a chip whose text is "B2". Memory starts zeroed, so the colour
        ;; (none), animation (none) and children (empty) need no writes.
        (i32.store8 (i32.const 512) (i32.const {{COMPONENT_CASE:chip}}))
        (i32.store (i32.add (i32.const 512) (i32.const {{COMPONENT_PAYLOAD}})) (i32.const 200))
        (i32.store (i32.add (i32.const 512) (i32.add (i32.const {{COMPONENT_PAYLOAD}}) (i32.const 4))) (i32.const 2))
        ;; The peer record starts with the session number.
        (call $set_tree
          (i32.const {{SLOT_CASE:member-badge}})
          (i32.load (i32.add (local.get $ev) (i32.const {{EVENT_PAYLOAD}})))
          ;; The slot variant also carries a string (chat-card), so it flattens to three values.
          (i32.const 0)
          (i32.const 512)
          (i32.const 1)
          (i32.const 128)))))
)
