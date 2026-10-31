/** Who the "Block …?" confirmation is open for (issue #140). */
import { useSyncExternalStore } from "react";
import type { BlockHow } from "./blockedStore";

export interface BlockTarget {
  serverId: number;
  /** Untrusted plain text. */
  name: string;
  /** Whether the person has a certificate: then the block follows it, else only this name here. */
  hasCert: boolean;
  how: BlockHow;
  /** Gets the focus back when the dialog closes. */
  opener: HTMLElement | null;
}

let target: BlockTarget | null = null;
const listeners = new Set<() => void>();

function set(next: BlockTarget | null): void {
  target = next;
  for (const listener of listeners) listener();
}

export function openBlockDialog(next: BlockTarget): void {
  set(next);
}

export function closeBlockDialog(): void {
  set(null);
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function useBlockTarget(): BlockTarget | null {
  return useSyncExternalStore(subscribe, () => target);
}

/**
 * The dialog target for someone known by session and certificate (person menu, requests): by
 * session while they are online, else by certificate. `null` when neither works.
 */
export function blockTargetFor(
  serverId: number,
  name: string,
  certHash: string | null,
  session: number | null,
  opener: HTMLElement | null,
): BlockTarget | null {
  const hasCert = certHash !== null;
  if (session !== null) {
    return { serverId, name, hasCert, how: { kind: "session", session }, opener };
  }
  if (certHash !== null)
    return { serverId, name, hasCert, how: { kind: "cert", certHash }, opener };
  return null;
}
