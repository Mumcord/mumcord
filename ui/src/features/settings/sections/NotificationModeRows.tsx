import { useId } from "react";
import { useTranslation } from "react-i18next";
import type { ActivityNotifySettings, NotificationMode } from "../../../api";
import { Select } from "../../../components/Select";
import type { SettingsSectionProps } from "./types";

/** The order of the choices in each dropdown. */
const MODES: readonly NotificationMode[] = ["off", "inApp", "desktop", "both"];

/** Activity events that notify (#597), in the order of the feed's kinds. */
const ACTIVITY_KINDS: readonly (keyof ActivityNotifySettings)[] = [
  "screenShare",
  "watchParty",
  "pluginActivity",
  "pluginEvent",
  "joins",
];

/**
 * One row per kind of notification (#660): new messages and the five activity kinds, each with
 * its own Off / In-app / System / Both. Shared by Settings → Notifications and the wizard.
 */
export function NotificationModeRows({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  const baseId = useId();
  const options = MODES.map((mode) => ({ value: mode, label: t(`notifications.mode.${mode}`) }));
  const row = (
    key: string,
    label: string,
    hint: string | null,
    value: NotificationMode,
    onChange: (mode: NotificationMode) => void,
  ) => (
    <div key={key} className="settings-control-row">
      <div className="settings-control-row__text">
        <label htmlFor={`${baseId}-${key}`}>{label}</label>
        {hint !== null && <p className="settings-hint">{hint}</p>}
      </div>
      <Select
        id={`${baseId}-${key}`}
        value={value}
        options={options}
        onChange={(mode) => {
          onChange(mode as NotificationMode);
        }}
      />
    </div>
  );
  return (
    <>
      {row(
        "messages",
        t("notifications.messages"),
        t("notifications.messagesHint"),
        settings.notifications,
        (notifications) => {
          update({ notifications });
        },
      )}
      {ACTIVITY_KINDS.map((kind) =>
        row(
          kind,
          t(`notifications.activity.${kind}`),
          null,
          settings.activityNotify[kind],
          (mode) => {
            update({ activityNotify: { ...settings.activityNotify, [kind]: mode } });
          },
        ),
      )}
    </>
  );
}
