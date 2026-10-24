import { useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type PttSession } from "../../../api";
import { Switch } from "../components/Switch";
import type { SettingsSectionProps } from "./types";

interface CompatibilityModeSwitchProps extends SettingsSectionProps {
  session: PttSession;
}

/**
 * Linux Wayland only: "Compatibility mode (like TeamSpeak)" (FR-1.48). The engine reads the
 * setting at start-up, so a change offers a restart until the running mode matches it.
 */
export function CompatibilityModeSwitch({
  session,
  settings,
  update,
}: CompatibilityModeSwitchProps) {
  const { t } = useTranslation("settings");
  const [restartFailed, setRestartFailed] = useState(false);
  const pending = settings.pttCompatibilityMode !== session.compatibilityActive;

  const restart = () => {
    setRestartFailed(false);
    getApi()
      .appRestart()
      .catch((error: unknown) => {
        console.error("Restarting Mumcord failed", error);
        setRestartFailed(true);
      });
  };

  return (
    <>
      <Switch
        label={t("voice.compatMode")}
        description={t("voice.compatModeHint")}
        checked={settings.pttCompatibilityMode}
        onChange={(pttCompatibilityMode) => {
          update({ pttCompatibilityMode });
        }}
      />
      {pending && (
        <div className="settings-field-row" role="status">
          <p className="settings-hint">{t("voice.compatRestartNeeded")}</p>
          <div className="settings-actions">
            <button type="button" className="settings-button" onClick={restart}>
              {t("voice.compatRestart")}
            </button>
          </div>
        </div>
      )}
      {!pending && session.compatibilityActive && (
        <p className="settings-hint">{t("voice.compatActive")}</p>
      )}
      <p role="alert" className="settings-error">
        {restartFailed ? t("voice.compatRestartFailed") : null}
      </p>
    </>
  );
}
