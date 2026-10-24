import { useId, useState, type SyntheticEvent } from "react";
import { useTranslation } from "react-i18next";
import { ModalDialog } from "./ModalDialog";

interface PasswordDialogProps {
  title: string;
  message: string;
  passwordLabel: string;
  confirmLabel: string;
  /** Asks for the password twice and refuses an empty one (export). */
  confirmTwice?: boolean;
  danger?: boolean;
  /**
   * Runs the action. A thrown error keeps the dialog open; its text is shown in the dialog.
   * Returning normally closes it via the caller.
   */
  onSubmit: (password: string) => Promise<void>;
  /** Turns a failure of `onSubmit` into plain language. */
  describeError: (error: unknown) => string;
  onCancel: () => void;
}

/** Password prompt for identity export (new password, twice) and import (optional password). */
export function PasswordDialog({
  title,
  message,
  passwordLabel,
  confirmLabel,
  confirmTwice = false,
  danger = false,
  onSubmit,
  describeError,
  onCancel,
}: PasswordDialogProps) {
  const { t } = useTranslation(["settings", "common"]);
  const id = useId();
  const [password, setPassword] = useState("");
  const [repeat, setRepeat] = useState("");
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const submit = async (event: SyntheticEvent) => {
    event.preventDefault();
    if (busy) return;
    if (confirmTwice) {
      if (password === "") {
        setProblem(t("identity.passwordEmpty"));
        return;
      }
      if (password !== repeat) {
        setProblem(t("identity.passwordMismatch"));
        return;
      }
    }
    setProblem(null);
    setBusy(true);
    try {
      await onSubmit(password);
    } catch (error: unknown) {
      setBusy(false);
      setProblem(describeError(error));
    }
  };

  return (
    <ModalDialog title={title} onClose={onCancel}>
      <form onSubmit={(event) => void submit(event)}>
        <p>{message}</p>
        <label className="settings-field" htmlFor={`${id}-pw`}>
          {passwordLabel}
        </label>
        <input
          id={`${id}-pw`}
          type="password"
          className="settings-input"
          autoComplete={confirmTwice ? "new-password" : "off"}
          value={password}
          onChange={(event) => {
            setPassword(event.target.value);
          }}
        />
        {confirmTwice && (
          <>
            <label className="settings-field" htmlFor={`${id}-repeat`}>
              {t("identity.passwordRepeat")}
            </label>
            <input
              id={`${id}-repeat`}
              type="password"
              className="settings-input"
              autoComplete="new-password"
              value={repeat}
              onChange={(event) => {
                setRepeat(event.target.value);
              }}
            />
          </>
        )}
        <div role="alert" className="settings-error">
          {problem}
        </div>
        <div className="settings-dialog__actions">
          <button type="button" className="settings-button" onClick={onCancel}>
            {t("common:cancel")}
          </button>
          <button
            type="submit"
            className={`settings-button ${danger ? "settings-button--danger" : "settings-button--primary"}`}
            aria-disabled={busy}
          >
            {confirmLabel}
          </button>
        </div>
      </form>
    </ModalDialog>
  );
}
