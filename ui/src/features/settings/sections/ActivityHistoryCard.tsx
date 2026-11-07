import { useState } from "react";
import { useTranslation } from "react-i18next";
import { clearActivityHistory } from "../../activity/activityStore";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { Group } from "../components/Section";

/** "Clear activity history" (#594): the saved activity feed of all servers, after a confirmation. */
export function ActivityHistoryCard() {
  const { t } = useTranslation("settings");
  const [confirming, setConfirming] = useState(false);
  return (
    <Group title={t("activityHistory.title")} icon="list">
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
