/**
 * The message bridge between a plugin's sealed web view and the plugin (FR-6.47). The page
 * sends with `window.parent.postMessage(string, "*")` and receives the plugin's `post` calls as
 * `message` events. Only strings from this exact frame pass, at most 16 KiB each and 60 per
 * second; the Rust side checks the same limits again. Message texts are never logged.
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

export interface BridgeOptions {
  /** The frame's window right now (`null` while it is not mounted). */
  readonly frame: () => Window | null;
  /** Hands a page message to the plugin. */
  readonly toPlugin: (text: string) => void;
  /** The page asked to give keyboard focus back to the app. */
  readonly onEscape: () => void;
  /** Called once for every dropped page message (counts only). */
  readonly onDropped?: () => void;
  readonly now?: () => number;
}

export interface Bridge {
  /** Feeds a `message` event of the app window. */
  readonly receive: (event: MessageEvent) => void;
  /** Sends a plugin message to the page (queued until {@link Bridge.loaded}). */
  readonly post: (message: string) => void;
  /** The page finished loading: queued posts go out. */
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
      if (fitsBridge(event.data) && allow()) options.toPlugin(event.data);
      else options.onDropped?.();
    },
    post: (message) => {
      if (ready) send(message);
      else if (queue.length < MAX_QUEUED) queue.push(message);
    },
    loaded: () => {
      ready = true;
      for (const message of queue.splice(0)) send(message);
    },
  };
}
