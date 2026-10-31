/** Settings sections another feature may open directly. */
export type SettingsJumpSection = "identity" | "sounds" | "voice";

let opener: ((section: SettingsJumpSection) => void) | null = null;

/**
 * Registers the app shell's settings opener. Kept as a module-level hook so features deep in the
 * tree (e.g. the server identity dialog) need no view context in their tests.
 */
export function setSettingsOpener(fn: ((section: SettingsJumpSection) => void) | null): void {
  opener = fn;
}

/**
 * Opens Settings on `section`, e.g. "Manage identities…" in the server identity dialog or
 * "Notification settings" in the notification centre.
 */
export function openSettingsSection(section: SettingsJumpSection): void {
  if (opener === null) {
    console.warn("Settings could not be opened: the app shell is not mounted");
    return;
  }
  opener(section);
}
