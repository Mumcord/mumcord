import { useTranslation } from "react-i18next";
import type { IndexedPlugin } from "../../../api";
import { usePlatformSettings } from "../platformSettings";
import { InstallProgress } from "../prompt/InstallProgress";
import { ScoreChip } from "../score";
import { useListingScore } from "../score/useListingScore";
import { openSettingsSection } from "../../settings/settingsOpener";
import { CATEGORIES } from "./filter";
import { PluginIcon } from "./PluginIcon";
import { sourceLabel } from "./source";

export interface PluginCardProps {
  plugin: IndexedPlugin;
  installed: boolean;
  onDetails: () => void;
  onInstall: () => void;
}

/** One plugin in the Browse list. Index texts are untrusted and rendered as plain text. */
export function PluginCard({ plugin, installed, onDetails, onInstall }: PluginCardProps) {
  const { t } = useTranslation("pluginBrowse");
  const name = plugin.name;
  const category = CATEGORIES.find((value) => plugin.categories.includes(value));
  const meta = [
    plugin.authors.join(", "),
    category === undefined ? "" : t(`category.${category}`),
    t("metaPermissions", { count: plugin.permissions.length }),
  ].filter((part) => part !== "");
  const official = plugin.source.kind === "official";
  // Official plugins are reviewed instead of scored (mockup 01); the label stays in the title row.
  const scored = useListingScore(plugin.id, null, !official);
  const settings = usePlatformSettings();
  const blocked =
    settings?.blockHighRisk === true && scored?.kind === "scored" && scored.score.band === "high";
  const needsMumcord = plugin.needsMumcord;
  return (
    <li className="plugin-card" data-plugin-id={plugin.id}>
      <PluginIcon id={plugin.id} name={name} hasIcon={plugin.icon !== null} />
      <div className="plugin-card-body">
        <div className="plugin-card-title">
          <h3 className="plugin-card-name">
            {/* The whole card opens the details: this button's ::after covers the card. */}
            <button
              type="button"
              className="plugin-card-open"
              aria-label={t("detailsNamed", { name })}
              onClick={onDetails}
            >
              {name}
            </button>
          </h3>
          <span className={official ? "plugin-label" : "plugin-label plugin-label-unofficial"}>
            {official ? `✓ ${t("officialReviewed")}` : sourceLabel(t, plugin.source)}
          </span>
        </div>
        <p className="plugin-card-meta">{meta.join(" · ")}</p>
        <p className="plugin-card-summary">{plugin.summary}</p>
        {scored !== null && <ScoreChip scored={scored} blocked={blocked} />}
        {needsMumcord !== null && (
          <p className="plugin-card-needs">
            <span>{t("needsMumcord", { version: needsMumcord })}</span>{" "}
            <button
              type="button"
              className="plugin-card-needs-link"
              onClick={() => {
                openSettingsSection("general");
              }}
            >
              {t("updateMumcord")}
            </button>
          </p>
        )}
        <InstallProgress id={plugin.id} name={name} variant="slim" />
      </div>
      {installed ? (
        <span className="plugin-card-installed">✓ {t("installed")}</span>
      ) : (
        <button
          type="button"
          className="settings-button settings-button--primary plugin-card-install"
          aria-label={t("installNamed", { name })}
          disabled={needsMumcord !== null || blocked}
          onClick={onInstall}
        >
          {/* Unofficial plugins open a confirmation first (mockup 01). */}
          {official ? t("install") : `${t("install")}…`}
        </button>
      )}
    </li>
  );
}
