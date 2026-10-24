import type { Language } from "../api";

export const SUPPORTED_LANGUAGES = ["en", "es", "de"] as const satisfies readonly Language[];

export type TranslationTree = { [key: string]: string | TranslationTree };
export type Resources = Record<Language, Record<string, TranslationTree>>;

// Every `locales/<language>/<namespace>.json` is picked up automatically, so feature tasks only
// add files. Eager loading keeps all three languages available for switching without restart.
const files = import.meta.glob<TranslationTree>("../locales/*/*.json", {
  eager: true,
  import: "default",
});

function buildResources(): Resources {
  const resources: Resources = { en: {}, es: {}, de: {} };
  for (const [path, tree] of Object.entries(files)) {
    const match = /\/locales\/(\w+)\/([\w-]+)\.json$/.exec(path);
    const language = SUPPORTED_LANGUAGES.find((lang) => lang === match?.[1]);
    const namespace = match?.[2];
    if (!language || !namespace) throw new Error(`Unexpected translation file ${path}`);
    resources[language][namespace] = tree;
  }
  return resources;
}

export const resources = buildResources();
export const namespaces = Object.keys(resources.en);
