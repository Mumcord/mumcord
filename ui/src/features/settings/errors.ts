import type { TFunction } from "i18next";
import { toEngineError } from "../../api";

/** Plain-language text for a failed engine call; the technical detail goes to the console. */
export function errorText(t: TFunction<"settings">, error: unknown): string {
  const { code } = toEngineError(error);
  switch (code) {
    case "secureStorageUnavailable":
      return t("errors.secureStorage");
    case "io":
      return t("errors.io");
    case "invalidInput":
      return t("errors.invalidInput");
    case "notFound":
      return t("errors.notFound");
    default:
      return t("errors.generic");
  }
}
