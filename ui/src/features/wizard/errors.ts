import { EngineError, type ErrorCode } from "../../api";

/** Error code of a failed command, or `null` for anything that is not an engine error. */
export function errorCode(error: unknown): ErrorCode | null {
  return error instanceof EngineError ? error.code : null;
}
