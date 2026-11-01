import { useSyncExternalStore } from "react";

const QUERY = "(prefers-reduced-motion: reduce)";

function media(): MediaQueryList | null {
  return typeof matchMedia === "function" ? matchMedia(QUERY) : null;
}

function subscribe(listener: () => void): () => void {
  const list = media();
  list?.addEventListener("change", listener);
  return () => list?.removeEventListener("change", listener);
}

/** Whether the system asks for reduced motion (§5.8): sprites and animations stop. */
export function useReducedMotion(): boolean {
  return useSyncExternalStore(subscribe, () => media()?.matches ?? false);
}
