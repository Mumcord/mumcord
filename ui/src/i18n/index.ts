import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import type { Language } from "../api";
import { osLanguages, resolveLanguage } from "./language";
import { namespaces, resources } from "./resources";

export { detectOsLanguage, resolveLanguage, osLanguages } from "./language";
export { SUPPORTED_LANGUAGES } from "./resources";

// Initialised synchronously with the OS language so the first render is already translated;
// the language from the settings record is applied once it has loaded.
void i18n.use(initReactI18next).init({
  resources,
  ns: namespaces,
  defaultNS: "common",
  lng: resolveLanguage(null, osLanguages()),
  fallbackLng: "en",
  interpolation: { escapeValue: false },
  initAsync: false,
  returnNull: false,
});
document.documentElement.lang = i18n.language;

/** Switches the UI language at runtime and keeps <html lang> in sync for screen readers. */
export async function applyLanguage(language: Language): Promise<void> {
  if (i18n.language !== language) await i18n.changeLanguage(language);
  document.documentElement.lang = language;
}

export { i18n };
