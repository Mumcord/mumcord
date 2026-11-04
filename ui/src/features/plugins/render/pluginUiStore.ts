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
  /** plugin id → place → its open web view there (FR-6.46; up to three places, FR-9.38). */
  readonly webviews: ReadonlyMap<string, ReadonlyMap<PluginWebviewPlace, PluginWebview>>;
  /** plugin id → the smallest size its side panel page asked for (FR-9.39). */
  readonly sizeHints?: ReadonlyMap<string, PluginWebviewSizeHint>;
  /** plugin id → the status line its side panel shows under the page (FR-9.39). */
  readonly sidePanelStatus?: ReadonlyMap<string, string>;
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

/** The minimum size, in CSS pixels, a plugin page asked for. */
export interface PluginWebviewSizeHint {
  readonly minWidth: number;
  readonly minHeight: number;
}

/** `webviews` with the plugin's frame in `place` set (`view`) or removed (`null`). */
function withWebview(
  webviews: PluginUiState["webviews"],
  plugin: string,
  place: PluginWebviewPlace,
  view: PluginWebview | null,
): PluginUiState["webviews"] {
  const places = new Map(webviews.get(plugin));
  if (view === null) places.delete(place);
  else places.set(place, view);
  const next = new Map(webviews);
  if (places.size === 0) next.delete(plugin);
  else next.set(plugin, places);
  return next;
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
      const webviews = withWebview(state.webviews, plugin, "dialog", null);
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
      const view = { place: change.place, url: change.url, seq: ++webviewSeq };
      return { ...state, webviews: withWebview(state.webviews, plugin, change.place, view) };
    }
    case "webviewHide": {
      if (!state.webviews.get(plugin)?.has(change.place)) return state;
      return { ...state, webviews: withWebview(state.webviews, plugin, change.place, null) };
    }
    // Bridge messages go straight to the open frame (see `webview/`), they are not state.
    case "webviewPost":
      return state;
    // Read by the side panel host for its smallest width.
    case "webviewSizeHint": {
      const sizeHints = new Map(state.sizeHints);
      sizeHints.set(plugin, { minWidth: change.minWidth, minHeight: change.minHeight });
      return { ...state, sizeHints };
    }
    // Opens a settings page (see `bind`); nothing to keep.
    case "openPermissions":
      return state;
    // The stage activity's tree under the key "stage"; empty once the user left it.
    case "stageTree":
      if (change.nodes.length === 0) slots.delete("stage");
      else slots.set("stage", change.nodes);
      trees.set(plugin, slots);
      return { ...state, trees };
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

/** Everything plugins show right now, outside React (e.g. for screen-reader announcements). */
export function getPluginUi(): PluginUiState {
  return getSnapshot();
}

/** Everything plugins show right now. */
export function usePluginUi(): PluginUiState {
  return useSyncExternalStore(subscribe, getSnapshot);
}

/** One plugin's tree for one slot, or `undefined` when it shows nothing there. */
export function usePluginTree(plugin: string, slot: PluginSlot): readonly PluginNode[] | undefined {
  return usePluginUi().trees.get(plugin)?.get(slotKey(slot));
}
