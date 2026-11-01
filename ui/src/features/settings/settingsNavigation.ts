import { createContext, useContext } from "react";

/** Lets one settings section send the user to a control in another one. */
export interface SettingsNavigation {
  /** True from the request until the switch has taken the focus. */
  compatibilityFocusPending: boolean;
  compatibilityFocused: () => void;
  /** Shows Voice and focuses the "Compatibility mode (X11)" switch (Linux Wayland). */
  openCompatibilityMode: () => void;
}

export const SettingsNavigationContext = createContext<SettingsNavigation | null>(null);

export function useSettingsNavigation(): SettingsNavigation | null {
  return useContext(SettingsNavigationContext);
}
