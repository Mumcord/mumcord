import { useTranslation } from "react-i18next";
import type { PluginItem } from "../../notifications/centreModel";
import { markRead, setCentreOpen } from "../../notifications/centreStore";
import { requestPluginScreen } from "./pluginRequests";

export interface PluginEntryText {
  title: string;
  body: string;
  /** The chips after the server chip: what kind of entry it is, or where a suggestion comes from. */
  chips: PluginChip[];
}

export interface PluginChip {
  label: string;
  /** `accent` marks a recommendation, `ok` an official source, `warn` an unofficial one. */
  tone?: "accent" | "ok" | "warn";
}

/** The host of a repository address, which is all that is safe and short enough to show. */
function hostOf(url: string): string {
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
}

/**
 * The texts of a plugin entry. Titles and bodies written by plugins and servers are untrusted
 * plain text and go to the screen as text only.
 */
export function usePluginEntryText(item: PluginItem | null): PluginEntryText {
  const { t } = useTranslation("pluginNotifications");
  if (item === null) return { title: "", body: "", chips: [] };
  const { notice } = item;
  const chips: PluginChip[] = [{ label: t("type") }];
  switch (notice.kind) {
    case "message":
      return { title: notice.title || notice.name, body: notice.title ? notice.body : "", chips };
    case "autoOff":
      return {
        title: t("autoOff.title", { name: notice.name }),
        body: t("autoOff.body"),
        chips,
      };
    case "update":
      return {
        title: t("update.title", { name: notice.name }),
        body: t("update.body", { version: notice.version }),
        chips,
      };
    case "suggestion":
      return {
        title: t("suggestion.title", { name: notice.name }),
        body: notice.reason || t("suggestion.noReason"),
        chips: [
          { label: t("suggestion.recommended"), tone: "accent" },
          notice.source.kind === "official"
            ? { label: `✓ ${t("suggestion.official")}`, tone: "ok" }
            : {
                label: `⚠ ${t("suggestion.unofficial", { host: hostOf(notice.source.url) })}`,
                tone: "warn",
              },
        ],
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
