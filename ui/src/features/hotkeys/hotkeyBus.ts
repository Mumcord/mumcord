/**
 * Hotkey actions that belong to a mounted component (the member column, the quick settings) are
 * handed over here, so the hotkey handler needs no reference to them.
 */
export type UiHotkeyAction = "toggleMemberColumn" | "openQuickSettings";

const listeners = new Map<UiHotkeyAction, Set<() => void>>();

/** Runs `handler` whenever `action` is triggered; returns the unsubscribe function. */
export function onUiHotkey(action: UiHotkeyAction, handler: () => void): () => void {
  let set = listeners.get(action);
  if (set === undefined) {
    set = new Set();
    listeners.set(action, set);
  }
  set.add(handler);
  return () => {
    set.delete(handler);
  };
}

/** Triggers `action`; returns whether anything handled it. */
export function triggerUiHotkey(action: UiHotkeyAction): boolean {
  const set = listeners.get(action);
  if (set === undefined || set.size === 0) return false;
  for (const handler of set) handler();
  return true;
}
