/**
 * Remembers which element opened a plugin dialog, so focus can return there when it closes.
 * Only the opener is kept; what the dialog shows lives in the plugin UI store.
 */
const openers = new Map<string, HTMLElement>();

/** Called when the user picks a plugin's user-menu entry. */
export function rememberDialogOpener(plugin: string, opener: HTMLElement | undefined): void {
  if (opener) openers.set(plugin, opener);
  else openers.delete(plugin);
}

/** Takes the opener back (once) when the dialog closes. */
export function takeDialogOpener(plugin: string): HTMLElement | undefined {
  const opener = openers.get(plugin);
  openers.delete(plugin);
  return opener;
}
