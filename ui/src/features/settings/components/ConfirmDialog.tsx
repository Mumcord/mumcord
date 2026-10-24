import { useTranslation } from "react-i18next";
import { ModalDialog } from "./ModalDialog";

interface ConfirmDialogProps {
  title: string;
  message: string;
  confirmLabel: string;
  onConfirm: () => void;
  onCancel: () => void;
}

/** Confirmation for destructive actions; Cancel comes first so it gets the initial focus. */
export function ConfirmDialog({
  title,
  message,
  confirmLabel,
  onConfirm,
  onCancel,
}: ConfirmDialogProps) {
  const { t } = useTranslation("common");
  return (
    <ModalDialog title={title} onClose={onCancel}>
      <p>{message}</p>
      <div className="settings-dialog__actions">
        <button type="button" className="settings-button" onClick={onCancel}>
          {t("cancel")}
        </button>
        <button
          type="button"
          className="settings-button settings-button--danger"
          onClick={onConfirm}
        >
          {confirmLabel}
        </button>
      </div>
    </ModalDialog>
  );
}
