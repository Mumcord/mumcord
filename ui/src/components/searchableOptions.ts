export interface SearchableOption {
  value: string;
  label: string;
  /** BCP 47 language of the label, so screen readers pronounce an endonym correctly. */
  lang?: string;
  /** Other words that find this option, e.g. a language's English name and its code. */
  keywords?: readonly string[];
}

/** Lower case without accents, so "cestina" finds "Čeština". */
function fold(text: string): string {
  return text.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase().trim();
}

/**
 * Options whose label or keywords contain the query, all of them for an empty query. Options with
 * a word that starts with the query come first, so "de" lists Deutsch before Nederlands.
 */
export function filterOptions<T extends SearchableOption>(
  options: readonly T[],
  query: string,
): T[] {
  const wanted = fold(query);
  if (wanted === "") return [...options];
  const prefix: T[] = [];
  const inside: T[] = [];
  for (const option of options) {
    const texts = [option.label, ...(option.keywords ?? [])].map(fold);
    if (texts.some((text) => text.split(/\s+/).some((word) => word.startsWith(wanted))))
      prefix.push(option);
    else if (texts.some((text) => text.includes(wanted))) inside.push(option);
  }
  return [...prefix, ...inside];
}
