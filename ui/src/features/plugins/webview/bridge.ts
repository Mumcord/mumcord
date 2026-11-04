/**
 * The message bridge between a plugin's sealed web view and the plugin (FR-6.47). The page
 * sends with `window.parent.postMessage(string, "*")` and receives the plugin's `post` calls as
 * `message` events. Only strings from this exact frame pass, at most 16 KiB each and 60 per
 * second; the Rust side checks the same limits again. Message texts are never logged.
 *
 * Gestures (FR-9.45): once the page has loaded, the bridge hands the page's bootstrap script a
 * random key that the page's own code never sees. The bootstrap reports genuine clicks and key
 * presses with that key; a report without it is page code pretending and never counts.
 */

/** One bridge message, in UTF-8 bytes. */
export const MAX_MESSAGE_BYTES = 16 * 1024;
/** Bridge messages per second, in each direction. */
export const MAX_MESSAGES_PER_SECOND = 60;
/** Posts that wait for the page to finish loading; more than a second's worth are dropped. */
const MAX_QUEUED = MAX_MESSAGES_PER_SECOND;

const encoder = new TextEncoder();

/** A token bucket of {@link MAX_MESSAGES_PER_SECOND}, starting full. */
export function rateLimiter(now: () => number = () => performance.now()): () => boolean {
  let tokens = MAX_MESSAGES_PER_SECOND;
  let last = now();
  return () => {
    const t = now();
    tokens = Math.min(
      MAX_MESSAGES_PER_SECOND,
      tokens + ((t - last) / 1000) * MAX_MESSAGES_PER_SECOND,
    );
    last = t;
    if (tokens < 1) return false;
    tokens -= 1;
    return true;
  };
}

/** Whether `data` is a bridge message the plugin may get. */
export function fitsBridge(data: unknown): data is string {
  return typeof data === "string" && encoder.encode(data).length <= MAX_MESSAGE_BYTES;
}

/** Whether `data` is the Escape signal of the page's bootstrap script (served by Rust). */
export function isEscapeSignal(data: unknown): boolean {
  return (
    typeof data === "object" &&
    data !== null &&
    (data as { mumcordWebview?: unknown }).mumcordWebview === "escape"
  );
}

/** A fresh random key for one frame's gesture reports (128 bits, hex). */
export function newGestureKey(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

/** Whether `data` is a gesture report of the bootstrap that holds `key`. */
export function isGestureReport(data: unknown, key: string): boolean {
  if (typeof data !== "object" || data === null) return false;
  const report = data as { mumcordWebview?: unknown; key?: unknown };
  return report.mumcordWebview === "gesture" && report.key === key;
}

function isGestureShaped(data: unknown): boolean {
  return (
    typeof data === "object" &&
    data !== null &&
    (data as { mumcordWebview?: unknown }).mumcordWebview === "gesture"
  );
}

export interface BridgeOptions {
  /** The frame's window right now (`null` while it is not mounted). */
  readonly frame: () => Window | null;
  /** Hands a page message to the plugin. */
  readonly toPlugin: (text: string) => void;
  /** The page asked to give keyboard focus back to the app. */
  readonly onEscape: () => void;
  /** This frame's gesture key (see {@link newGestureKey}); keep it for the frame's lifetime. */
  readonly gestureKey: string;
  /** The user really clicked or pressed a key in the visible page (FR-9.45). */
  readonly onGesture: () => void;
  /** Called once for every dropped page message (counts only). */
  readonly onDropped?: () => void;
  readonly now?: () => number;
}

export interface Bridge {
  /** Feeds a `message` event of the app window. */
  readonly receive: (event: MessageEvent) => void;
  /** Sends a plugin message to the page (queued until {@link Bridge.loaded}). */
  readonly post: (message: string) => void;
  /** The page finished loading: the gesture key and then queued posts go out. */
  readonly loaded: () => void;
}

export function createBridge(options: BridgeOptions): Bridge {
  const allow = rateLimiter(options.now);
  let ready = false;
  const queue: string[] = [];

  const send = (message: string) => {
    // The frame has an opaque origin, so "*" is the only target that reaches it; the window
    // itself is the one this bridge belongs to.
    options.frame()?.postMessage(message, "*");
  };

  return {
    receive: (event) => {
      const frame = options.frame();
      if (frame === null || event.source !== frame) return;
      if (isEscapeSignal(event.data)) {
        options.onEscape();
        return;
      }
      if (isGestureShaped(event.data)) {
        // A hidden app window gets no real clicks, so a report then is not the user's.
        if (
          isGestureReport(event.data, options.gestureKey) &&
          document.visibilityState === "visible"
        )
          options.onGesture();
        return;
      }
      if (fitsBridge(event.data) && allow()) options.toPlugin(event.data);
      else options.onDropped?.();
    },
    post: (message) => {
      if (ready) send(message);
      else if (queue.length < MAX_QUEUED) queue.push(message);
    },
    loaded: () => {
      ready = true;
      options.frame()?.postMessage({ mumcordWebview: "key", key: options.gestureKey }, "*");
      for (const message of queue.splice(0)) send(message);
    },
  };
}
