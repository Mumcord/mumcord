/** Real engine client: thin wrapper over the generated Tauri bindings. */
import { commands, events } from "../bindings/commands";
import {
  toEngineError,
  unwrap,
  type CommandResult,
  type EngineCommands,
  type EventName,
  type EventPayloads,
  type MumcordApi,
  type Unsubscribe,
} from "./client";

type AnyCommand = (...args: unknown[]) => Promise<CommandResult<unknown>>;

function wrap(fn: AnyCommand) {
  return async (...args: unknown[]): Promise<unknown> => {
    let result: CommandResult<unknown>;
    try {
      result = await fn(...args);
    } catch (err) {
      // The generated runtime rethrows genuine JS errors (e.g. IPC unavailable).
      throw toEngineError(err);
    }
    return unwrap(result);
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
  listen((e) => {
    handler(e.payload);
  }).then(
    (fn) => {
      if (stopped) fn();
      else unlisten = fn;
    },
    (err: unknown) => {
      console.error(`failed to listen to engine event ${event}`, err);
    },
  );
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
  return { ...wrapped, kind: "tauri", on };
}
