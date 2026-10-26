import { createContext, useContext } from "react";

/** Top-level screen: wizard on first start, otherwise the main window. */
export type Screen = "loading" | "startupError" | "wizard" | "main";
/** Full-window overlays on top of the main window. */
export type Overlay = "settings" | null;

export interface ViewState {
  screen: Screen;
  overlay: Overlay;
  /** Settings section to show first when the settings open; null keeps the default. */
  settingsSection: "overlay" | null;
}

export type ViewAction =
  | { type: "loaded"; screen: "wizard" | "main" }
  | { type: "failed" }
  | { type: "retry" }
  | { type: "finishWizard" }
  | { type: "openSettings"; section?: "overlay" }
  | { type: "closeOverlay" };

export const initialViewState: ViewState = {
  screen: "loading",
  overlay: null,
  settingsSection: null,
};

export function viewReducer(state: ViewState, action: ViewAction): ViewState {
  switch (action.type) {
    case "loaded":
      return { screen: action.screen, overlay: null, settingsSection: null };
    case "failed":
      return { screen: "startupError", overlay: null, settingsSection: null };
    case "retry":
      return initialViewState;
    case "finishWizard":
      return { screen: "main", overlay: null, settingsSection: null };
    case "openSettings":
      return state.screen === "main"
        ? { ...state, overlay: "settings", settingsSection: action.section ?? null }
        : state;
    case "closeOverlay":
      return { ...state, overlay: null };
  }
}

export interface View extends ViewState {
  /** The wizard calls this after saving `wizardCompleted: true`. */
  finishWizard: () => void;
  openSettings: () => void;
  /** Opens the settings on the overlay section. */
  openOverlaySettings: () => void;
  closeOverlay: () => void;
  retry: () => void;
}

export const ViewContext = createContext<View | null>(null);

export function useView(): View {
  const view = useContext(ViewContext);
  if (!view) throw new Error("useView must be used inside <ViewProvider>");
  return view;
}
