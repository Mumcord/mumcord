import { createContext, useContext } from "react";

/**
 * Visible labels (in the current language) of the settings that differ from the active preset or
 * the default (#161). Controls look themselves up by their label, so every switch and choice in
 * the Settings window gets its "Changed" badge without wiring each one. Outside the Settings
 * window (wizard, server dialog) there is no provider and no badge.
 */
export const ChangedSettingsContext = createContext<ReadonlySet<string> | null>(null);

/** Whether the setting labelled `label` is marked as changed. */
export function useIsChanged(label: string): boolean {
  return useContext(ChangedSettingsContext)?.has(label) ?? false;
}
