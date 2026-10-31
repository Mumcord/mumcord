import { createContext, useContext, type RefObject } from "react";

/**
 * Remembers whether the member column is collapsed (issue #165). A layout preference of this
 * window, like the column widths, so it lives in the webview's storage rather than the engine's
 * settings record.
 */

const STORAGE_KEY = "mumcord.membersCollapsed";

export function loadMembersCollapsed(storage?: Storage): boolean {
  try {
    return (storage ?? localStorage).getItem(STORAGE_KEY) === "true";
  } catch (error) {
    console.warn("Reading the member column state failed; showing the column", error);
    return false;
  }
}

export function saveMembersCollapsed(collapsed: boolean, storage?: Storage): void {
  try {
    (storage ?? localStorage).setItem(STORAGE_KEY, String(collapsed));
  } catch (error) {
    console.warn("Saving the member column state failed", error);
  }
}

type ShortcutEvent = Pick<KeyboardEvent, "key" | "ctrlKey" | "metaKey" | "shiftKey" | "altKey">;

/** Ctrl+Shift+M, or Cmd+Shift+M on macOS: show or hide the member column. */
export function isMembersShortcut(event: ShortcutEvent): boolean {
  return (
    (event.ctrlKey || event.metaKey) &&
    event.shiftKey &&
    !event.altKey &&
    event.key.toLowerCase() === "m"
  );
}

/**
 * What the members button in the chat header needs from the shell: whether the column is shown,
 * whether it is the narrow-window overlay, and how to toggle it. The button registers itself in
 * `buttonRef` so the shell can hand it the focus when the column closes around the focus.
 */
export interface MembersColumn {
  shown: boolean;
  overlay: boolean;
  controls: string;
  toggle: () => void;
  buttonRef: RefObject<HTMLButtonElement | null>;
}

export const MembersColumnContext = createContext<MembersColumn | null>(null);

/** The shell's member column, or `null` outside the shell (e.g. in feature tests). */
export function useMembersColumn(): MembersColumn | null {
  return useContext(MembersColumnContext);
}
