;; Puts a one-chip panel in the Plugins strip (the strip lists only plugins with a panel tree),
;; shows web/index.html in the panel popover and echoes every message the page sends, so the
;; page can prove the bridge works while its own network and parent probes are blocked (AC-6.16).
(module
  (import "mumcord:plugin/log@1.0.0" "write" (func $log (param i32 i32 i32)))
  (import "mumcord:plugin/ui@1.0.0" "set-tree" (func $set_tree (param i32 i32 i32 i32 i32 i32)))
  (import "mumcord:plugin/webview@1.0.0" "show" (func $show (param i32 i32 i32 i32)))
  (import "mumcord:plugin/webview@1.0.0" "post" (func $post (param i32 i32 i32)))
  {{PRELUDE}}
  (data (i32.const 200) "index.html")
  (data (i32.const 240) "Web view")
  (func $show_page
    ;; place panel-popover is case 0
    (call $show (i32.const 0) (i32.const 200) (i32.const 10) (i32.const 128)))
  (func (export "start") (param i32 i32 i32 i32 i32) (result i32)
    ;; One node at 512: a chip "Web view". Memory starts zeroed, so colour, animation and
    ;; children need no writes. The panel slot has no payload.
    (i32.store8 (i32.const 512) (i32.const {{COMPONENT_CASE:chip}}))
    (i32.store (i32.add (i32.const 512) (i32.const {{COMPONENT_PAYLOAD}})) (i32.const 240))
    (i32.store (i32.add (i32.const 512) (i32.add (i32.const {{COMPONENT_PAYLOAD}}) (i32.const 4))) (i32.const 8))
    (call $set_tree (i32.const {{SLOT_CASE:panel}}) (i32.const 0) (i32.const 0) (i32.const 512) (i32.const 1) (i32.const 128))
    (call $show_page)
    (call $start_ok))
  (func (export "on-event") (param $ev i32)
    (local $kind i32)
    (local.set $kind (i32.load8_u (local.get $ev)))
    (if (i32.or
          (i32.eq (local.get $kind) (i32.const {{EVENT_CASE:ui-action}}))
          (i32.eq (local.get $kind) (i32.const {{EVENT_CASE:panel-visibility-changed}})))
      (then (call $show_page)))
    (if (i32.eq (local.get $kind) (i32.const {{EVENT_CASE:webview-message}}))
      (then
        ;; The payload is the message string: pointer, then length.
        (call $post
          (i32.load (i32.add (local.get $ev) (i32.const {{EVENT_PAYLOAD}})))
          (i32.load (i32.add (local.get $ev) (i32.add (i32.const {{EVENT_PAYLOAD}}) (i32.const 4))))
          (i32.const 128)))))
)
