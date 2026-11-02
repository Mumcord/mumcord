import type { TFunction } from "i18next";
import type { IndexSource, InstalledPlugin } from "../../../api";

/** "Official" or "Unofficial · <repository>" (FR-6.61), shown on every card and prompt. */
export function sourceLabel(t: TFunction<"pluginBrowse">, source: IndexSource): string {
  return source.kind === "official" ? t("official") : t("unofficial", { repository: source.name });
}

/**
 * The one-time warning before the first install from a third-party repository (FR-6.61): shown
 * while no installed plugin comes from that repository.
 */
export function needsRepositoryWarning(
  source: IndexSource,
  installed: readonly InstalledPlugin[],
): boolean {
  if (source.kind !== "repo") return false;
  return !installed.some(
    (plugin) => plugin.source.kind === "repo" && plugin.source.repositoryId === source.repositoryId,
  );
}
