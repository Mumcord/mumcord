import type { IndexedPlugin } from "../../../api";

/** The manifest categories (FR-6.11), in the order of the filter. */
export const CATEGORIES = [
  "fun",
  "games",
  "chat",
  "voice",
  "accessibility",
  "looks",
  "tools",
] as const;
export type Category = (typeof CATEGORIES)[number] | "all";

/** Echolings gets the highlighted card at the top (FR-6.59). */
const HIGHLIGHTED = "echolings";

/** Filters by category and search text and puts the highlighted plugin first. */
export function visiblePlugins(
  plugins: readonly IndexedPlugin[],
  category: Category,
  search: string,
): IndexedPlugin[] {
  const needle = search.trim().toLocaleLowerCase();
  const matches = plugins.filter(
    (plugin) =>
      (category === "all" || plugin.categories.includes(category)) &&
      (needle === "" ||
        plugin.name.toLocaleLowerCase().includes(needle) ||
        plugin.summary.toLocaleLowerCase().includes(needle)),
  );
  const highlighted = matches.filter((plugin) => isHighlighted(plugin));
  return [...highlighted, ...matches.filter((plugin) => !isHighlighted(plugin))];
}

export function isHighlighted(plugin: IndexedPlugin): boolean {
  return plugin.id === HIGHLIGHTED && plugin.source.kind === "official";
}
