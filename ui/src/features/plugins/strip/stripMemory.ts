const KEY = "mumcord.pluginStrip.open";

/** The plugin whose inline view was open last, so it stays open across restarts (FR-6.64). */
export function loadOpen(): string | null {
  try {
    return window.localStorage.getItem(KEY) || null;
  } catch {
    return null;
  }
}

export function saveOpen(id: string | null): void {
  try {
    window.localStorage.setItem(KEY, id ?? "");
  } catch (error: unknown) {
    console.error("Remembering the plugin strip state failed", error);
  }
}
