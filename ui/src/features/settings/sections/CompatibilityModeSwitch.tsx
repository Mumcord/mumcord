import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type KdeX11Access, type PttSession } from "../../../api";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { Switch } from "../components/Switch";
import { useSettingsNavigation } from "../settingsNavigation";
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
  const [x11SettingsFailed, setX11SettingsFailed] = useState(false);
  const [kdeAccess, setKdeAccess] = useState<KdeX11Access | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [changed, setChanged] = useState(false);
  const [changeFailed, setChangeFailed] = useState(false);
  const navigation = useSettingsNavigation();
  const switchRef = useRef<HTMLInputElement>(null);
  const focusPending = navigation?.compatibilityFocusPending === true;
  const focused = navigation?.compatibilityFocused;
  useEffect(() => {
    if (!focusPending) return;
    switchRef.current?.focus();
    switchRef.current?.scrollIntoView({ block: "center" });
    focused?.();
  }, [focusPending, focused]);
  const kde = session.kde;
  useEffect(() => {
    if (!kde) return;
    getApi()
      .pttKdeX11Access()
      .then(setKdeAccess, (error: unknown) => {
        console.error("Reading the KDE X11 mouse setting failed", error);
      });
  }, [kde]);
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

  const openX11Settings = () => {
    setX11SettingsFailed(false);
    getApi()
      .pttOpenKdeX11Settings()
      .catch((error: unknown) => {
        console.error("Opening the KDE X11 settings failed", error);
        setX11SettingsFailed(true);
      });
  };

  const changeAccess = (call: Promise<KdeX11Access>) => {
    setConfirming(false);
    setChangeFailed(false);
    call.then(
      (next) => {
        setKdeAccess(next);
        setChanged(true);
      },
      (error: unknown) => {
        console.error("Changing the KDE X11 mouse setting failed", error);
        setChangeFailed(true);
      },
    );
  };

  return (
    <>
      <Switch
        inputRef={switchRef}
        label={t("voice.compatMode")}
        description={t("voice.compatModeHint")}
        checked={settings.pttCompatibilityMode}
        onChange={(pttCompatibilityMode) => {
          update({ pttCompatibilityMode });
        }}
      />
      {session.kde && (
        <div className="settings-field-row">
          {kdeAccess?.allowed === true ? (
            <p className="settings-hint">{t("voice.compatKdeAllowed")}</p>
          ) : (
            <p className="settings-hint">{t("voice.compatKdeHint")}</p>
          )}
          {changed && (
            <p className="settings-hint" role="status">
              {t("voice.compatKdeRestart")}
            </p>
          )}
          <div className="settings-actions">
            {kdeAccess?.allowed === false && (
              <button
                type="button"
                className="settings-button"
                onClick={() => {
                  setConfirming(true);
                }}
              >
                {t("voice.compatKdeAllow")}
              </button>
            )}
            {kdeAccess?.undoAvailable === true && (
              <button
                type="button"
                className="settings-button"
                onClick={() => {
                  changeAccess(getApi().pttKdeX11Undo());
                }}
              >
                {t("voice.compatKdeUndo")}
              </button>
            )}
            <button type="button" className="settings-button" onClick={openX11Settings}>
              {t("voice.compatKdeOpen")}
            </button>
          </div>
        </div>
      )}
      {confirming && (
        <ConfirmDialog
          title={t("voice.compatKdeConfirmTitle")}
          message={t("voice.compatKdeConfirm")}
          confirmLabel={t("voice.compatKdeConfirmButton")}
          onConfirm={() => {
            changeAccess(getApi().pttKdeX11AllowMouse());
          }}
          onCancel={() => {
            setConfirming(false);
          }}
        />
      )}
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
        {x11SettingsFailed ? t("voice.compatKdeFailed") : null}
        {changeFailed ? t("voice.compatKdeChangeFailed") : null}
      </p>
    </>
  );
}
