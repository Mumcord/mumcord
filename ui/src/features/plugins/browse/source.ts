import type { TFunction } from "i18next";
import type { IndexSource } from "../../../api";

/** "Official" or "Unofficial · <repository>" (FR-6.61), shown on every card and prompt. */
export function sourceLabel(t: TFunction<"pluginBrowse">, source: IndexSource): string {
  return source.kind === "official" ? t("official") : t("unofficial", { repository: source.name });
}
