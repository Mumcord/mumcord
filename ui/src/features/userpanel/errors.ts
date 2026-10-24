import type { TFunction } from "i18next";
import { EngineError } from "../../api";

/** Plain-language text for a failed command; `message` of the server is only shown for denials. */
export function describeError(t: TFunction<"userpanel">, error: unknown): string {
  if (error instanceof EngineError) {
    switch (error.code) {
      case "messageTooLarge":
        return t("errors.tooLarge");
      case "permissionDenied":
        return error.message
          ? t("errors.deniedWithReason", { reason: error.message })
          : t("errors.denied");
      case "notConnected":
        return t("errors.notConnected");
      case "io":
      case "notFound":
        return t("errors.file");
      default:
        return t("errors.generic");
    }
  }
  console.error("Unexpected user panel error", error);
  return t("errors.generic");
}
