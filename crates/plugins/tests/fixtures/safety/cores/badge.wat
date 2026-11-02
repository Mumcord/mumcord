;; Shows a "B2" chip as member badge for everyone in the channel, the user included: those already
;; there when it starts (the presence snapshot; a plugin installed later gets no `joined` for them)
;; and everyone who joins afterwards.
(module
  (import "mumcord:plugin/log@1.0.0" "write" (func $log (param i32 i32 i32)))
  (import "mumcord:plugin/ui@1.0.0" "set-tree" (func $set_tree (param i32 i32 i32 i32 i32 i32)))
  (import "mumcord:plugin/peers@1.0.0" "presence" (func $presence (param i32)))
  {{PRELUDE}}
  (data (i32.const 200) "B2")

  (func $badge (param $session i32)
    ;; One node at 512: a chip whose text is "B2". Memory starts zeroed, so the colour (none),
    ;; animation (none) and children (empty) need no writes.
    (i32.store8 (i32.const 512) (i32.const {{COMPONENT_CASE:chip}}))
    (i32.store (i32.add (i32.const 512) (i32.const {{COMPONENT_PAYLOAD}})) (i32.const 200))
    (i32.store (i32.add (i32.const 512) (i32.add (i32.const {{COMPONENT_PAYLOAD}}) (i32.const 4))) (i32.const 2))
    (call $set_tree
      (i32.const {{SLOT_CASE:member-badge}})
      (local.get $session)
      ;; The slot variant also carries a string (chat-card), so it flattens to three values.
      (i32.const 0)
      (i32.const 512)
      (i32.const 1)
      (i32.const 128)))

  (func (export "start") (param i32 i32 i32 i32 i32) (result i32)
    (local $peer i32)
    (local $end i32)
    ;; result<list<peer>, error> at 2048: discriminant byte, then the list's pointer and length.
    (call $presence (i32.const 2048))
    (if (i32.eqz (i32.load8_u (i32.const 2048)))
      (then
        (local.set $peer (i32.load (i32.const 2052)))
        (local.set $end
          (i32.add (local.get $peer) (i32.mul (i32.load (i32.const 2056)) (i32.const {{PEER_SIZE}}))))
        (block $done
          (loop $next
            (br_if $done (i32.ge_u (local.get $peer) (local.get $end)))
            ;; The peer record starts with the session number.
            (call $badge (i32.load (local.get $peer)))
            (local.set $peer (i32.add (local.get $peer) (i32.const {{PEER_SIZE}})))
            (br $next)))))
    (call $start_ok))

  (func (export "on-event") (param $ev i32)
    (if (i32.eq (i32.load8_u (local.get $ev)) (i32.const {{EVENT_CASE:joined}}))
      (then (call $badge (i32.load (i32.add (local.get $ev) (i32.const {{EVENT_PAYLOAD}})))))))
)
