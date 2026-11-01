const KEY = "mumcord.pluginStrip.collapsed";

/** Whether the user collapsed the strip; it stays collapsed across restarts (FR-6.64). */
export function loadCollapsed(): boolean {
  try {
    return window.localStorage.getItem(KEY) === "1";
  } catch {
    return false;
  }
}

export function saveCollapsed(collapsed: boolean): void {
  try {
    window.localStorage.setItem(KEY, collapsed ? "1" : "0");
  } catch (error: unknown) {
    console.error("Remembering the plugin strip state failed", error);
  }
}
