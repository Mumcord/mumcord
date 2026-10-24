import { createContext, useContext } from "react";

/** Top-level screen: wizard on first start, otherwise the main window. */
export type Screen = "loading" | "startupError" | "wizard" | "main";
/** Full-window overlays on top of the main window. */
export type Overlay = "settings" | null;

export interface ViewState {
  screen: Screen;
  overlay: Overlay;
}

export type ViewAction =
  | { type: "loaded"; screen: "wizard" | "main" }
  | { type: "failed" }
  | { type: "retry" }
  | { type: "finishWizard" }
  | { type: "openSettings" }
  | { type: "closeOverlay" };

export const initialViewState: ViewState = { screen: "loading", overlay: null };

export function viewReducer(state: ViewState, action: ViewAction): ViewState {
  switch (action.type) {
    case "loaded":
      return { screen: action.screen, overlay: null };
    case "failed":
      return { screen: "startupError", overlay: null };
    case "retry":
      return initialViewState;
    case "finishWizard":
      return { screen: "main", overlay: null };
    case "openSettings":
      return state.screen === "main" ? { ...state, overlay: "settings" } : state;
    case "closeOverlay":
      return { ...state, overlay: null };
  }
}

export interface View extends ViewState {
  /** The wizard calls this after saving `wizardCompleted: true`. */
  finishWizard: () => void;
  openSettings: () => void;
  closeOverlay: () => void;
  retry: () => void;
}

export const ViewContext = createContext<View | null>(null);

export function useView(): View {
  const view = useContext(ViewContext);
  if (!view) throw new Error("useView must be used inside <ViewProvider>");
  return view;
}
