import { useSyncExternalStore } from "react";

/** Side panel width limits in rem (FR-9.39). */
export const SIDE_PANEL_MIN_REM = 18;
export const SIDE_PANEL_MAX_REM = 32;
export const SIDE_PANEL_DEFAULT_REM = 24;
/** Arrow-key step of the resize handle. */
export const SIDE_PANEL_STEP_REM = 1;

export const clampSideWidth = (rem: number): number =>
  Math.min(SIDE_PANEL_MAX_REM, Math.max(SIDE_PANEL_MIN_REM, Math.round(rem * 10) / 10));

/** The smallest width a plugin asks for (its size hint in CSS pixels), within the limits. */
export function minWidthForHint(minWidthPx: number | undefined, remPx = 16): number {
  return clampSideWidth(
    minWidthPx === undefined ? SIDE_PANEL_MIN_REM : Math.ceil(minWidthPx / remPx),
  );
}

/*
 * A side panel the user closed stays hidden until the plugin shows a page again (a new `seq`).
 * The engine does not announce a user close, so the panel is hidden here first.
 */
let dismissed = new Map<string, number>();
const listeners = new Set<() => void>();

export function dismissSidePanel(plugin: string, seq: number): void {
  dismissed = new Map(dismissed).set(plugin, seq);
  for (const listener of listeners) listener();
}

export function resetSidePanelDismissals(): void {
  dismissed = new Map();
  for (const listener of listeners) listener();
}

export function useDismissedSidePanels(): ReadonlyMap<string, number> {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    () => dismissed,
  );
}
