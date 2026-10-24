/** Shared state between `ChannelTree` and its memoised rows (`treeRows`). */
import { createContext, type ReactNode } from "react";
import type { User } from "../../api";
import type { ServerState } from "./serverStore";

/** A value that rows read through selectors; setting it notifies them. */
export interface ValueStore<T> {
  get: () => T;
  set: (value: T) => void;
  subscribe: (listener: () => void) => () => void;
}

export function valueStore<T>(initial: T): ValueStore<T> {
  let value = initial;
  const listeners = new Set<() => void>();
  return {
    get: () => value,
    set: (next) => {
      if (Object.is(next, value)) return;
      value = next;
      for (const listener of listeners) listener();
    },
    subscribe: (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
}

/** Tree state owned by `ChannelTree` that rows depend on. */
export interface TreeUi {
  collapsed: ReadonlySet<number>;
  showAll: ReadonlySet<number>;
  tabStopId: string | undefined;
  selectedChannelId: number | null;
  selfChannelId: number | null;
  selfSession: number | null;
}

/** Handlers of `ChannelTree`, read when an event fires (always the latest ones). */
export interface TreeActions {
  focus: (rowId: string) => void;
  setExpanded: (channelId: number, expanded: boolean) => void;
  toggleShowAll: (channelId: number) => void;
  select: (channelId: number) => void;
  join: (channelId: number) => void;
  joinOnce: (channelId: number, now: number) => void;
  openPerson: (session: number, opener: HTMLElement | null) => void;
  /** Pure factories of self-updating content (e.g. the unread badge); see `ChannelTreeProps`. */
  channelBadge?: ((channelId: number) => ReactNode) | undefined;
  channelAction?:
    ((channel: { channelId: number; name: string; tabbable: boolean }) => ReactNode) | undefined;
}

export interface TreeContextValue {
  hintId: string;
  /** The server state the tree shows; rows select from it (e.g. whether someone talks). */
  server: ValueStore<ServerState>;
  ui: ValueStore<TreeUi>;
  avatars: ValueStore<(user: User) => string | null>;
  /** People whose avatar has been on (or near) the screen; only they get avatars loaded. */
  onScreen: OnScreen;
  actions: { current: TreeActions };
}

/**
 * Tracks which people's rows have come near the visible area, so avatars load lazily instead of
 * for every person on connect (scale report, P5). Seen people stay in the set: their avatars stay
 * loaded while scrolling. Without IntersectionObserver (tests) every watched row counts as seen.
 */
export interface OnScreen extends ValueStore<ReadonlySet<number>> {
  /** Watches a row element showing `sessions`; returns a function that stops watching. */
  watch: (element: Element, sessions: readonly number[]) => () => void;
}

/** Load avatars a little before rows scroll into view. */
const ON_SCREEN_MARGIN = "200px";

export function onScreenTracker(): OnScreen {
  const store = valueStore<ReadonlySet<number>>(new Set());
  const watched = new Map<Element, readonly number[]>();
  let pending: number[] = [];
  let frame: number | null = null;
  // Many rows enter at once (connect, scrolling, expanding): one store update per frame.
  const add = (sessions: readonly number[]) => {
    pending.push(...sessions);
    if (frame !== null) return;
    frame = requestAnimationFrame(() => {
      frame = null;
      const current = store.get();
      const fresh = pending.filter((session) => !current.has(session));
      pending = [];
      if (fresh.length > 0) store.set(new Set([...current, ...fresh]));
    });
  };
  const observer =
    typeof IntersectionObserver === "undefined"
      ? null
      : new IntersectionObserver(
          (entries) => {
            for (const entry of entries) {
              const sessions = watched.get(entry.target);
              if (entry.isIntersecting && sessions) add(sessions);
            }
          },
          { rootMargin: ON_SCREEN_MARGIN },
        );
  return {
    ...store,
    watch: (element, sessions) => {
      if (observer === null) {
        const current = store.get();
        if (sessions.some((session) => !current.has(session))) {
          store.set(new Set([...current, ...sessions]));
        }
        return () => undefined;
      }
      watched.set(element, sessions);
      observer.observe(element);
      return () => {
        watched.delete(element);
        observer.unobserve(element);
      };
    },
  };
}

export const TreeContext = createContext<TreeContextValue | null>(null);
