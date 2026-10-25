/** The overlay options and move mode, loaded once and then kept current by `overlayChanged`. */
import { useEffect, useState } from "react";
import { getApi, toEngineError, type OverlayState } from "../api";
import { applyLanguage, osLanguages, resolveLanguage } from "../i18n";

export function useOverlayState(): OverlayState | null {
  const [state, setState] = useState<OverlayState | null>(null);

  useEffect(() => {
    const api = getApi();
    let live = true;
    const stop = api.on("overlayChanged", ({ state: next }) => {
      setState(next);
    });
    api
      .overlayState()
      .then((initial) => {
        // An event that arrived meanwhile is newer than this answer.
        if (live) setState((current) => current ?? initial);
      })
      .catch((cause: unknown) => {
        console.error("Loading the overlay options failed", toEngineError(cause).code);
      });
    return () => {
      live = false;
      stop();
    };
  }, []);

  const language = state?.language ?? null;
  const loaded = state !== null;
  useEffect(() => {
    if (!loaded) return;
    void applyLanguage(resolveLanguage(language, osLanguages()));
  }, [language, loaded]);

  return state;
}
