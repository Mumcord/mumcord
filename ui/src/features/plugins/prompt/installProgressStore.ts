/**
 * The running installs and updates, by plugin id, from `plugins://install-progress`. One shared
 * store so the install prompt, the Browse card and details, the Updates row and the Installed list
 * all show the same progress without each listening on its own.
 */
import { useSyncExternalStore } from "react";
import { getApi, type MumcordApi, type PluginInstallStage, type Unsubscribe } from "../../../api";

/** A step that is still running; `done` and `failed` end the entry. */
export type ActiveInstallStage = Exclude<
  PluginInstallStage,
  { stage: "done" } | { stage: "failed" }
>;

type Active = ReadonlyMap<string, ActiveInstallStage>;

const NONE: Active = new Map();
let current: Active = NONE;
let boundApi: MumcordApi | undefined;
let off: Unsubscribe | undefined;
const listeners = new Set<() => void>();

function publish(next: Active): void {
  current = next;
  for (const listener of listeners) listener();
}

function apply(id: string, progress: PluginInstallStage): void {
  const next = new Map(current);
  if (progress.stage === "done" || progress.stage === "failed") next.delete(id);
  else next.set(id, progress);
  publish(next);
}

function subscribe(listener: () => void): () => void {
  const api = getApi();
  if (api !== boundApi) {
    off?.();
    boundApi = api;
    current = NONE;
    off = api.on("pluginsInstallProgress", ({ id, progress }) => {
      apply(id, progress);
    });
  }
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0) {
      off?.();
      off = undefined;
      boundApi = undefined;
      current = NONE;
    }
  };
}

/**
 * Forgets the progress of `id`. The install prompt calls it once its download and check are
 * finished: that step reports no "done", as nothing is installed until the user agrees.
 */
export function clearInstallProgress(id: string): void {
  if (!current.has(id)) return;
  const next = new Map(current);
  next.delete(id);
  publish(next);
}

/** The running step of the install or update of `id`, or `null` when none runs. */
export function useInstallProgress(id: string): ActiveInstallStage | null {
  return useSyncExternalStore(subscribe, () => current).get(id) ?? null;
}
