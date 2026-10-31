import { useEffect, useMemo, useReducer, type ReactNode } from "react";
import { migrateLegacyHintPreferences } from "../a11y";
import { getApi } from "../api";
import { usePreferences } from "./preferences";
import { initialViewState, viewReducer, ViewContext, type View } from "./view";

/** Loads identity and settings on start and decides between wizard and main window. */
export function ViewProvider({ children }: { children: ReactNode }) {
  const [state, dispatch] = useReducer(viewReducer, initialViewState);
  const { applySettings } = usePreferences();

  useEffect(() => {
    if (state.screen !== "loading") return;
    let cancelled = false;
    const api = getApi();
    Promise.all([
      api.identityGet(),
      api.settingsGet().then((loaded) => migrateLegacyHintPreferences(loaded)),
    ])
      .then(([identity, settings]) => {
        if (cancelled) return;
        applySettings(settings);
        const firstStart = identity === null || !settings.wizardCompleted;
        dispatch({ type: "loaded", screen: firstStart ? "wizard" : "main" });
      })
      .catch((error: unknown) => {
        if (cancelled) return;
        console.error("Loading identity and settings failed", error);
        dispatch({ type: "failed" });
      });
    return () => {
      cancelled = true;
    };
  }, [state.screen, applySettings]);

  const view = useMemo<View>(
    () => ({
      ...state,
      finishWizard: () => {
        dispatch({ type: "finishWizard" });
      },
      openSettings: (section) => {
        dispatch(
          section === undefined ? { type: "openSettings" } : { type: "openSettings", section },
        );
      },
      openOverlaySettings: () => {
        dispatch({ type: "openSettings", section: "overlay" });
      },
      openAudioSettings: () => {
        dispatch({ type: "openSettings", section: "audio" });
      },
      openPrivacySettings: () => {
        dispatch({ type: "openSettings", section: "privacy" });
      },
      openRerunWizard: () => {
        dispatch({ type: "openRerunWizard" });
      },
      closeRerunWizard: () => {
        dispatch({ type: "closeRerunWizard" });
      },
      closeOverlay: () => {
        dispatch({ type: "closeOverlay" });
      },
      retry: () => {
        dispatch({ type: "retry" });
      },
    }),
    [state],
  );

  return <ViewContext value={view}>{children}</ViewContext>;
}
