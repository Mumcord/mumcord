/**
 * Asks the plugin screens (Settings → Plugins, the store, the install prompt, the Add repository
 * dialog) to open something. The notification centre only knows what the user wants; the shell
 * that owns those screens answers. A request that arrives while no screen listens waits for the
 * next one that subscribes, so the shell can mount after the click.
 */

export type PluginRequest =
  /** Official plugin: open its details page in the store. */
  | { kind: "details"; plugin: string }
  /** Official plugin: open the install prompt (the user sees every permission first). */
  | { kind: "install"; plugin: string }
  /** Third-party plugin: open the Add repository dialog with the announced key as Expected key. */
  | { kind: "addRepository"; url: string; key: string | null }
  /** Open the Updates tab. */
  | { kind: "updates" }
  /** Open an installed plugin in Settings → Plugins. */
  | { kind: "installed"; plugin: string };

type Listener = (request: PluginRequest) => void;

const listeners = new Set<Listener>();
let waiting: PluginRequest | null = null;

export function requestPluginScreen(request: PluginRequest): void {
  if (listeners.size === 0) {
    waiting = request;
    return;
  }
  for (const listener of listeners) listener(request);
}

/** The plugin shell subscribes while it is mounted; a waiting request is delivered at once. */
export function subscribePluginRequests(listener: Listener): () => void {
  listeners.add(listener);
  if (waiting !== null) {
    const request = waiting;
    waiting = null;
    listener(request);
  }
  return () => {
    listeners.delete(listener);
  };
}
