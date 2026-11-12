/**
 * Plugin frames as child web views (macOS, FR-9.3). S0 found that a busy plugin page in an
 * iframe freezes the whole UI on macOS, while a web view of its own next to the UI stays smooth.
 * There, `PluginFrame` draws a placeholder where the iframe would be, and the frame's own web
 * view is laid over it and follows it: resizes, scrolling, tab switches and layout changes.
 *
 * The child web view is always drawn above the UI, so it is hidden whenever its placeholder is
 * not on screen: in a hidden tab or panel, scrolled away, or under a modal dialog of the app.
 *
 * Messages take the same shapes as with an iframe (see `bridge.ts`); only the transport differs:
 * the page's posts arrive as `pluginFrame` events, and the bridge's posts go out with
 * `pluginFramePost` (see `src-tauri/src/plugins/child_webview.rs`).
 */
import {
  getApi,
  type PluginFrameBounds,
  type PluginFrameChange,
  type PluginFrameDelivery,
  type Unsubscribe,
} from "../../../api";
import { signalKind } from "./bridge";

/** A rectangle in CSS pixels, as `getBoundingClientRect` gives it. */
export interface Box {
  readonly left: number;
  readonly top: number;
  readonly right: number;
  readonly bottom: number;
}

/**
 * Whether plugin frames are child web views here: the real app on macOS. The system web view
 * says so in its user agent, which is known at once, so no frame ever starts as an iframe there.
 */
export function childFramesUsed(): boolean {
  return getApi().kind === "tauri" && /Macintosh|Mac OS X/.test(navigator.userAgent);
}

/** A fresh frame id: 64 random bits as hex (the Rust side accepts 16–32 hex digits). */
export function newFrameId(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(8));
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

/**
 * The part of `rect` inside every clip, in device pixels (`ratio` is the device pixel ratio),
 * or `null` when nothing of it shows. Edges are rounded on their own, so neighbouring frames
 * never overlap or leave a gap.
 */
export function visibleBounds(
  rect: Box,
  clips: readonly Box[],
  ratio: number,
): PluginFrameBounds | null {
  let { left, top, right, bottom } = rect;
  for (const clip of clips) {
    left = Math.max(left, clip.left);
    top = Math.max(top, clip.top);
    right = Math.min(right, clip.right);
    bottom = Math.min(bottom, clip.bottom);
  }
  const x = Math.round(left * ratio);
  const y = Math.round(top * ratio);
  const width = Math.round(right * ratio) - x;
  const height = Math.round(bottom * ratio) - y;
  return width >= 1 && height >= 1 ? { x, y, width, height } : null;
}

export function sameBounds(a: PluginFrameBounds | null, b: PluginFrameBounds | null): boolean {
  if (a === null || b === null) return a === b;
  return a.x === b.x && a.y === b.y && a.width === b.width && a.height === b.height;
}

/** Whether a modal dialog of the app lies above `element` (the web view would cover it). */
function underModal(element: Element): boolean {
  for (const modal of document.querySelectorAll("dialog[open], [aria-modal='true']")) {
    if (modal.contains(element)) continue;
    if (modal instanceof HTMLDialogElement && !modal.matches(":modal")) continue;
    return true;
  }
  return false;
}

/** Where `element`'s web view belongs right now, or `null` when it must be hidden. */
export function measurePlacement(element: HTMLElement): PluginFrameBounds | null {
  if (!element.isConnected || document.visibilityState !== "visible") return null;
  if (typeof element.checkVisibility === "function" && !element.checkVisibility()) return null;
  if (underModal(element)) return null;
  const clips: Box[] = [{ left: 0, top: 0, right: window.innerWidth, bottom: window.innerHeight }];
  for (let parent = element.parentElement; parent !== null; parent = parent.parentElement) {
    const style = getComputedStyle(parent);
    if (style.overflowX !== "visible" || style.overflowY !== "visible") {
      clips.push(parent.getBoundingClientRect());
    }
  }
  return visibleBounds(element.getBoundingClientRect(), clips, window.devicePixelRatio || 1);
}

export interface Scheduler {
  readonly request: (callback: () => void) => number;
  readonly cancel: (handle: number) => void;
}

const animationFrames: Scheduler = {
  request: (callback) => requestAnimationFrame(callback),
  cancel: (handle) => {
    cancelAnimationFrame(handle);
  },
};

/**
 * Measures `element` on every animation frame and reports each change of its placement (the
 * first measurement always). Layout moves without a resize (a panel opening next to it) and
 * scrolling anywhere are caught alike; nothing is sent while nothing changes.
 *
 * A placement is reported again when the window's height changed, even if it is the same: the
 * system keeps a child web view at its distance from the window's bottom edge, so a frame whose
 * place is fixed from the top would otherwise drift up or down by the height change.
 */
export function trackPlacement(
  element: HTMLElement,
  onChange: (bounds: PluginFrameBounds | null) => void,
  measure: (element: HTMLElement) => PluginFrameBounds | null = measurePlacement,
  scheduler: Scheduler = animationFrames,
  windowHeight: () => number = () => window.innerHeight,
): () => void {
  let last: PluginFrameBounds | null | undefined;
  let lastHeight = windowHeight();
  let handle = 0;
  const tick = () => {
    const next = measure(element);
    const height = windowHeight();
    if (last === undefined || !sameBounds(last, next) || (next !== null && height !== lastHeight)) {
      last = next;
      onChange(next);
    }
    lastHeight = height;
    handle = scheduler.request(tick);
  };
  tick();
  return () => {
    scheduler.cancel(handle);
  };
}

/** A bridge post as the Rust side takes it; `null` for anything the bridge never sends. */
export function toDelivery(data: unknown): PluginFrameDelivery | null {
  if (typeof data === "string") return { type: "text", text: data };
  const fields = data as { key?: unknown; muted?: unknown };
  switch (signalKind(data)) {
    case "key":
      return typeof fields.key === "string" ? { type: "key", key: fields.key } : null;
    case "mute":
      return typeof fields.muted === "boolean" ? { type: "mute", muted: fields.muted } : null;
    default:
      return null;
  }
}

/** A page post from a child web view, shaped as the iframe's `message` event data would be. */
export function pagePost(change: Extract<PluginFrameChange, { type: "message" | "signal" }>) {
  if (change.type === "message") return change.text;
  return change.key === null
    ? { mumcordWebview: change.kind }
    : { mumcordWebview: change.kind, key: change.key };
}

const handlers = new Map<string, (change: PluginFrameChange) => void>();
let unsubscribe: Unsubscribe | null = null;

/**
 * Routes the `pluginFrame` events of `frame` to `handler` until the returned function is
 * called. Register before opening the frame: commands wait for listeners still registering,
 * so not even the first event can be missed.
 */
export function listenFrame(
  frame: string,
  handler: (change: PluginFrameChange) => void,
): () => void {
  handlers.set(frame, handler);
  unsubscribe ??= getApi().on("pluginFrame", (event) => {
    handlers.get(event.frame)?.(event.change);
  });
  return () => {
    handlers.delete(frame);
    if (handlers.size === 0) {
      unsubscribe?.();
      unsubscribe = null;
    }
  };
}
