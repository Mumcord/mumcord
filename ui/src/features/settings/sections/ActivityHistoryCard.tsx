import { useState } from "react";
import { useTranslation } from "react-i18next";
import { clearActivityHistory } from "../../activity/activityStore";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { Group } from "../components/Section";
import { Switch } from "../components/Switch";
import type { SettingsSectionProps } from "./types";

/**
 * "Keep activity history after a restart" (#661) and "Clear activity history" (#594): whether
 * the feed is saved on this device, and deleting what was saved, after a confirmation.
 */
export function ActivityHistoryCard({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  const [confirming, setConfirming] = useState(false);
  return (
    <Group title={t("activityHistory.title")} icon="list">
      <Switch
        label={t("activityHistory.keep")}
        description={t("activityHistory.keepHint")}
        checked={settings.keepActivityHistory}
        onChange={(keepActivityHistory) => {
          update({ keepActivityHistory });
        }}
      />
      <div className="settings-control-row">
        <div className="settings-control-row__text">
          <p className="settings-control-row__title">{t("activityHistory.clear")}</p>
          <p className="settings-hint">{t("activityHistory.hint")}</p>
          <p className="settings-hint settings-hint--muted">{t("activityHistory.local")}</p>
        </div>
        <button
          type="button"
          className="settings-button settings-button--danger"
          onClick={() => {
            setConfirming(true);
          }}
        >
          {t("activityHistory.button")}
        </button>
      </div>
      {confirming && (
        <ConfirmDialog
          title={t("activityHistory.clear")}
          message={t("activityHistory.confirm")}
          confirmLabel={t("activityHistory.confirmButton")}
          onConfirm={() => {
            clearActivityHistory();
            setConfirming(false);
          }}
          onCancel={() => {
            setConfirming(false);
          }}
        />
      )}
    </Group>
  );
}
