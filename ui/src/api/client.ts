/**
 * Typed contract between the UI and the engine, derived from the generated bindings so it
 * can never drift from the Rust side. See docs/engine-api.md for the meaning of each call.
 */
import type { commands, events } from "../bindings/commands";
import type { ApiError, ErrorCode } from "../bindings/commands";

type GeneratedCommands = typeof commands;
type GeneratedEvents = typeof events;

/** Command result as produced by the generated bindings. */
export type CommandResult<T> = { status: "ok"; data: T } | { status: "error"; error: ApiError };

/** The data of a fallible command, or the plain value of a command that cannot fail. */
type OkData<R> = R extends { status: "error"; error: ApiError }
  ? never
  : R extends { status: "ok"; data: infer T }
    ? T
    : R;

/** All engine commands; they resolve with the data or reject with an {@link EngineError}. */
export type EngineCommands = {
  [K in keyof GeneratedCommands]: (
    ...args: Parameters<GeneratedCommands[K]>
  ) => Promise<OkData<Awaited<ReturnType<GeneratedCommands[K]>>>>;
};

export type CommandName = keyof EngineCommands;
export type EventName = keyof GeneratedEvents;

/** Payload type of each engine event. */
export type EventPayloads = {
  [K in EventName]: Parameters<Parameters<GeneratedEvents[K]["listen"]>[0]>[0]["payload"];
};

/** Events that belong to one server; their payload carries its `serverId`. */
export type ServerEventName = {
  [K in EventName]: EventPayloads[K] extends { serverId: number } ? K : never;
}[EventName];

export type Unsubscribe = () => void;

export interface MumcordApi extends EngineCommands {
  /** Which implementation is active; useful for diagnostics and tests. */
  readonly kind: "tauri" | "mock";
  /** Subscribes to an engine event; call the returned function to stop listening. */
  on<K extends EventName>(event: K, handler: (payload: EventPayloads[K]) => void): Unsubscribe;
}

/** Error thrown by every failing command. */
export class EngineError extends Error {
  readonly code: ErrorCode;

  constructor(code: ErrorCode, message: string) {
    super(message);
    this.name = "EngineError";
    this.code = code;
  }
}

function isApiError(value: unknown): value is ApiError {
  return (
    typeof value === "object" &&
    value !== null &&
    "code" in value &&
    typeof value.code === "string" &&
    "message" in value &&
    typeof value.message === "string"
  );
}

/**
 * Normalises anything a failed call produced. Framework failures (unknown command, bad
 * argument) reject with a plain string or Error instead of an ApiError.
 */
export function toEngineError(value: unknown): EngineError {
  if (value instanceof EngineError) return value;
  if (isApiError(value)) return new EngineError(value.code, value.message);
  if (value instanceof Error) return new EngineError("internal", value.message);
  return new EngineError("internal", String(value));
}

/** Whether a generated command answered with a result wrapper rather than a plain value. */
export function isCommandResult(value: unknown): value is CommandResult<unknown> {
  return (
    typeof value === "object" &&
    value !== null &&
    "status" in value &&
    ((value.status === "ok" && "data" in value) || (value.status === "error" && "error" in value))
  );
}

export function unwrap<T>(result: CommandResult<T>): T {
  if (result.status === "error") throw toEngineError(result.error);
  return result.data;
}
