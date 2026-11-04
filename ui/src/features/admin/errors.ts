/** Plain-language text for what the ban list could not do (FR-10.59, FR-10.60). */
import type { TFunction } from "i18next";
import type { EngineError } from "../../api";

export function errorText(t: TFunction<"admin">, error: EngineError): string {
  switch (error.code) {
    case "permissionDenied":
      return t("errors.permissionDenied");
    case "serverUnreachable":
      return t("errors.noAnswer");
    case "notConnected":
      return t("errors.disconnected");
    case "invalidInput":
      return t("errors.invalidInput");
    default:
      // An unknown reason shows the server's own text, sanitized (FR-10.60).
      return error.message;
  }
}

/**
 * The wording an action on a person needs: a refusal names the right that was missing, and the
 * engine's own reason stands in for anything unmapped (FR-10.60).
 */
export function actionErrorText(t: TFunction<"admin">, error: EngineError): string {
  switch (error.code) {
    case "permissionDenied":
      return t("errors.actionDenied");
    case "serverUnreachable":
      return t("errors.noAnswer");
    case "notConnected":
      return t("errors.disconnected");
    default:
      return error.message === "" ? t("errors.invalidInput") : error.message;
  }
}
