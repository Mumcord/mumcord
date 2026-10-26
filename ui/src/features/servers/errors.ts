import type { TFunction } from "i18next";
import type { ApiError, ErrorCode } from "../../api";

/** Errors where editing the saved server is the next step. */
export const EDIT_FIXES: readonly ErrorCode[] = ["wrongPassword", "nameInUse", "invalidInput"];

/** Refusal over the connected-servers limit, naming the limit when known (FR-2.1, FR-2.7). */
export function serverLimitText(
  t: TFunction<"servers">,
  limit: number | null,
  alwaysConnect = false,
): string {
  if (alwaysConnect) {
    return limit === null
      ? t("errors.alwaysConnectLimitUnknown")
      : t("errors.alwaysConnectLimit", { limit });
  }
  return limit === null ? t("errors.serverLimitUnknown") : t("errors.serverLimit", { limit });
}

/**
 * Plain-language explanation with a next step for why a connection failed or ended. `limit` is
 * the connected-servers limit for a `serverLimitReached` refusal.
 */
export function connectionErrorText(
  t: TFunction<"servers">,
  error: ApiError,
  limit: number | null = null,
): string {
  switch (error.code) {
    case "serverLimitReached":
      return serverLimitText(t, limit);
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
