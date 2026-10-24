import type { Language } from "../api";
import { SUPPORTED_LANGUAGES } from "./resources";

/** First supported OS language by preference order (`de-AT` → `de`), else English. */
export function detectOsLanguage(osLanguages: readonly string[]): Language {
  for (const tag of osLanguages) {
    const base = tag.toLowerCase().split("-")[0];
    const supported = SUPPORTED_LANGUAGES.find((lang) => lang === base);
    if (supported) return supported;
  }
  return "en";
}

/** The `language` setting wins; `null` follows the OS. */
export function resolveLanguage(
  setting: Language | null,
  osLanguages: readonly string[],
): Language {
  return setting ?? detectOsLanguage(osLanguages);
}

export function osLanguages(): readonly string[] {
  return navigator.languages.length > 0 ? navigator.languages : [navigator.language];
}
