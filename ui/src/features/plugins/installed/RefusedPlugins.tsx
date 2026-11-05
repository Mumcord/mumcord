import { useId } from "react";
import { useTranslation } from "react-i18next";
import type { RefusedPlugin } from "../../../api";

/**
 * Plugins refused at their first load in this run (FR-9.90). They have no stored record, so no
 * card; the reason is one of Diagnostics' fixed texts, never the plugin's own words.
 */
export function RefusedPlugins({ refused }: { refused: RefusedPlugin[] }) {
  const { t } = useTranslation("pluginInstalled");
  const headingId = useId();
  if (refused.length === 0) return null;
  return (
    <section aria-labelledby={headingId} className="plugins-installed-refused">
      <h3 id={headingId}>{t("refused.title")}</h3>
      <ul>
        {refused.map((entry) => (
          <li key={entry.name}>
            <strong>{entry.name}</strong>
            <p role="alert" className="settings-error">
              {entry.issue.kind === "refused_app_menu_label"
                ? t("diagnostics.refusedAppMenuLabel")
                : t("diagnostics.fieldWithoutPermission", {
                    field: entry.issue.field,
                    permission: entry.issue.permission,
                  })}
            </p>
            <p className="settings-hint">
              {t("refused.when", { time: new Date(entry.atMillis).toLocaleString() })}
            </p>
          </li>
        ))}
      </ul>
    </section>
  );
}
