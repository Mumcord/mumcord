import type { Language } from "../api";

/** The 24 official EU languages (FR-1.36), ISO 639-1 codes as stored in the settings. */
export const SUPPORTED_LANGUAGES = [
  "bg",
  "cs",
  "da",
  "de",
  "el",
  "en",
  "es",
  "et",
  "fi",
  "fr",
  "ga",
  "hr",
  "hu",
  "it",
  "lt",
  "lv",
  "mt",
  "nl",
  "pl",
  "pt",
  "ro",
  "sk",
  "sl",
  "sv",
] as const satisfies readonly Language[];

/** Each language's name in that language, so everyone finds theirs in the picker. */
export const ENDONYMS: Record<Language, string> = {
  bg: "Български",
  cs: "Čeština",
  da: "Dansk",
  de: "Deutsch",
  el: "Ελληνικά",
  en: "English",
  es: "Español",
  et: "Eesti",
  fi: "Suomi",
  fr: "Français",
  ga: "Gaeilge",
  hr: "Hrvatski",
  hu: "Magyar",
  it: "Italiano",
  lt: "Lietuvių",
  lv: "Latviešu",
  mt: "Malti",
  nl: "Nederlands",
  pl: "Polski",
  pt: "Português",
  ro: "Română",
  sk: "Slovenčina",
  sl: "Slovenščina",
  sv: "Svenska",
};

/** Each language's English name, so the picker also finds "German" for Deutsch. */
export const ENGLISH_NAMES: Record<Language, string> = {
  bg: "Bulgarian",
  cs: "Czech",
  da: "Danish",
  de: "German",
  el: "Greek",
  en: "English",
  es: "Spanish",
  et: "Estonian",
  fi: "Finnish",
  fr: "French",
  ga: "Irish",
  hr: "Croatian",
  hu: "Hungarian",
  it: "Italian",
  lt: "Lithuanian",
  lv: "Latvian",
  mt: "Maltese",
  nl: "Dutch",
  pl: "Polish",
  pt: "Portuguese",
  ro: "Romanian",
  sk: "Slovak",
  sl: "Slovenian",
  sv: "Swedish",
};

export type TranslationTree = { [key: string]: string | TranslationTree };
export type Bundles = Record<string, TranslationTree>;

// Every `locales/<language>/<namespace>.json` is picked up automatically, so feature tasks only
// add files. English is bundled into the start-up code because it is the fallback for missing
// keys; the other 23 languages are separate chunks fetched on demand, so neither the main window
// nor the in-game overlay parses two megabytes of translations it never shows.
const englishFiles = import.meta.glob<TranslationTree>("../locales/en/*.json", {
  eager: true,
  import: "default",
});
const otherFiles = import.meta.glob<TranslationTree>(
  ["../locales/*/*.json", "!../locales/en/*.json"],
  { import: "default" },
);

function parsePath(path: string): { language: Language; namespace: string } {
  const match = /\/locales\/(\w+)\/([\w-]+)\.json$/.exec(path);
  const language = SUPPORTED_LANGUAGES.find((lang) => lang === match?.[1]);
  const namespace = match?.[2];
  if (!language || !namespace) throw new Error(`Unexpected translation file ${path}`);
  return { language, namespace };
}

export const english: Bundles = Object.fromEntries(
  Object.entries(englishFiles).map(([path, tree]) => [parsePath(path).namespace, tree]),
);
export const namespaces = Object.keys(english);

/** All namespaces of one language; a language without files yields none (English fills in). */
export async function loadBundles(language: Language): Promise<Bundles> {
  if (language === "en") return english;
  const entries = Object.entries(otherFiles).filter(
    ([path]) => parsePath(path).language === language,
  );
  return Object.fromEntries(
    await Promise.all(
      entries.map(async ([path, load]) => [parsePath(path).namespace, await load()] as const),
    ),
  );
}
