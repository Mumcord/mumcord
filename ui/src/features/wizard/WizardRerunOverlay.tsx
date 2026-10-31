import { useEffect, useId } from "react";
import { useTranslation } from "react-i18next";
import { Wizard } from "./Wizard";

interface WizardRerunOverlayProps {
  /** Closes the wizard; the settings behind it stay open and unchanged. */
  onClose: () => void;
  /** Opens the identity manager (#148) instead of finishing the wizard. */
  onManageIdentities: () => void;
}

/**
 * The setup wizard on top of the settings (#149, FR-3.118): a full-window modal over the open
 * settings window, so Cancel and Finish land back on "Run setup wizard again".
 */
export function WizardRerunOverlay({ onClose, onManageIdentities }: WizardRerunOverlayProps) {
  const { t } = useTranslation("wizard");
  const titleId = useId();

  // Focus returns to the button that opened it, like the settings overlay does.
  useEffect(() => {
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    return () => {
      if (opener?.isConnected) opener.focus();
    };
  }, []);

  return (
    <div className="overlay">
      <div
        className="centered-screen wizard-rerun-overlay"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
      >
        <h1 id={titleId} className="wizard-rerun__title">
          {t("rerun.title")}
        </h1>
        <Wizard mode="rerun" onClose={onClose} onManageIdentities={onManageIdentities} />
      </div>
    </div>
  );
}
