import { useRef } from "react";
import { useTranslation } from "react-i18next";
import { Dialog } from "./Dialog";

interface ConfirmDialogProps {
  title: string;
  message: string;
  confirmLabel: string;
  danger?: boolean;
  onConfirm: () => void;
  onClose: () => void;
}

/** Yes/no confirmation; focus starts on Cancel so Enter never confirms by accident. */
export function ConfirmDialog({
  title,
  message,
  confirmLabel,
  danger = false,
  onConfirm,
  onClose,
}: ConfirmDialogProps) {
  const { t } = useTranslation("common");
  const cancelRef = useRef<HTMLButtonElement>(null);
  return (
    <Dialog title={title} onClose={onClose} initialFocus={cancelRef}>
      <p>{message}</p>
      <div className="servers-dialog__actions">
        <button ref={cancelRef} type="button" className="button" onClick={onClose}>
          {t("cancel")}
        </button>
        <button
          type="button"
          className={danger ? "button servers-button--danger" : "button button--primary"}
          onClick={onConfirm}
        >
          {confirmLabel}
        </button>
      </div>
    </Dialog>
  );
}
