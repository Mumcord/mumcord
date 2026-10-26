import { useId, useState, type SubmitEvent } from "react";
import { useTranslation } from "react-i18next";
import {
  decodeThemeCode,
  findThemeCode,
  type SharedTheme,
  type ThemeDecodeError,
} from "../../../theme/shareCodec";
import { ModalDialog } from "../components/ModalDialog";

interface ImportThemeDialogProps {
  version: string;
  /** `newerVersion` is the creating Mumcord version when it is newer than this one. */
  onImport: (theme: SharedTheme, newerVersion: string | null) => void;
  onClose: () => void;
}

const ERROR_KEYS: Record<ThemeDecodeError, string> = {
  invalid: "themeEditor.importInvalid",
  unreadable: "themeEditor.importUnreadable",
};

/** Paste a shared theme message or code (FR-2.16); only valid, readable themes get through. */
export function ImportThemeDialog({ version, onImport, onClose }: ImportThemeDialogProps) {
  const { t } = useTranslation(["settings", "common"]);
  const id = useId();
  const [text, setText] = useState("");
  const [error, setError] = useState<ThemeDecodeError | null>(null);

  const onSubmit = (event: SubmitEvent) => {
    event.preventDefault();
    const code = findThemeCode(text);
    const result = code === null ? null : decodeThemeCode(code, version);
    if (!result?.ok) {
      setError(result?.error ?? "invalid");
      return;
    }
    onImport(result.theme, result.madeWithNewer ? result.createdWith : null);
  };

  return (
    <ModalDialog title={t("themeEditor.importTitle")} onClose={onClose}>
      <form onSubmit={onSubmit} noValidate>
        <label htmlFor={`${id}-code`} className="settings-field">
          {t("themeEditor.importLabel")}
        </label>
        <textarea
          id={`${id}-code`}
          className="settings-input"
          rows={3}
          value={text}
          spellCheck={false}
          aria-invalid={error !== null}
          aria-describedby={`${id}-hint${error ? ` ${id}-error` : ""}`}
          onChange={(event) => {
            setText(event.target.value);
            setError(null);
          }}
        />
        <p id={`${id}-hint`} className="settings-hint">
          {t("themeEditor.importHint")}
        </p>
        {error && (
          <p id={`${id}-error`} className="settings-error" role="alert">
            {t(ERROR_KEYS[error])}
          </p>
        )}
        <div className="settings-dialog__actions">
          <button type="button" className="settings-button" onClick={onClose}>
            {t("common:cancel")}
          </button>
          <button
            type="submit"
            className="settings-button settings-button--primary"
            disabled={text.trim() === ""}
          >
            {t("themeEditor.importConfirm")}
          </button>
        </div>
      </form>
    </ModalDialog>
  );
}
