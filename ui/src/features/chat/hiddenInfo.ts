/**
 * Memory of dismissed pinned info boxes (FR-2.66): the server welcome message (per server) and
 * a channel description (per channel). Stores a hash of the hidden text, so the box shows again
 * as soon as the text changes. A UI convenience kept in
 * local storage; failures fall back to "not hidden" instead of breaking the chat.
 */
import { useSyncExternalStore } from "react";

/** Storage keys: the welcome message belongs to a server, a description to a channel of it. */
export const welcomeKey = (serverId: number): string => `welcome.${String(serverId)}`;
export const descriptionKey = (serverId: number, channelId: number): string =>
  `description.${String(serverId)}.${String(channelId)}`;

const KEY_PREFIX = "mumcord.hidden.";

const listeners = new Set<() => void>();
/** Mirror of storage so a failing storage still hides for this session and reads stay cheap. */
const cache = new Map<string, string | null>();

/** FNV-1a: a short, stable fingerprint; it only detects changes, it is not a security hash. */
export function textHash(text: string): string {
  let hash = 0x811c9dc5;
  for (let index = 0; index < text.length; index += 1) {
    hash ^= text.charCodeAt(index);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return `${hash.toString(16)}:${String(text.length)}`;
}

function read(key: string): string | null {
  if (cache.has(key)) return cache.get(key) ?? null;
  let value: string | null = null;
  try {
    value = localStorage.getItem(KEY_PREFIX + key);
  } catch {
    value = null;
  }
  cache.set(key, value);
  return value;
}

function write(key: string, hash: string | null): void {
  cache.set(key, hash);
  try {
    if (hash === null) localStorage.removeItem(KEY_PREFIX + key);
    else localStorage.setItem(KEY_PREFIX + key, hash);
  } catch (error) {
    console.error("Saving the welcome message state failed", error);
  }
  listeners.forEach((listener) => {
    listener();
  });
}

export function hideInfo(key: string, text: string): void {
  write(key, textHash(text));
}

export function showInfo(key: string): void {
  write(key, null);
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** Hash of the hidden text stored under `key`, or null when it is not hidden. */
export function useHiddenHash(key: string): string | null {
  return useSyncExternalStore(subscribe, () => read(key));
}

/** Test hook: forgets the in-memory mirror so storage is read again. */
export function resetHiddenInfo(): void {
  cache.clear();
}
