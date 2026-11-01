import { useTranslation } from "react-i18next";
import type { PluginItem } from "../../notifications/centreModel";
import { markRead, setCentreOpen } from "../../notifications/centreStore";
import { requestPluginScreen } from "./pluginRequests";

export interface PluginEntryText {
  title: string;
  body: string;
  /** The second chip: what kind of entry it is. */
  where: string;
}

/**
 * The texts of a plugin entry. Titles and bodies written by plugins and servers are untrusted
 * plain text and go to the screen as text only.
 */
export function usePluginEntryText(item: PluginItem | null): PluginEntryText {
  const { t } = useTranslation("pluginNotifications");
  if (item === null) return { title: "", body: "", where: "" };
  const { notice } = item;
  const where = t("type");
  switch (notice.kind) {
    case "message":
      return { title: notice.title || notice.name, body: notice.title ? notice.body : "", where };
    case "autoOff":
      return {
        title: t("autoOff.title", { name: notice.name }),
        body: t("autoOff.body"),
        where,
      };
    case "update":
      return {
        title: t("update.title", { name: notice.name }),
        body: t("update.body", { version: notice.version }),
        where,
      };
    case "suggestion":
      return {
        title: t("suggestion.title", { name: notice.name }),
        body: notice.reason || t("suggestion.noReason"),
        where,
      };
  }
}

/** A click on the entry: marks it read and opens the screen that explains it. */
export function openPluginItem(item: PluginItem): void {
  markRead(item.id);
  const { notice } = item;
  if (notice.kind === "message") return;
  setCentreOpen(false);
  switch (notice.kind) {
    case "autoOff":
      requestPluginScreen({ kind: "installed", plugin: notice.plugin });
      return;
    case "update":
      requestPluginScreen({ kind: "updates" });
      return;
    case "suggestion":
      if (notice.source.kind === "official") {
        requestPluginScreen({ kind: "details", plugin: notice.plugin });
      }
      return;
  }
}
