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
