;; A component that imports WASI. Mumcord provides no WASI, so it must not load (AC-6.9).
(component
  (import "wasi:filesystem/types@0.2.0" (instance)))
