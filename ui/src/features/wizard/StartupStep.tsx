import { useTranslation } from "react-i18next";
import { Switch } from "../settings/components/Switch";
import { UpdateModeRow } from "../settings/sections/UpdatesSection";
import type { SettingsSectionProps } from "../settings/sections/types";
import { effectiveUpdateMode, updateModePatch } from "../updates/updateMode";
import { useUpdateStatus } from "../updates/useUpdateStatus";
import { StepFrame } from "./StepFrame";

interface StartupStepProps extends SettingsSectionProps {
  /** Whether Mumcord already opens `mumble://` links; `null` when unknown. */
  linkHandlerActive: boolean | null;
  registerLinkHandler: boolean;
  onRegisterLinkHandlerChange: (on: boolean) => void;
  onNext: () => void;
}

/**
 * "Startup and updates" (Custom only): start at login, start minimized, `mumble://` links and
 * the update mode (#295), with the controls and keys of Settings → General.
 */
export function StartupStep({
  settings,
  update,
  linkHandlerActive,
  registerLinkHandler,
  onRegisterLinkHandlerChange,
  onNext,
}: StartupStepProps) {
  const { t } = useTranslation("wizard");
  const { t: ts } = useTranslation("settings");
  const { status } = useUpdateStatus();
  const canInstall = status?.canInstall ?? true;
  return (
    <StepFrame title={t("startup.title")} subtitle={t("startup.subtitle")} onSubmit={onNext}>
      <section className="wizard-card wizard-rows" aria-label={ts("appearance.behaviour")}>
        <Switch
          label={ts("appearance.autostart")}
          checked={settings.autostart}
          onChange={(autostart) => {
            update({ autostart });
          }}
        />
        <Switch
          label={ts("appearance.startMinimized")}
          tip={{
            label: t("tipLabel", { name: ts("appearance.startMinimized") }),
            text: ts("appearance.startMinimizedHint"),
          }}
          checked={settings.startMinimized}
          disabled={!settings.autostart}
          onChange={(startMinimized) => {
            update({ startMinimized });
          }}
        />
        {linkHandlerActive === true ? (
          <p className="wizard-muted">{t("startup.linkHandlerActive")}</p>
        ) : (
          <Switch
            label={t("startup.linkHandler")}
            tip={{
              label: t("tipLabel", { name: t("startup.linkHandler") }),
              text: t("startup.linkHandlerHint"),
            }}
            checked={registerLinkHandler}
            onChange={onRegisterLinkHandlerChange}
          />
        )}
      </section>
      <section className="wizard-card" aria-label={ts("sections.updates")}>
        <UpdateModeRow
          mode={effectiveUpdateMode(settings, canInstall)}
          canInstall={canInstall}
          onChange={(mode) => {
            update(updateModePatch(mode));
          }}
        />
      </section>
    </StepFrame>
  );
}
