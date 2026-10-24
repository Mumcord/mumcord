const MODIFIER_CODES = new Set([
  "ControlLeft",
  "ControlRight",
  "AltLeft",
  "AltRight",
  "ShiftLeft",
  "ShiftRight",
  "MetaLeft",
  "MetaRight",
  "OSLeft",
  "OSRight",
  "AltGraph",
]);

type KeyLike = Pick<KeyboardEvent, "code" | "ctrlKey" | "altKey" | "shiftKey" | "metaKey">;

/**
 * Turns a keydown into the shortcut string `ptt_capture_key` expects (`KeyboardEvent.code` plus
 * modifiers, e.g. "Ctrl+KeyA"). Returns `null` while only a modifier is held, so capture waits
 * for the actual key.
 */
export function keyCombo(event: KeyLike): string | null {
  if (event.code === "" || MODIFIER_CODES.has(event.code)) return null;
  const parts: string[] = [];
  if (event.ctrlKey) parts.push("Ctrl");
  if (event.altKey) parts.push("Alt");
  if (event.shiftKey) parts.push("Shift");
  if (event.metaKey) parts.push("Meta");
  parts.push(event.code);
  return parts.join("+");
}

/** `MouseEvent.button` → `pttKey`: middle button and the two side buttons (back, forward). */
const MOUSE_KEYS: Partial<Record<number, string>> = { 1: "MouseMiddle", 3: "Mouse4", 4: "Mouse5" };

/** Mouse labels by `pttKey`, as translation key suffixes. */
const MOUSE_LABELS: Partial<Record<string, "middle" | "back" | "forward">> = {
  MouseMiddle: "middle",
  Mouse4: "back",
  Mouse5: "forward",
};

/**
 * Turns a mouse button into the `pttKey` string `ptt_capture_key` expects. Left and right click
 * (and anything else) give `null`: they stay ordinary clicks.
 */
export function mouseKey(button: number): string | null {
  return MOUSE_KEYS[button] ?? null;
}

/** Translation suffix for a stored mouse key ("Mouse4" → "back"); `null` for keyboard keys. */
export function mouseLabel(key: string): "middle" | "back" | "forward" | null {
  return MOUSE_LABELS[key] ?? null;
}
