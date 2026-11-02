import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { ModalDialog } from "../../settings/components/ModalDialog";

interface RemoveDialogProps {
  name: string;
  onRemove: (deleteData: boolean) => void;
  onCancel: () => void;
}

/**
 * "Remove <name>?" (spec §5.5). Data is kept by default so a reinstall within 30 days gets it
 * back (Q5.3); Cancel comes first so it gets the initial focus.
 */
export function RemoveDialog({ name, onRemove, onCancel }: RemoveDialogProps) {
  const { t } = useTranslation("pluginInstalled");
  const { t: tc } = useTranslation("common");
  const checkboxId = useId();
  const [deleteData, setDeleteData] = useState(false);
  return (
    <ModalDialog title={t("remove.title", { name })} onClose={onCancel}>
      <div className="plugins-installed-remove">
        <label htmlFor={checkboxId} className="plugins-installed-check">
          <input
            id={checkboxId}
            type="checkbox"
            checked={deleteData}
            onChange={(event) => {
              setDeleteData(event.target.checked);
            }}
          />
          {t("remove.deleteData")}
        </label>
        <p className="settings-hint">{t("remove.hint")}</p>
      </div>
      <div className="settings-dialog__actions">
        <button type="button" className="settings-button" onClick={onCancel}>
          {tc("cancel")}
        </button>
        <button
          type="button"
          className="settings-button settings-button--danger"
          onClick={() => {
            onRemove(deleteData);
          }}
        >
          {t("remove.confirm")}
        </button>
      </div>
    </ModalDialog>
  );
}
