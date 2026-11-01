import { useEffect, useSyncExternalStore } from "react";
import { EngineError, getApi, type PluginIndex } from "../../../api";

/** The store's plugin list, shared by the Browse tab and the deep-link handler. */
export type PluginIndexState =
  | { status: "loading" }
  | { status: "loaded"; index: PluginIndex }
  /** Never fetched and no cached copy; `message` is the engine's text. */
  | { status: "failed"; message: string };

let state: PluginIndexState = { status: "loading" };
let inflight: { language: string; promise: Promise<PluginIndex | null> } | null = null;
const listeners = new Set<() => void>();

function set(next: PluginIndexState): void {
  state = next;
  for (const listener of listeners) listener();
}

/**
 * Loads the list in `language` (the engine answers from its cache when offline). Calls made
 * while a load for the same language runs share it. Resolves to `null` when it failed.
 */
export function loadPluginIndex(language: string): Promise<PluginIndex | null> {
  if (inflight?.language === language) return inflight.promise;
  if (state.status !== "loaded") set({ status: "loading" });
  const promise = getApi()
    .pluginsIndex(language)
    .then((index) => {
      set({ status: "loaded", index });
      return index;
    })
    .catch((error: unknown) => {
      console.error("Loading the plugin list failed", error);
      set({
        status: "failed",
        message: error instanceof EngineError ? error.message : "",
      });
      return null;
    })
    .finally(() => {
      if (inflight?.promise === promise) inflight = null;
    });
  inflight = { language, promise };
  return promise;
}

/** The list, loaded again whenever a component using it mounts or the language changes. */
export function usePluginIndex(language: string): PluginIndexState {
  useEffect(() => {
    void loadPluginIndex(language);
  }, [language]);
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    () => state,
  );
}

/** Tests only. */
export function resetPluginIndex(): void {
  inflight = null;
  set({ status: "loading" });
}
