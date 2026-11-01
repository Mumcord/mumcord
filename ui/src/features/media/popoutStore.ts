/**
 * Which media views are popped out into their own window (issue #164), from `media_popout_state`
 * and `mediaPopoutChanged`. Both the main window and the pop-out windows read it: the main window
 * shows a placeholder where the view was, the pop-out shows what its slot targets.
 */
import { createContext, useContext, useSyncExternalStore } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../a11y";
import {
  getApi,
  type MediaPopoutKind,
  type MediaPopoutState,
  type MediaPopoutTarget,
  type MumcordApi,
  type Unsubscribe,
} from "../../api";
import { useServerStateOf } from "../channels";

const DOCKED: MediaPopoutState = {
  watch: { target: null, onTop: false },
  share: { target: null, onTop: false },
};

let state: MediaPopoutState = DOCKED;
/** An event arrived, so the initial read is older and must not replace it. */
let changed = false;
let boundApi: MumcordApi | undefined;
let unsubscribers: Unsubscribe[] = [];
const listeners = new Set<() => void>();

function set(next: MediaPopoutState): void {
  changed = true;
  state = next;
  for (const listener of listeners) listener();
}

function bind(api: MumcordApi): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  boundApi = api;
  state = DOCKED;
  changed = false;
  unsubscribers = [
    api.on("mediaPopoutChanged", ({ state: next }) => {
      set(next);
    }),
  ];
  api.mediaPopoutState().then(
    (initial) => {
      if (boundApi === api && !changed) set(initial);
    },
    (error: unknown) => {
      console.error("Loading the pop-out state failed", error);
    },
  );
}

function ensureBound(): void {
  const api = getApi();
  if (api !== boundApi) bind(api);
}

function subscribe(listener: () => void): () => void {
  ensureBound();
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** Which views are popped out right now, for code outside React. */
export function popouts(): MediaPopoutState {
  ensureBound();
  return state;
}

export function usePopouts(): MediaPopoutState {
  return useSyncExternalStore(subscribe, popouts);
}

export function sameTarget(a: MediaPopoutTarget | null, b: MediaPopoutTarget | null): boolean {
  return a?.serverId === b?.serverId && a?.session === b?.session;
}

/** Whether `kind` shows exactly this party (`session` `null`) or share in its own window. */
export function isPoppedOut(
  current: MediaPopoutState,
  kind: MediaPopoutKind,
  serverId: number,
  session: number | null,
): boolean {
  return sameTarget(current[kind].target, { serverId, session });
}

function report(message: string, error: unknown): void {
  console.error(message, error);
  announce(message, "assertive");
}

/** Moves the view into its own window, or brings the open one to the front. */
export function popOut(
  kind: MediaPopoutKind,
  target: MediaPopoutTarget,
  title: string,
  failedText: string,
): void {
  getApi()
    .mediaPopoutOpen(kind, target, title, null)
    .catch((error: unknown) => {
      report(failedText, error);
    });
}

/** Closes the window; the view docks in the main window again. */
export function popIn(kind: MediaPopoutKind, failedText: string): void {
  getApi()
    .mediaPopoutClose(kind)
    .catch((error: unknown) => {
      report(failedText, error);
    });
}

export function setPopoutOnTop(kind: MediaPopoutKind, on: boolean, failedText: string): void {
  getApi()
    .mediaPopoutSetOnTop(kind, on)
    .catch((error: unknown) => {
      report(failedText, error);
    });
}

/**
 * Whether the tree renders inside a pop-out window. The views read it to swap "Pop out" for the
 * window's own buttons and to leave the watch itself to the main window.
 */
export const InPopout = createContext(false);

export function useInPopout(): boolean {
  return useContext(InPopout);
}

/** The kind of pop-out this window is, from its address (`?window=media-popout&kind=…`). */
export function popoutKindOf(search: string): MediaPopoutKind | null {
  const kind = new URLSearchParams(search).get("kind");
  return kind === "watch" || kind === "share" ? kind : null;
}

/**
 * The pop-out's window title: `content` and the user's channel ("Watch party — Lobby"), so the
 * window is easy to find in the taskbar and with Alt+Tab.
 */
export function usePopoutTitle(serverId: number, content: string): string {
  const { t } = useTranslation("common");
  const { users, channels, selfSession } = useServerStateOf(serverId);
  const own = selfSession === null ? undefined : users.get(selfSession);
  const channel = own === undefined ? undefined : channels.get(own.channelId)?.name;
  return channel === undefined ? content : t("media.windowTitle", { title: content, channel });
}

/** Test helper: forgets the binding and docks both views. */
export function resetPopoutStore(): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  unsubscribers = [];
  boundApi = undefined;
  state = DOCKED;
  changed = false;
}
