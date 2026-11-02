import { useTranslation } from "react-i18next";
import { useUpdatesCount } from "./useUpdatesCount";
import "../plugins.css";

/** The update count next to "Plugins" in the settings sidebar; nothing when there is none. */
export function UpdatesBadge() {
  const { t } = useTranslation("pluginSettings");
  const count = useUpdatesCount();
  if (count === 0) return null;
  return (
    <span className="plugins-tab__pill settings__tab-badge">
      {/* Read as "Plugins, Updates 2" instead of a bare number. */}
      <span className="visually-hidden">{t("tabs.updates")} </span>
      {count}
    </span>
  );
}
