import { useTranslation } from "react-i18next";
import { NotificationModeRows } from "../settings/sections/NotificationModeRows";
import type { SettingsSectionProps } from "../settings/sections/types";
import { StepFrame } from "./StepFrame";

/**
 * "How should Mumcord tell you?" (Custom only, #660): how new messages and each kind of activity
 * notify, with the rows from Settings → Notifications.
 */
export function NotificationsStep({
  settings,
  update,
  onNext,
}: SettingsSectionProps & { onNext: () => void }) {
  const { t } = useTranslation("wizard");
  const { t: ts } = useTranslation("settings");
  return (
    <StepFrame
      title={t("notifications.title")}
      subtitle={t("notifications.subtitle")}
      onSubmit={onNext}
    >
      <section
        className="wizard-card wizard-rows settings-group"
        aria-label={ts("notifications.title")}
      >
        <NotificationModeRows settings={settings} update={update} />
      </section>
      <p className="wizard-note">{ts("notifications.modesHint")}</p>
    </StepFrame>
  );
}
