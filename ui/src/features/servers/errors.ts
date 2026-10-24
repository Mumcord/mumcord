import type { TFunction } from "i18next";
import type { ApiError, ErrorCode } from "../../api";

/** Errors where editing the saved server is the next step. */
export const EDIT_FIXES: readonly ErrorCode[] = ["wrongPassword", "nameInUse", "invalidInput"];

/** Plain-language explanation with a next step for why a connection failed or ended. */
export function connectionErrorText(t: TFunction<"servers">, error: ApiError): string {
  switch (error.code) {
    case "serverUnreachable":
      return t("errors.serverUnreachable");
    case "wrongPassword":
      return t("errors.wrongPassword");
    case "nameInUse":
      return t("errors.nameInUse");
    case "certificateRejected":
      return t("errors.certificateRejected");
    case "certificateChanged":
      return t("errors.certificateChanged");
    case "permissionDenied":
      return error.message
        ? t("errors.deniedWithReason", { reason: error.message })
        : t("errors.denied");
    case "invalidInput":
      return t("errors.invalidAddress");
    case "secureStorageUnavailable":
      return t("errors.secureStorage");
    case "notFound":
      return t("errors.notFound");
    default:
      return t("errors.generic");
  }
}
