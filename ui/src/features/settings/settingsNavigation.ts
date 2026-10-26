import { createContext, useContext } from "react";

/** Lets one settings section send the user to a control in another one. */
export interface SettingsNavigation {
  /** True from the request until the switch has taken the focus. */
  compatibilityFocusPending: boolean;
  compatibilityFocused: () => void;
}

export const SettingsNavigationContext = createContext<SettingsNavigation | null>(null);

export function useSettingsNavigation(): SettingsNavigation | null {
  return useContext(SettingsNavigationContext);
}
