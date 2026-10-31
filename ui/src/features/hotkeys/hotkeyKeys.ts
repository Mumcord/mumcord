import type { HotkeyAction } from "../../api";

/**
 * Stored hotkeys use the engine's canonical form: modifiers in the order Ctrl, Alt, Shift, Super,
 * then one key ("Ctrl+Shift+D", "F9", "Numpad1"). These helpers turn a `keyCombo` from a key press
 * into that form and split a stored key for display.
 */

/** `keyCombo` output ("Ctrl+KeyA", "Shift+Digit1") in the stored form ("Ctrl+A", "Shift+1"). */
export function canonicalCombo(combo: string): string {
  const parts = combo.split("+");
  const key = parts.pop() ?? "";
  const single = /^(?:Key|Digit)(.)$/.exec(key);
  return [...parts, single ? single[1] : key].join("+");
}

/** The parts of a stored key for one `<kbd>` each; "Super" reads as "Cmd" on macOS. */
export function keyParts(key: string, macos: boolean): string[] {
  return key.split("+").map((part) => (macos && part === "Super" ? "Cmd" : part));
}

/** Two actions are the same hotkey row (a "switch to channel" row per channel). */
export function sameAction(a: HotkeyAction, b: HotkeyAction): boolean {
  if (a.kind === "switchChannel" && b.kind === "switchChannel") {
    return a.serverId === b.serverId && a.path.join("\u0000") === b.path.join("\u0000");
  }
  return a.kind === b.kind;
}

/** Whether a key press is meant for a text field, where it must type and not trigger anything. */
export function typingTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return (
    target.isContentEditable ||
    target instanceof HTMLTextAreaElement ||
    (target instanceof HTMLInputElement &&
      !["checkbox", "radio", "button", "range"].includes(target.type))
  );
}
