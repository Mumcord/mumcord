/** Id of the notification centre panel, for the bell's `aria-controls`. */
export const CENTRE_PANEL_ID = "notification-centre";

type ShortcutEvent = Pick<KeyboardEvent, "key" | "ctrlKey" | "metaKey" | "shiftKey" | "altKey">;

/** Ctrl+Shift+N, or Cmd+Shift+N on macOS: open or close the notification centre (#143). */
export function isCentreShortcut(event: ShortcutEvent): boolean {
  return (
    (event.ctrlKey || event.metaKey) &&
    event.shiftKey &&
    !event.altKey &&
    event.key.toLowerCase() === "n"
  );
}

/** Puts the keyboard focus back on the bell after the panel closed. */
export function focusBell(): void {
  requestAnimationFrame(() => {
    document.querySelector<HTMLElement>("[data-centre-bell]")?.focus();
  });
}
