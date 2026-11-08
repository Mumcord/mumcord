/** Real engine client: thin wrapper over the generated Tauri bindings. */
import { commands, events } from "../bindings/commands";
import {
  isCommandResult,
  toEngineError,
  unwrap,
  type EngineCommands,
  type EventName,
  type EventPayloads,
  type MumcordApi,
  type Unsubscribe,
} from "./client";

type AnyCommand = (...args: unknown[]) => Promise<unknown>;

/**
 * Listener registrations still on their way to the backend. Commands wait for them: a store
 * subscribes and then fetches the current state, and an event sent between the fetch and the
 * registration would otherwise be lost (e.g. a server connecting right at start).
 */
const registering = new Set<Promise<unknown>>();

function wrap(fn: AnyCommand) {
  return async (...args: unknown[]): Promise<unknown> => {
    if (registering.size > 0) await Promise.all(registering);
    let result: unknown;
    try {
      result = await fn(...args);
    } catch (err) {
      // The generated runtime rethrows genuine JS errors (e.g. IPC unavailable).
      throw toEngineError(err);
    }
    // Commands that cannot fail (e.g. `update_status`) answer with the plain value.
    return isCommandResult(result) ? unwrap(result) : result;
  };
}

function on<K extends EventName>(
  event: K,
  handler: (payload: EventPayloads[K]) => void,
): Unsubscribe {
  let stopped = false;
  let unlisten: (() => void) | undefined;
  // `listen` is generic per event; the mapped payload type matches by construction.
  const listen = events[event].listen as unknown as (
    cb: (e: { payload: EventPayloads[K] }) => void,
  ) => Promise<() => void>;
  const registered = listen((e) => {
    handler(e.payload);
  })
    .then(
      (fn) => {
        if (stopped) fn();
        else unlisten = fn;
      },
      (err: unknown) => {
        console.error(`failed to listen to engine event ${event}`, err);
      },
    )
    .finally(() => {
      registering.delete(registered);
    });
  registering.add(registered);
  return () => {
    stopped = true;
    unlisten?.();
  };
}

export function createTauriApi(): MumcordApi {
  // Each generated command has its own signature; the mapped EngineCommands type
  // describes the wrapped result exactly, which TS cannot infer through Object.entries.
  const wrapped = Object.fromEntries(
    Object.entries(commands).map(([name, fn]) => [name, wrap(fn as AnyCommand)]),
  ) as EngineCommands;
  return { ...wrapped, copyText: copyWithFallback(wrapped.copyText), kind: "tauri", on };
}

/**
 * Copies through the app's clipboard, and through the webview's when the system refused it
 * (issue #634): Windows can deny the app's clipboard access while the webview's still works.
 * The original error stays the answer when both fail.
 */
export function copyWithFallback(copy: EngineCommands["copyText"]): EngineCommands["copyText"] {
  return async (text) => {
    try {
      return await copy(text);
    } catch (error) {
      try {
        await navigator.clipboard.writeText(text);
        return null;
      } catch {
        throw error;
      }
    }
  };
}
