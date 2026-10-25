/**
 * Who the overlay lists in "talking" mode: everyone talking now plus those who stopped less than
 * `FADE_MS` ago (they fade out meanwhile), in the order they started. Nothing runs while nobody
 * talks: one timer per person who stopped, no polling.
 */
import { useEffect, useRef, useState } from "react";
import { useServerSelector, type ServerState } from "../features/channels/serverStore";

/** How long someone stays listed (fading out) after they stopped talking. */
export const FADE_MS = 1500;

export interface Talker {
  session: number;
  /** Stopped talking; on its way out. */
  fading: boolean;
}

const selectTalking = (s: ServerState) => s.talking;

export function useTalkers(): readonly Talker[] {
  const talking = useServerSelector(selectTalking);
  // Insertion order is the start order; `true` marks people who stopped. The ref is the source of
  // truth for the effect and the timers; the state copy triggers renders.
  const recentRef = useRef<ReadonlyMap<number, boolean>>(new Map());
  const [recent, setRecent] = useState<ReadonlyMap<number, boolean>>(() => new Map());
  const timers = useRef(new Map<number, ReturnType<typeof setTimeout>>());

  useEffect(() => {
    const commit = (next: ReadonlyMap<number, boolean>) => {
      recentRef.current = next;
      setRecent(next);
    };
    const pending = timers.current;
    const previous = recentRef.current;
    const next = new Map(previous);
    let changed = false;
    for (const session of talking) {
      clearTimeout(pending.get(session));
      pending.delete(session);
      if (next.get(session) !== false) {
        next.set(session, false);
        changed = true;
      }
    }
    for (const [session, fading] of previous) {
      if (fading || talking.has(session)) continue;
      next.set(session, true);
      changed = true;
      pending.set(
        session,
        setTimeout(() => {
          pending.delete(session);
          if (recentRef.current.get(session) !== true) return;
          const rest = new Map(recentRef.current);
          rest.delete(session);
          commit(rest);
        }, FADE_MS),
      );
    }
    if (changed) commit(next);
  }, [talking]);

  useEffect(() => {
    const pending = timers.current;
    return () => {
      for (const timer of pending.values()) clearTimeout(timer);
      pending.clear();
    };
  }, []);

  return [...recent].map(([session, fading]) => ({ session, fading }));
}
