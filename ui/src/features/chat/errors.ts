import type { TFunction } from "i18next";
import { EngineError } from "../../api";

/**
 * Plain-language text for a failed send; the server's own reason is only shown for denials.
 * `tooLarge` is the text the composer shows for its own size check, so a server refusal reads
 * the same.
 */
export function describeSendError(t: TFunction<"chat">, error: unknown, tooLarge: string): string {
  if (error instanceof EngineError) {
    switch (error.code) {
      case "messageTooLarge":
        return tooLarge;
      case "permissionDenied":
        return error.message
          ? t("errors.deniedWithReason", { reason: error.message })
          : t("errors.denied");
      case "notConnected":
        return t("errors.notConnected");
      default:
        break;
    }
  }
  console.error("Sending a chat message failed", error);
  return t("errors.generic");
}
