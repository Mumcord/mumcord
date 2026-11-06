/**
 * The plugin activities in the user's voice channel (FR-9.46, FR-9.47), as the host reports them
 * with `stage_activities`. The host sends `plugins://stage-changed` whenever they change (and
 * `plugins://changed` when plugins come or go), so the store simply reads the list again then.
 * Both the stage tabs and the chat's activity lines read it.
 */
import { useSyncExternalStore } from "react";
import { getApi, toEngineError, type StageActivityView, type StageJoinError } from "../../../api";

let activities: readonly StageActivityView[] = [];
/** The host answered at least once; until then the empty list means nothing. */
let loaded = false;
const listeners = new Set<() => void>();
let unsubscribers: (() => void)[] = [];
let loadId = 0;

function publish(next: readonly StageActivityView[]): void {
  activities = next;
  loaded = true;
  for (const listener of listeners) listener();
}

/** Reads the list again; a slower, older answer never replaces a newer one. */
export async function refreshActivities(): Promise<void> {
  const id = ++loadId;
  try {
    const next = await getApi().stageActivities();
    if (id === loadId) publish(next);
  } catch (error: unknown) {
    // The list stays as it was; the next change event reads it again.
    console.error("Loading the plugin activities failed", toEngineError(error).code);
  }
}

/**
 * Listens for activity changes. Exported for code outside React (the activity log), which has to
 * follow the activities while its tab is closed and no component is mounted; the first listener
 * binds the host events and triggers the first load.
 */
export function subscribeStageActivities(listener: () => void): () => void {
  listeners.add(listener);
  if (listeners.size === 1) {
    const api = getApi();
    const reload = () => {
      void refreshActivities();
    };
    unsubscribers = [api.on("pluginsStageChanged", reload), api.on("pluginsChanged", reload)];
    reload();
  }
  return () => {
    listeners.delete(listener);
    if (listeners.size > 0) return;
    for (const unsubscribe of unsubscribers) unsubscribe();
    unsubscribers = [];
  };
}

/** The current plugin activities, for code outside React. */
export function stageActivities(): readonly StageActivityView[] {
  return activities;
}

/** Whether the list came from the host yet; read next to `useStageActivities`. */
export function activitiesLoaded(): boolean {
  return loaded;
}

/** The activity of `plugin` the user is in, if any, for code outside React. */
export function joinedActivityOf(plugin: string): StageActivityView | undefined {
  return activities.find((a) => a.plugin === plugin && a.kind === "activity" && a.joined);
}

export function useStageActivities(): readonly StageActivityView[] {
  return useSyncExternalStore(subscribeStageActivities, () => activities);
}

const JOIN_ERRORS: readonly StageJoinError[] = ["gone", "full", "notPermitted", "failed"];

/**
 * Joins `activity` for the user's click on Join; the host follows up with the activity's tab.
 * Returns why it failed, or `null`.
 */
export async function joinActivity(activity: string): Promise<StageJoinError | null> {
  try {
    await getApi().stageJoin(activity);
    return null;
  } catch (error: unknown) {
    // The command's own errors arrive as their name in the message.
    const { message } = toEngineError(error);
    const reason = JOIN_ERRORS.find((known) => known === message) ?? "failed";
    if (reason === "failed") console.error("Joining a plugin activity failed", message);
    return reason;
  } finally {
    void refreshActivities();
  }
}

/** Leaves `activity` ("Leave <plugin>"); returns whether it worked. */
export async function leaveActivity(activity: string): Promise<boolean> {
  try {
    await getApi().stageLeave(activity);
    return true;
  } catch (error: unknown) {
    console.error("Leaving a plugin activity failed", toEngineError(error).code);
    return false;
  } finally {
    void refreshActivities();
  }
}

/** Test helper: back to the start. */
export function resetActivityStore(): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  unsubscribers = [];
  listeners.clear();
  activities = [];
  loaded = false;
  loadId++;
}
