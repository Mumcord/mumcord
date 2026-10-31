import { useId } from "react";
import { useTranslation } from "react-i18next";
import { Dialog } from "../servers/Dialog";

interface WizardCancelDialogProps {
  /** Closes the question and keeps the wizard open. */
  onKeepEditing: () => void;
  /** Closes the wizard; nothing was saved, so nothing is lost but the choices. */
  onDiscard: () => void;
}

/** Confirms Cancel when the rerun holds changes that are not saved yet (#149). */
export function WizardCancelDialog({ onKeepEditing, onDiscard }: WizardCancelDialogProps) {
  const { t } = useTranslation("wizard");
  const bodyId = useId();
  return (
    <Dialog
      title={t("rerun.discardTitle")}
      role="alertdialog"
      describedBy={bodyId}
      onClose={onKeepEditing}
    >
      <p id={bodyId}>{t("rerun.discardBody")}</p>
      <div className="servers-dialog__actions">
        <button type="button" className="button" onClick={onKeepEditing}>
          {t("rerun.discardKeep")}
        </button>
        <button type="button" className="button servers-button--danger" onClick={onDiscard}>
          {t("rerun.discardConfirm")}
        </button>
      </div>
    </Dialog>
  );
}
