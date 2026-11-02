import { toEngineError } from "../../../api";

/**
 * The engine's refusal as plain text. The wording comes from the engine (it knows whether the
 * address was insecure, unreachable, signed with another key …), so it is shown as it is.
 */
export function repositoryProblem(error: unknown): string {
  return toEngineError(error).message;
}
