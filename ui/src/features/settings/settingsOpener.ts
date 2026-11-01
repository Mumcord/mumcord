import type { SectionId } from "./sectionIds";

let opener: ((section: SectionId) => void) | null = null;

/**
 * Registers the app shell's settings opener. Kept as a module-level hook so features deep in the
 * tree (e.g. the server identity dialog) need no view context in their tests.
 */
export function setSettingsOpener(fn: ((section: SectionId) => void) | null): void {
  opener = fn;
}

/**
 * Opens Settings on `section`, e.g. "Manage identities…" in the server identity dialog or
 * "Notification settings" in the notification centre.
 */
export function openSettingsSection(section: SectionId): void {
  if (opener === null) {
    console.warn("Settings could not be opened: the app shell is not mounted");
    return;
  }
  opener(section);
}
