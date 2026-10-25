import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import type { Language } from "../api";
import { osLanguages, resolveLanguage } from "./language";
import { english, loadBundles, namespaces } from "./resources";

export { detectOsLanguage, resolveLanguage, osLanguages } from "./language";
export { ENDONYMS, SUPPORTED_LANGUAGES } from "./resources";

// Plurals: i18next picks the suffix with Intl.PluralRules for the active language, so translators
// add one key per CLDR plural category of their language: `_one` and `_other` everywhere, plus
// `_few` (cs, ga, hr, lt, mt, pl, ro, sk, sl), `_two` (ga, mt, sl), `_many` (cs, es, fr, ga,
// it, lt, mt, pl, pt, sk) and `_zero` (lv). i18n.test.ts names exactly which forms are missing.
//
// Starts in English synchronously so the module is usable at import time; the entry points wait
// for `initialLanguage` before their first render, so nobody sees English flash by.
void i18n.use(initReactI18next).init({
  resources: { en: english },
  ns: namespaces,
  defaultNS: "common",
  lng: "en",
  fallbackLng: "en",
  interpolation: { escapeValue: false },
  initAsync: false,
  returnNull: false,
});
document.documentElement.lang = i18n.language;

let requested: Language = "en";

/**
 * Switches the UI language at runtime, fetching its translations first, and keeps <html lang> in
 * sync for screen readers. When the user picks again while a download is in flight, only the
 * latest choice is applied.
 */
export async function applyLanguage(language: Language): Promise<void> {
  requested = language;
  if (!i18n.hasResourceBundle(language, "common")) {
    const bundles = await loadBundles(language);
    for (const [namespace, tree] of Object.entries(bundles)) {
      i18n.addResourceBundle(language, namespace, tree, true, true);
    }
  }
  if (requested !== language) return;
  if (i18n.language !== language) await i18n.changeLanguage(language);
  document.documentElement.lang = language;
}

/** The OS language, loaded; never rejects so the app still renders (in English) on failure. */
export const initialLanguage: Promise<void> = applyLanguage(
  resolveLanguage(null, osLanguages()),
).catch((error: unknown) => {
  console.error("Loading the UI language failed", error);
});

export { i18n };
