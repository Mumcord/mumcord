import { useSyncExternalStore } from "react";

/**
 * "<name> installed" toasts (FR-6.60). They live outside the Browse tab so a toast still shows
 * when Settings was closed while the install ran.
 */
export interface PluginToast {
  id: number;
  text: string;
}

let toasts: readonly PluginToast[] = [];
let nextId = 1;
const listeners = new Set<() => void>();

function set(next: readonly PluginToast[]): void {
  toasts = next;
  for (const listener of listeners) listener();
}

export function showPluginToast(text: string): void {
  set([...toasts, { id: nextId++, text }]);
}

export function dismissPluginToast(id: number): void {
  set(toasts.filter((toast) => toast.id !== id));
}

export function usePluginToasts(): readonly PluginToast[] {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    () => toasts,
  );
}

/** Tests only. */
export function resetPluginToasts(): void {
  set([]);
}
