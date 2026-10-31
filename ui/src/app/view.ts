import { createContext, useContext } from "react";

/** Top-level screen: wizard on first start, otherwise the main window. */
export type Screen = "loading" | "startupError" | "wizard" | "main";
/** Full-window overlays on top of the main window. */
export type Overlay = "settings" | null;

/** Settings sections other screens can lead to directly. */
export type SettingsSection = "overlay" | "audio" | "privacy" | "identity" | "sounds";

export interface ViewState {
  screen: Screen;
  overlay: Overlay;
  /** Settings section to show first when the settings open; null keeps the default. */
  settingsSection: SettingsSection | null;
  /** The setup wizard is open again on top of the settings (#149); it saves only on Finish. */
  rerunWizard: boolean;
  /** Bumped when something outside the settings wrote them, so the open screen reloads. */
  settingsRevision: number;
}

export type ViewAction =
  | { type: "loaded"; screen: "wizard" | "main" }
  | { type: "failed" }
  | { type: "retry" }
  | { type: "finishWizard" }
  | { type: "openSettings"; section?: SettingsSection }
  | { type: "openRerunWizard" }
  | { type: "closeRerunWizard" }
  | { type: "closeOverlay" };

export const initialViewState: ViewState = {
  screen: "loading",
  overlay: null,
  settingsSection: null,
  rerunWizard: false,
  settingsRevision: 0,
};

export function viewReducer(state: ViewState, action: ViewAction): ViewState {
  switch (action.type) {
    case "loaded":
      return { ...initialViewState, screen: action.screen };
    case "failed":
      return { ...initialViewState, screen: "startupError" };
    case "retry":
      return initialViewState;
    case "finishWizard":
      return { ...initialViewState, screen: "main" };
    case "openSettings":
      return state.screen === "main"
        ? { ...state, overlay: "settings", settingsSection: action.section ?? null }
        : state;
    case "openRerunWizard":
      return state.overlay === "settings" ? { ...state, rerunWizard: true } : state;
    case "closeRerunWizard":
      // Finish may have written settings behind the open settings screen (#149).
      return { ...state, rerunWizard: false, settingsRevision: state.settingsRevision + 1 };
    case "closeOverlay":
      return { ...state, overlay: null, rerunWizard: false };
  }
}

export interface View extends ViewState {
  /** The wizard calls this after saving `wizardCompleted: true`. */
  finishWizard: () => void;
  openSettings: (section?: SettingsSection) => void;
  /** Opens the settings on the overlay section. */
  openOverlaySettings: () => void;
  /** Opens the settings on the audio section. */
  openAudioSettings: () => void;
  /** Opens the settings on the privacy section. */
  openPrivacySettings: () => void;
  /** Opens the setup wizard again from Settings (#149). */
  openRerunWizard: () => void;
  closeRerunWizard: () => void;
  closeOverlay: () => void;
  retry: () => void;
}

export const ViewContext = createContext<View | null>(null);

/** Opens the privacy settings; `null` without a view provider (tests), where there is nothing to open. */
export function useOpenPrivacySettings(): (() => void) | null {
  return useContext(ViewContext)?.openPrivacySettings ?? null;
}

export function useView(): View {
  const view = useContext(ViewContext);
  if (!view) throw new Error("useView must be used inside <ViewProvider>");
  return view;
}
