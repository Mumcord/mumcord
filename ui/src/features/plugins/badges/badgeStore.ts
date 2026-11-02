/**
 * What the badge and profile-card slots need to know about plugins: the plugin whose badge is shown next to names (FR-6.63). One shared store so the 100 rows of
 * a big channel tree do not each load the plugin list; it follows `plugins://changed`.
 */
import { useSyncExternalStore } from "react";
import { getApi, type MumcordApi, type Unsubscribe } from "../../../api";

export interface BadgePlugin {
  id: string;
  name: string;
}

export interface BadgeState {
  badge: BadgePlugin | null;
  /** Running plugins that may add a profile-card section. */
  profileCard: readonly BadgePlugin[];
}

const NONE: BadgeState = { badge: null, profileCard: [] };
let current: BadgeState = NONE;
let boundApi: MumcordApi | undefined;
let off: Unsubscribe | undefined;
let loadId = 0;
const listeners = new Set<() => void>();

function publish(next: BadgeState): void {
  if (JSON.stringify(next) === JSON.stringify(current)) return;
  current = next;
  for (const listener of listeners) listener();
}

function load(api: MumcordApi): void {
  const id = ++loadId;
  api.pluginsList().then(
    (overview) => {
      if (id !== loadId) return;
      const running = overview.allOff
        ? []
        : overview.plugins.filter((p) => p.enabled && p.status.kind === "running");
      const chosen = running.find((p) => p.badge);
      publish({
        badge: chosen ? { id: chosen.id, name: chosen.name } : null,
        profileCard: running
          .filter((p) => p.granted.includes("ui.profile-card"))
          .map((p) => ({ id: p.id, name: p.name })),
      });
    },
    (error: unknown) => {
      console.error("Loading the badge plugin failed", error);
    },
  );
}

function subscribe(listener: () => void): () => void {
  const api = getApi();
  if (api !== boundApi) {
    off?.();
    boundApi = api;
    current = NONE;
    off = api.on("pluginsChanged", () => {
      load(api);
    });
    load(api);
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

/** The chosen badge plugin, or `null` when none is chosen or it is not running. */
export function useBadgePlugin(): BadgePlugin | null {
  return useSyncExternalStore(subscribe, () => current).badge;
}

/** Running plugins that may add a section to profile cards. */
export function usePluginList(): readonly BadgePlugin[] {
  return useSyncExternalStore(subscribe, () => current).profileCard;
}
