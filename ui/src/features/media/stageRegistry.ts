/**
 * What each stage tab shows (FR-9.44): its name, and for plugin activities the icon and the
 * content. The built-in views are rendered by the app's mounts and only name their tab here;
 * plugin activities bring their content along, so the media area can render them itself.
 */
import { useEffect, useSyncExternalStore, type ReactNode } from "react";
import { closeMedia, openMedia, type MediaKind } from "./mediaView";

export type PluginStageId = `plugin:${string}`;

export interface PluginStageItem {
  /** Tab and screen-reader name, e.g. "Board Brawl". */
  label: string;
  /** Small icon before the name. */
  icon?: ReactNode;
  /** The activity; it stays mounted while another tab is the main item. */
  render: () => ReactNode;
}

export interface StageEntries {
  labels: Partial<Record<MediaKind, string>>;
  plugins: ReadonlyMap<PluginStageId, PluginStageItem>;
}

let entries: StageEntries = { labels: {}, plugins: new Map() };
const listeners = new Set<() => void>();

function change(next: StageEntries): void {
  entries = next;
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/**
 * Puts a plugin activity into the media area (as a tab with a dot while something else is the
 * main item). Calling it again with the same id updates the tab. The returned function removes
 * the activity and its tab.
 */
export function registerStageItem(id: PluginStageId, item: PluginStageItem): () => void {
  const update = entries.plugins.has(id);
  const plugins = new Map(entries.plugins);
  plugins.set(id, item);
  change({ ...entries, plugins });
  if (!update) openMedia(id);
  return () => {
    if (entries.plugins.get(id) !== item) return;
    const rest = new Map(entries.plugins);
    rest.delete(id);
    change({ ...entries, plugins: rest });
    closeMedia(id);
  };
}

/** Names the tab of a built-in view while that view is mounted, e.g. "Jonas's screen". */
export function useStageLabel(kind: MediaKind, label: string): void {
  useEffect(() => {
    if (entries.labels[kind] === label) return;
    change({ ...entries, labels: { ...entries.labels, [kind]: label } });
  }, [kind, label]);
}

export function useStageEntries(): StageEntries {
  return useSyncExternalStore(subscribe, () => entries);
}

/** Test helper: back to the start. */
export function resetStageEntries(): void {
  change({ labels: {}, plugins: new Map() });
}
