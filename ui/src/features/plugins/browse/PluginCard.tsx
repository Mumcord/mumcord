import { useTranslation } from "react-i18next";
import type { IndexedPlugin } from "../../../api";
import { PluginIcon } from "./PluginIcon";
import { sourceLabel } from "./source";

export interface PluginCardProps {
  plugin: IndexedPlugin;
  installed: boolean;
  /** Echolings' highlighted card at the top (FR-6.59). */
  highlighted: boolean;
  onDetails: () => void;
  onInstall: () => void;
}

/** One plugin in the Browse list. Index texts are untrusted and rendered as plain text. */
export function PluginCard({
  plugin,
  installed,
  highlighted,
  onDetails,
  onInstall,
}: PluginCardProps) {
  const { t } = useTranslation("pluginBrowse");
  const name = plugin.name;
  return (
    <li className={highlighted ? "plugin-card plugin-card-highlight" : "plugin-card"}>
      <PluginIcon id={plugin.id} name={name} hasIcon={plugin.icon !== null} />
      <div className="plugin-card-body">
        <h3 className="plugin-card-name">{name}</h3>
        <div className="plugin-card-labels">
          {highlighted && (
            <span className="plugin-label plugin-label-highlight">{t("highlight")}</span>
          )}
          <span
            className={
              plugin.source.kind === "official"
                ? "plugin-label"
                : "plugin-label plugin-label-unofficial"
            }
          >
            {sourceLabel(t, plugin.source)}
          </span>
          {installed && <span className="plugin-label">{t("installed")}</span>}
        </div>
        <p className="plugin-card-summary">{plugin.summary}</p>
      </div>
      <div className="plugin-card-actions">
        <button
          type="button"
          className="settings-button"
          aria-label={t("detailsNamed", { name })}
          onClick={onDetails}
        >
          {t("details")}
        </button>
        {!installed && (
          <button
            type="button"
            className="settings-button settings-button--primary"
            aria-label={t("installNamed", { name })}
            onClick={onInstall}
          >
            {t("install")}
          </button>
        )}
      </div>
    </li>
  );
}
