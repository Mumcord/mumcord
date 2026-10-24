import type { TFunction } from "i18next";
import { EngineError } from "../../api";

/** Plain-language text for a failed follow/unfollow. */
export function describeFollowError(t: TFunction<"follow">, error: unknown): string {
  if (error instanceof EngineError) {
    switch (error.code) {
      case "permissionDenied":
        return t("errors.denied");
      case "listenerLimitReached":
        return t("errors.limit");
      case "serverTooOld":
        return t("errors.tooOld");
      case "notConnected":
        return t("errors.notConnected");
      default:
        break;
    }
  }
  console.error("Following a channel failed", error);
  return t("errors.generic");
}
