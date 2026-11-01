import { useTranslation } from "react-i18next";
import { ModalDialog } from "./ModalDialog";

interface ChangeListDialogProps {
  title: string;
  /** Sentence before the list of settings. */
  intro: string;
  /** Visible labels of the settings that change. */
  labels: readonly string[];
  /** Shown instead of intro and list when nothing would change; then only Close is offered. */
  emptyText?: string;
  confirmLabel: string;
  onConfirm: () => void;
  onCancel: () => void;
  /** Replaces Cancel, e.g. "Back" to the previous step. */
  cancelLabel?: string;
}

/**
 * Confirmation that lists every setting a reset or a preset switch changes (#161), so nothing
 * is changed silently. Cancel comes first so it gets the initial focus.
 */
export function ChangeListDialog({
  title,
  intro,
  labels,
  emptyText,
  confirmLabel,
  onConfirm,
  onCancel,
  cancelLabel,
}: ChangeListDialogProps) {
  const { t } = useTranslation("common");
  const empty = labels.length === 0 && emptyText !== undefined;
  return (
    <ModalDialog title={title} onClose={onCancel}>
      {empty ? <p>{emptyText}</p> : <ChangeList intro={intro} labels={labels} />}
      <div className="settings-dialog__actions">
        <button type="button" className="settings-button" onClick={onCancel}>
          {empty ? t("close") : (cancelLabel ?? t("cancel"))}
        </button>
        {!empty && (
          <button
            type="button"
            className="settings-button settings-button--primary"
            onClick={onConfirm}
          >
            {confirmLabel}
          </button>
        )}
      </div>
    </ModalDialog>
  );
}

/** A sentence and the labels of the settings that change. */
export function ChangeList({ intro, labels }: { intro: string; labels: readonly string[] }) {
  return (
    <>
      <p>{intro}</p>
      {labels.length > 0 && (
        <ul className="settings-change-list">
          {labels.map((label) => (
            <li key={label}>{label}</li>
          ))}
        </ul>
      )}
    </>
  );
}
