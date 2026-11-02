import { useEffect, useSyncExternalStore } from "react";
import { openSettingsSection } from "../settings/settingsOpener";
import { subscribePluginRequests, type PluginRequest } from "./notifications/pluginRequests";

/**
 * The one place that answers requests to open a plugin screen (from the notification centre,
 * deep links, the empty Installed tab …). The router opens Settings → Plugins on the right tab and
 * keeps the request here until the tab that handles it consumes it, so it also works when Settings
 * was closed or the tab mounts later. Tabs read from this store; nobody else subscribes to the bus.
 */

export type PluginRequestKind = PluginRequest["kind"];
export type PluginRequestOf<K extends PluginRequestKind> = Extract<PluginRequest, { kind: K }>;
/** The tabs a request can open (Developer is never opened by a request). */
export type RequestedPluginTab = "installed" | "browse" | "updates" | "repositories";

const TAB_FOR: Record<PluginRequestKind, RequestedPluginTab> = {
  installed: "installed",
  details: "browse",
  install: "browse",
  updates: "updates",
  addRepository: "repositories",
};

let pending: Partial<Record<PluginRequestKind, PluginRequest>> = {};
let tab: RequestedPluginTab | null = null;
const listeners = new Set<() => void>();

function notify(): void {
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** Opens Settings → Plugins on `target` (e.g. "Browse plugins" in the empty Installed tab). */
export function openPluginTab(target: RequestedPluginTab): void {
  tab = target;
  notify();
  openSettingsSection("plugins");
}

/** Keeps the request for its tab and opens that tab. A newer request of the same kind wins. */
export function routePluginRequest(request: PluginRequest): void {
  pending = { ...pending, [request.kind]: request };
  openPluginTab(TAB_FOR[request.kind]);
}

/** The waiting request of `kind`, or `null`. The tab that handles it calls `consumePluginRequest`. */
export function usePendingPluginRequest<K extends PluginRequestKind>(
  kind: K,
): PluginRequestOf<K> | null {
  return useSyncExternalStore(
    subscribe,
    () => (pending[kind] ?? null) as PluginRequestOf<K> | null,
  );
}

/** Marks the waiting request of `kind` as handled. */
export function consumePluginRequest(kind: PluginRequestKind): void {
  if (pending[kind] === undefined) return;
  pending = Object.fromEntries(Object.entries(pending).filter(([key]) => key !== kind));
  notify();
}

/** The tab Settings → Plugins should switch to, or `null`. PluginsSection consumes it. */
export function useRequestedPluginTab(): RequestedPluginTab | null {
  return useSyncExternalStore(subscribe, () => tab);
}

export function consumeRequestedPluginTab(): void {
  if (tab === null) return;
  tab = null;
  notify();
}

/** Subscribes the router to the request bus while mounted. PluginsHost (always mounted) calls it. */
export function usePluginRequestRouter(): void {
  useEffect(() => subscribePluginRequests(routePluginRequest), []);
}

/** Tests only. */
export function resetPluginRequestStore(): void {
  pending = {};
  tab = null;
  notify();
}
