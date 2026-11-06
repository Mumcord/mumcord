/**
 * The message bridge between a plugin's sealed web view and the plugin (FR-6.47). The page
 * sends with `window.parent.postMessage(string, "*")` and receives the plugin's `post` calls as
 * `message` events. Only strings from this exact frame pass, at most 16 KiB each and 60 per
 * second; the Rust side checks the same limits again. Message texts are never logged.
 *
 * Gestures (FR-9.45): once the page has loaded, the bridge hands the page's bootstrap script a
 * random key that the page's own code never sees. The bootstrap reports genuine clicks and key
 * presses with that key; a report without it is page code pretending and never counts.
 *
 * The bootstrap's other signals (heartbeat, Escape, F6, sound) are objects too, so a page can
 * never smuggle them in as bridge strings; faking one only hurts the page itself.
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

/**
 * Wraps `send` so each call starts only after the previous one settled. Every backend call may run
 * on its own thread, so two messages sent at once could reach the plugin in either order (a probe's
 * "running" after its result); one frame's messages must arrive in the order the page sent them.
 */
export function inOrder<T>(send: (value: T) => Promise<unknown>): (value: T) => void {
  const waiting: T[] = [];
  let busy = false;
  const next = () => {
    busy = waiting.length > 0;
    if (!busy) return;
    send(waiting.shift() as T).then(next, next);
  };
  return (value) => {
    waiting.push(value);
    if (!busy) next();
  };
}

/** Whether `data` is a bridge message the plugin may get. */
export function fitsBridge(data: unknown): data is string {
  return typeof data === "string" && encoder.encode(data).length <= MAX_MESSAGE_BYTES;
}

/** The `mumcordWebview` kind of a bootstrap signal (served by Rust), or `null` for others. */
export function signalKind(data: unknown): string | null {
  if (typeof data !== "object" || data === null) return null;
  const kind = (data as { mumcordWebview?: unknown }).mumcordWebview;
  return typeof kind === "string" ? kind : null;
}

/** A fresh random key for one frame's gesture reports (128 bits, hex). */
export function newGestureKey(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

/** Whether `data` is a gesture report of the bootstrap that holds `key`. */
export function isGestureReport(data: unknown, key: string): boolean {
  return signalKind(data) === "gesture" && (data as { key?: unknown }).key === key;
}

/** Where the bridge posts to: the iframe's window, or a child web view's stand-in (macOS). */
export interface FrameTarget {
  postMessage(message: unknown, targetOrigin: string): void;
}

export interface BridgeOptions {
  /** The frame's window right now (`null` while it is not mounted). */
  readonly frame: () => FrameTarget | null;
  /** Hands a page message to the plugin. */
  readonly toPlugin: (text: string) => void;
  /** Escape inside the page: focus goes back to the app (and a dialog closes, FR-9.41). */
  readonly onEscape: () => void;
  /** F6 inside the page: focus goes back to the app only. */
  readonly onF6?: () => void;
  /** The page's once-a-second sign of life (FR-9.42). */
  readonly onHeartbeat?: () => void;
  /** The page started playing sound after a click inside it (FR-9.43). */
  readonly onSound?: () => void;
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
  /** Feeds a message already known to come from this frame (a child web view's event). */
  readonly receiveData: (data: unknown) => void;
  /** Sends a plugin message to the page (queued until {@link Bridge.loaded}). */
  readonly post: (message: string) => void;
  /** The page finished loading: the gesture key and then queued posts go out. */
  readonly loaded: () => void;
  /** Mutes or unmutes the page's sound (mute button, deafened); sent at once when loaded. */
  readonly mute: (muted: boolean) => void;
}

export function createBridge(options: BridgeOptions): Bridge {
  const allow = rateLimiter(options.now);
  let ready = false;
  let muted: boolean | null = null;
  const queue: string[] = [];

  const send = (message: string) => {
    // The frame has an opaque origin, so "*" is the only target that reaches it; the window
    // itself is the one this bridge belongs to.
    options.frame()?.postMessage(message, "*");
  };

  const receiveData = (data: unknown) => {
    switch (signalKind(data)) {
      case null:
        break;
      case "escape":
        options.onEscape();
        return;
      case "f6":
        options.onF6?.();
        return;
      case "heartbeat":
        options.onHeartbeat?.();
        return;
      case "sound":
        options.onSound?.();
        return;
      case "gesture":
        // A hidden app window gets no real clicks, so a report then is not the user's.
        if (isGestureReport(data, options.gestureKey) && document.visibilityState === "visible")
          options.onGesture();
        return;
      default:
        // An unknown signal is never a bridge message (those are strings).
        options.onDropped?.();
        return;
    }
    if (fitsBridge(data) && allow()) options.toPlugin(data);
    else options.onDropped?.();
  };

  return {
    receive: (event) => {
      const frame = options.frame();
      if (frame === null || event.source !== frame) return;
      receiveData(event.data);
    },
    receiveData,
    post: (message) => {
      if (ready) send(message);
      else if (queue.length < MAX_QUEUED) queue.push(message);
    },
    loaded: () => {
      ready = true;
      options.frame()?.postMessage({ mumcordWebview: "key", key: options.gestureKey }, "*");
      for (const message of queue.splice(0)) send(message);
      if (muted !== null) options.frame()?.postMessage({ mumcordWebview: "mute", muted }, "*");
    },
    mute: (next) => {
      muted = next;
      if (ready) options.frame()?.postMessage({ mumcordWebview: "mute", muted }, "*");
    },
  };
}
