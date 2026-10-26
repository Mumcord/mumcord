import { useId, useState, type SubmitEvent } from "react";
import { useTranslation } from "react-i18next";
import { cleanThemeName } from "../../../theme/shareCodec";
import { ModalDialog } from "../components/ModalDialog";

interface RenameThemeDialogProps {
  name: string;
  /** Called with the cleaned new name. */
  onRename: (name: string) => void;
  onCancel: () => void;
}

/** Renames an own theme from its circle's context menu (FR-2.63). */
export function RenameThemeDialog({ name, onRename, onCancel }: RenameThemeDialogProps) {
  const { t } = useTranslation("settings");
  const id = useId();
  const [text, setText] = useState(name);
  const clean = cleanThemeName(text);

  const onSubmit = (event: SubmitEvent) => {
    event.preventDefault();
    if (clean !== "") onRename(clean);
  };

  return (
    <ModalDialog title={t("themeEditor.renameTitle", { name })} onClose={onCancel}>
      <form onSubmit={onSubmit} noValidate>
        <label htmlFor={`${id}-name`} className="settings-field">
          {t("themeEditor.name")}
        </label>
        <input
          id={`${id}-name`}
          className="settings-input"
          value={text}
          aria-invalid={clean === ""}
          onChange={(event) => {
            setText(event.target.value);
          }}
        />
        <div className="settings-dialog__actions">
          <button type="button" className="settings-button" onClick={onCancel}>
            {t("common:cancel")}
          </button>
          <button
            type="submit"
            className="settings-button settings-button--primary"
            disabled={clean === ""}
          >
            {t("themeEditor.renameConfirm")}
          </button>
        </div>
      </form>
    </ModalDialog>
  );
}
