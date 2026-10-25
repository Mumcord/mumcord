import { useTranslation } from "react-i18next";
import { openInBrowser } from "../chat/openLink";
import { StepFrame } from "./StepFrame";

export const PRIVACY_URL = "https://github.com/ToBee94/mumcord/blob/main/PRIVACY.md";

interface PrivacyStepProps {
  focusOnMount: boolean;
  onBack: () => void;
  onNext: () => void;
}

/** Step 3: what Mumcord does with the network and with data, before the link-preview choice (QA.78). */
export function PrivacyStep({ focusOnMount, onBack, onNext }: PrivacyStepProps) {
  const { t } = useTranslation("wizard");
  return (
    <StepFrame
      step={3}
      title={t("privacy.title")}
      focusOnMount={focusOnMount}
      busy={false}
      submitLabel={t("common:next")}
      onSubmit={onNext}
      onBack={onBack}
    >
      <ul className="wizard-list">
        <li>{t("privacy.noTelemetry")}</li>
        <li>{t("privacy.ownServers")}</li>
        <li>{t("privacy.linkPreviews")}</li>
        <li>{t("privacy.mumcordUsers")}</li>
        <li>{t("privacy.local")}</li>
      </ul>
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
