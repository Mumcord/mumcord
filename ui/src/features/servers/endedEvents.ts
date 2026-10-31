/**
 * The banner of an ended connection hid itself after its timeout (#88). The notification centre
 * (#143, Q1.18) keeps such events so they are not lost when nobody was looking.
 */
import type { ConnectionEnd } from "./store";

export interface EndedHidden {
  end: ConnectionEnd;
  /** The banner's text in the current language, e.g. "Connection lost: server unreachable". */
  text: string;
}

const listeners = new Set<(event: EndedHidden) => void>();

export function announceEndedHidden(event: EndedHidden): void {
  for (const listener of listeners) listener(event);
}

export function subscribeEndedHidden(listener: (event: EndedHidden) => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}
