import { useTranslation } from "react-i18next";
import type { Settings } from "../../../api";
import type { PresetId } from "../../wizard/presets";
import { Group } from "../components/Section";
import { useSettingsNavigation } from "../settingsNavigation";
import { summaryLines } from "./privacySummary";

/**
 * Read-only overview at the top of Privacy & Connections (#161): whether the IP address stays
 * hidden, who can message you, what is loaded from outside, history sync and sharing. Each line
 * links to the card where it is changed.
 */
export function PrivacySummaryCard({
  settings,
  presetMatch,
}: {
  settings: Settings;
  /** The preset all preset-covered values match right now, if any. */
  presetMatch: PresetId | null;
}) {
  const { t } = useTranslation("settings");
  const navigation = useSettingsNavigation();
  const hidden = settings.p2pMode === "relayOnly" || settings.p2pMode === "relayFirst";
  return (
    <Group
      title={t("privacySummary.title")}
      icon="shield"
      wide
      badge={
        presetMatch !== null && (
          <span className="setting-badge setting-badge--recommended">
            {t("privacySummary.matches", { preset: t(`presets.${presetMatch}.name`) })}
          </span>
        )
      }
    >
      <p className="privacy-summary__headline">
        {t(hidden ? "privacySummary.ipHidden" : "privacySummary.ipVisible")}
      </p>
      <ul className="privacy-summary__lines">
        {summaryLines(settings, t).map((line) => (
          <li key={line.target.group}>
            <span>{line.text}</span>
            <button
              type="button"
              className="settings-link"
              aria-label={t("privacySummary.changeLabel", { topic: t(line.target.group) })}
              onClick={() => {
                navigation?.jumpTo(line.target);
              }}
            >
              {t("privacySummary.change")}
            </button>
          </li>
        ))}
      </ul>
    </Group>
  );
}
