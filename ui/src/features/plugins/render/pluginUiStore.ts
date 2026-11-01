/**
 * What every running plugin shows (FR-6.40): its trees per slot, its open dialog and its
 * user-menu entry. One store for the app, fed by `plugins_ui_trees` and `plugin://ui`; it
 * reloads after `plugins://changed` because stopped plugins lose their UI. Slot owners
 * (strip, badges, chat, dialog) read from here and render with `PluginTree`.
 */
import { useSyncExternalStore } from "react";
import {
  getApi,
  type MumcordApi,
  type PluginNode,
  type PluginSlot,
  type PluginUi,
  type PluginWebviewPlace,
  type Unsubscribe,
} from "../../../api";
import { routePluginRequest } from "../pluginRequestStore";

export interface PluginDialogHeader {
  readonly title: string;
  readonly subtitle: string | null;
}

export interface PluginUiState {
  /** plugin id → slot key ({@link slotKey}) → tree. */
  readonly trees: ReadonlyMap<string, ReadonlyMap<string, readonly PluginNode[]>>;
  /** plugin id → header of its open dialog. */
  readonly dialogs: ReadonlyMap<string, PluginDialogHeader>;
  /** plugin id → its entry in the user context menu. */
  readonly menuEntries: ReadonlyMap<string, PluginMenuEntryState>;
  /** plugin id → its open web view (FR-6.46). */
  readonly webviews: ReadonlyMap<string, PluginWebview>;
  /** The latest open/close request for a panel popover; `seq` changes on every request. */
  readonly popoverRequest?: PluginPopoverRequest | null;
}

/** A plugin asked to open (or close) its panel in the strip's large popover. */
export interface PluginPopoverRequest {
  readonly plugin: string;
  readonly open: boolean;
  readonly seq: number;
}

/** A user-menu entry: `sessions` lists the users it shows on, `null` means everyone. */
export interface PluginMenuEntryState {
  readonly label: string;
  readonly sessions: readonly number[] | null;
}

/** An open web view; `seq` changes on every `webviewShow`, so showing a page again reloads it. */
export interface PluginWebview {
  readonly place: PluginWebviewPlace;
  readonly url: string;
  readonly seq: number;
}

const EMPTY: PluginUiState = {
  trees: new Map(),
  dialogs: new Map(),
  menuEntries: new Map(),
  webviews: new Map(),
  popoverRequest: null,
};
let webviewSeq = 0;
let popoverSeq = 0;

/** A stable string for a slot, e.g. `chatCard:poll-1`. */
export function slotKey(slot: PluginSlot): string {
  switch (slot.kind) {
    case "memberBadge":
    case "profileCard":
    case "chatDecoration":
      return `${slot.kind}:${String(slot.session)}`;
    case "chatCard":
      return `chatCard:${slot.id}`;
    default:
      return slot.kind;
  }
}

/** Applies one change; returns a new state (the old one stays untouched for React). */
export function applyChange(state: PluginUiState, { plugin, change }: PluginUi): PluginUiState {
  const trees = new Map(state.trees);
  const slots = new Map(trees.get(plugin));
  switch (change.type) {
    case "setTree":
      slots.set(slotKey(change.slot), change.nodes);
      trees.set(plugin, slots);
      return { ...state, trees };
    case "clear":
      slots.delete(slotKey(change.slot));
      trees.set(plugin, slots);
      return { ...state, trees };
    case "openDialog": {
      const dialogs = new Map(state.dialogs);
      dialogs.set(plugin, { title: change.title, subtitle: change.subtitle });
      return { ...state, dialogs };
    }
    case "closeDialog": {
      const dialogs = new Map(state.dialogs);
      dialogs.delete(plugin);
      slots.delete(slotKey({ kind: "dialog" }));
      trees.set(plugin, slots);
      const webviews = new Map(state.webviews);
      if (webviews.get(plugin)?.place === "dialog") webviews.delete(plugin);
      return { ...state, trees, dialogs, webviews };
    }
    case "panelPopover":
      return { ...state, popoverRequest: { plugin, open: change.open, seq: ++popoverSeq } };
    case "userMenuEntry": {
      const menuEntries = new Map(state.menuEntries);
      if (change.label === null) menuEntries.delete(plugin);
      else menuEntries.set(plugin, { label: change.label, sessions: change.sessions });
      return { ...state, menuEntries };
    }
    case "webviewShow": {
      const webviews = new Map(state.webviews);
      webviews.set(plugin, { place: change.place, url: change.url, seq: ++webviewSeq });
      return { ...state, webviews };
    }
    case "webviewHide": {
      const webviews = new Map(state.webviews);
      webviews.delete(plugin);
      return { ...state, webviews };
    }
    // Bridge messages go straight to the open frame (see `webview/`), they are not state.
    case "webviewPost":
      return state;
    // Opens a settings page (see `bind`); nothing to keep.
    case "openPermissions":
      return state;
  }
}

let state: PluginUiState = EMPTY;

/**
 * Ends a plugin's dialog (and a web view shown in it) on the host side. The user's Esc/Close must
 * not depend on the plugin answering; a later `close-dialog` from the plugin changes nothing.
 */
export function closePluginDialog(plugin: string): void {
  ensureBound();
  state = applyChange(state, { plugin, change: { type: "closeDialog" } });
  notify();
}
let boundApi: MumcordApi | undefined;
let unsubscribe: Unsubscribe[] = [];
let generation = 0;
/** Changes that arrived while a snapshot was loading; replayed on top of it. */
let pending: PluginUi[] | null = null;
const listeners = new Set<() => void>();

function notify(): void {
  for (const listener of listeners) listener();
}

function reload(api: MumcordApi): void {
  const mine = ++generation;
  pending = [];
  api.pluginsUiTrees().then(
    (snapshot) => {
      if (generation !== mine) return;
      let next = EMPTY;
      for (const change of [...snapshot, ...(pending ?? [])]) next = applyChange(next, change);
      pending = null;
      state = next;
      notify();
    },
    (error: unknown) => {
      if (generation !== mine) return;
      pending = null;
      console.error("Loading the plugin UI failed", error);
    },
  );
}

function bind(api: MumcordApi): void {
  for (const off of unsubscribe) off();
  boundApi = api;
  state = EMPTY;
  unsubscribe = [
    api.on("pluginUi", (change) => {
      // The plugin asked to show its permissions after a click in its own UI.
      if (change.change.type === "openPermissions") {
        routePluginRequest({ kind: "installed", plugin: change.plugin });
      }
      if (pending !== null) pending.push(change);
      state = applyChange(state, change);
      notify();
    }),
    api.on("pluginsChanged", () => {
      reload(api);
    }),
  ];
  reload(api);
}

function ensureBound(): void {
  const api = getApi();
  if (api !== boundApi) bind(api);
}

function subscribe(listener: () => void): () => void {
  ensureBound();
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function getSnapshot(): PluginUiState {
  ensureBound();
  return state;
}

/** Everything plugins show right now. */
export function usePluginUi(): PluginUiState {
  return useSyncExternalStore(subscribe, getSnapshot);
}

/** One plugin's tree for one slot, or `undefined` when it shows nothing there. */
export function usePluginTree(plugin: string, slot: PluginSlot): readonly PluginNode[] | undefined {
  return usePluginUi().trees.get(plugin)?.get(slotKey(slot));
}
