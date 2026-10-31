import { useTranslation } from "react-i18next";
import type { P2pMode } from "../../api";
import { P2pModeRadios } from "../p2p/P2pModeRadios";
import { openInBrowser } from "../chat/openLink";
import { StepFrame } from "./StepFrame";

export const PRIVACY_URL = "https://github.com/Mumcord/mumcord/blob/main/PRIVACY.md";

interface PrivacyStepProps {
  /** 1-based step number and step count of this wizard pass (#149 rerun: 2 of 4). */
  step: number;
  total: number;
  /** The peer-to-peer mode chosen so far (spec 04, FR-4.1); saved with the final step. */
  p2pMode: P2pMode;
  onP2pModeChange: (mode: P2pMode) => void;
  focusOnMount: boolean;
  onBack: () => void;
  onNext: () => void;
  /** Rerun only: closes the wizard (#149). */
  onCancel?: (() => void) | undefined;
}

/**
 * Step 3 on first start, step 2 in the rerun: what Mumcord does with the network and with data,
 * before the link-preview choice (QA.78), and the peer-to-peer mode (spec 04, FR-4.1).
 */
export function PrivacyStep({
  step,
  total,
  p2pMode,
  onP2pModeChange,
  focusOnMount,
  onBack,
  onNext,
  onCancel,
}: PrivacyStepProps) {
  const { t } = useTranslation("wizard");
  return (
    <StepFrame
      step={step}
      total={total}
      title={t("privacy.title")}
      focusOnMount={focusOnMount}
      busy={false}
      submitLabel={t("common:next")}
      onSubmit={onNext}
      onBack={onBack}
      onCancel={onCancel}
    >
      <ul className="wizard-list">
        <li>{t("privacy.noTelemetry")}</li>
        <li>{t("privacy.ownServers")}</li>
        <li>{t("privacy.linkPreviews")}</li>
        <li>{t("privacy.mumcordUsers")}</li>
        <li>{t("privacy.local")}</li>
      </ul>
      <P2pModeRadios
        value={p2pMode}
        onChange={(mode) => {
          // Without `allowGlobal` the radios never offer "Use global setting".
          if (mode !== null) onP2pModeChange(mode);
        }}
      />
      <div className="wizard-row">
        <button
          type="button"
          className="button"
          onClick={() => {
            openInBrowser(PRIVACY_URL);
          }}
        >
          {t("privacy.statement")}
        </button>
      </div>
    </StepFrame>
  );
}
