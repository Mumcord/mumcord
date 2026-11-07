/**
 * Opens the "Register on this server" dialog of the user panel from elsewhere, e.g. the hint
 * after a relay refused a guest (#601).
 */
import { useSyncExternalStore } from "react";

let requested = 0;
const listeners = new Set<() => void>();

export function openRegisterDialog(): void {
  requested += 1;
  for (const listener of listeners) listener();
}

/** Grows with every request, so the user panel opens the dialog once per request. */
export function useRegisterRequests(): number {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    () => requested,
  );
}
