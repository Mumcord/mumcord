import type { TFunction } from "i18next";
import { EngineError } from "../../../api";

/**
 * The engine reports registry and verification failures with the fixed English texts of spec
 * §5.7; they are shown in the user's language here. Any other text is shown as it is.
 */
const KNOWN_TEXTS: Record<string, string> = {
  "Can't load the plugin list. Check your connection.": "errors.unreachable",
  "Showing the last known plugin list (offline)": "errors.offline",
  "Download didn't match the official list": "errors.mismatch",
  "Signature check failed": "errors.signature",
  "This plugin asks for permissions not listed in the repository": "errors.permissions",
};

/** The user-facing text of an engine message, translated when it is one of the §5.7 texts. */
export function pluginMessageText(t: TFunction<"pluginBrowse">, message: string): string {
  const key = KNOWN_TEXTS[message];
  return key === undefined ? message : t(key);
}

/** The text of a failed command, or `fallback` when the engine gave none. */
export function pluginErrorText(
  t: TFunction<"pluginBrowse">,
  error: unknown,
  fallback: string,
): string {
  return error instanceof EngineError && error.message !== ""
    ? pluginMessageText(t, error.message)
    : fallback;
}
