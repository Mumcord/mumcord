/**
 * GIF favorites (FR-1.52, QA.43): image links the user saved to send again. The engine keeps
 * them (most recently used first); this keeps a copy for the running session so the star
 * buttons, the image menu and the picker agree without asking the engine on every render.
 */
import { useEffect, useSyncExternalStore } from "react";
import { getApi } from "../../api";

const NONE: readonly string[] = [];
let urls: readonly string[] = NONE;
let loaded = false;
const listeners = new Set<() => void>();

function publish(next: readonly string[]): void {
  urls = next;
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function report(what: string): (error: unknown) => void {
  return (error) => {
    console.error(`${what} failed`, error);
  };
}

/** Newest request wins, so a slow answer never overwrites a later change. */
let request = 0;

function fetchList(): void {
  const mine = ++request;
  getApi()
    .gifFavorites()
    .then((list) => {
      if (mine !== request) return;
      loaded = true;
      publish(list);
    })
    .catch(report("Loading the GIF favorites"));
}

function load(): void {
  if (!loaded) fetchList();
}

/** The saved links, most recently used first. Loads them from the engine on first use. */
export function useGifFavorites(): readonly string[] {
  useEffect(load, []);
  return useSyncExternalStore(subscribe, () => urls);
}

/** Applies a change at once, then takes the engine's list as the truth (also after a failure). */
function change(next: readonly string[], call: () => Promise<unknown>, what: string): void {
  publish(next);
  call().catch(report(what)).finally(fetchList);
}

export function saveFavorite(url: string): void {
  change(
    [url, ...urls.filter((u) => u !== url)],
    () => getApi().gifFavoriteAdd(url),
    "Saving a GIF favorite",
  );
}

export function removeFavorite(url: string): void {
  change(
    urls.filter((u) => u !== url),
    () => getApi().gifFavoriteRemove(url),
    "Removing a GIF favorite",
  );
}

/** Moves a favorite to the front after it was sent. */
export function markFavoriteUsed(url: string): void {
  if (!urls.includes(url)) return;
  change(
    [url, ...urls.filter((u) => u !== url)],
    () => getApi().gifFavoriteUse(url),
    "Updating a GIF favorite",
  );
}

/** Forgets the session copy (tests). */
export function clearFavorites(): void {
  urls = NONE;
  loaded = false;
  request++;
  for (const listener of listeners) listener();
}
