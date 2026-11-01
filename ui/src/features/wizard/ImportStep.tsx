import { useId } from "react";
import { useTranslation } from "react-i18next";
import type { Identity, MumbleImportFound, Settings } from "../../api";
import { MumbleImport } from "../mumbleImport/MumbleImport";

interface ImportStepProps {
  /** 1-based step number and the number of steps, as the other steps show them. */
  step: number;
  total: number;
  found: MumbleImportFound;
  settings: Settings;
  saveSettings: (next: Settings) => Promise<Settings>;
  /** The identity the import added, or `null` after an undo. */
  onIdentityChange: (identity: Identity | null) => void;
  onNext: () => void;
}

/**
 * Optional first step when the official Mumble client is installed: copy favourites, the
 * certificate and settings (FR-3.93, FR-3.94, issue #66). Shown before the identity step, so
 * an imported certificate already is the user's identity there.
 */
export function ImportStep({
  step,
  total,
  found,
  settings,
  saveSettings,
  onIdentityChange,
  onNext,
}: ImportStepProps) {
  const { t } = useTranslation("mumbleImport");
  const progress = t("wizard:progress", { current: step, total });
  const headingId = useId();
  return (
    <section className="wizard-step wizard-step__body" aria-labelledby={headingId}>
      <MumbleImport
        found={found}
        settings={settings}
        saveSettings={async (patch) => {
          await saveSettings({ ...settings, ...patch });
        }}
        header={(stage) => (
          <>
            <p className="wizard-eyebrow">{t("optional", { progress })}</p>
            <h2 id={headingId} className="mumble-import__heading" tabIndex={-1} data-stage-heading>
              {stage === "choose" ? t("title") : t("summary.title")}
            </h2>
            <p className="wizard-muted">{stage === "choose" ? t("intro") : t("summary.intro")}</p>
          </>
        )}
        dismissLabel={t("skip")}
        onDismiss={onNext}
        onFinish={onNext}
        onChanged={(done) => {
          onIdentityChange(done?.identity ?? null);
        }}
      />
    </section>
  );
}
