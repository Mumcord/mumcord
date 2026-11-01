import type { SearchableOption } from "../components/searchableOptions";
import { ENDONYMS, ENGLISH_NAMES, SUPPORTED_LANGUAGES } from "./resources";

const collator = new Intl.Collator("en");
const LANGUAGES = [...SUPPORTED_LANGUAGES].sort((a, b) =>
  collator.compare(ENDONYMS[a], ENDONYMS[b]),
);

/** Each language under its own name, found by that name, its English name or its code. */
export const LANGUAGE_OPTIONS: readonly SearchableOption[] = LANGUAGES.map((code) => ({
  value: code,
  label: ENDONYMS[code],
  lang: code,
  keywords: [ENGLISH_NAMES[code], code],
}));
