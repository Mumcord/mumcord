import { useEffect, useId, useRef, useState, type SubmitEvent } from "react";
import { useTranslation } from "react-i18next";
import { Dialog } from "./Dialog";
import { cancelPasswordPrompt, retryWithPassword, type PasswordPrompt } from "./store";

interface PasswordDialogProps {
  prompt: PasswordPrompt;
  serverName: string;
  /** The server had a saved password; "Save password" starts checked then. */
  savedPassword: boolean;
}

/**
 * Asks for another password after a connect was refused for a wrong one (FR-2.59). Connect
 * retries with the typed password for this connect only; the saved server changes after the
 * connect succeeded, and only if "Save password" is on.
 */
export function PasswordDialog({ prompt, serverName, savedPassword }: PasswordDialogProps) {
  const { t } = useTranslation("servers");
  const id = useId();
  const inputRef = useRef<HTMLInputElement>(null);
  const [password, setPassword] = useState("");
  const [visible, setVisible] = useState(false);
  const [save, setSave] = useState(savedPassword);
  const { serverId, busy, wrong } = prompt;

  // Back in the field after a refused retry, so the next try starts typing at once.
  useEffect(() => {
    if (wrong && !busy) {
      inputRef.current?.focus();
      inputRef.current?.select();
    }
  }, [wrong, busy]);

  const cancel = () => {
    cancelPasswordPrompt(serverId);
  };
  const submit = (event: SubmitEvent) => {
    event.preventDefault();
    if (busy || password === "") return;
    void retryWithPassword(serverId, password, save);
  };

  return (
    <Dialog title={t("password.title", { server: serverName })} onClose={cancel}>
      <form onSubmit={submit}>
        <p>{t("password.intro")}</p>
        <div className="servers-form__field">
          <label htmlFor={`${id}-password`}>{t("password.label")}</label>
          <div className="servers-password__row">
            <input
              ref={inputRef}
              id={`${id}-password`}
              type={visible ? "text" : "password"}
              value={password}
              autoComplete="off"
              disabled={busy}
              aria-invalid={wrong && !busy}
              aria-describedby={wrong && !busy ? `${id}-error` : undefined}
              onChange={(event) => {
                setPassword(event.target.value);
              }}
            />
            <button
              type="button"
              className="button"
              aria-pressed={visible}
              onClick={() => {
                setVisible((v) => !v);
              }}
            >
              {visible ? t("password.hide") : t("password.show")}
            </button>
          </div>
          <p id={`${id}-error`} className="servers-form__error" role="alert">
            {wrong && !busy ? t("password.wrong") : ""}
          </p>
          <label className="servers-form__check">
            <input
              type="checkbox"
              checked={save}
              disabled={busy}
              onChange={(event) => {
                setSave(event.target.checked);
              }}
            />
            {t("password.save")}
          </label>
        </div>
        <div className="servers-dialog__actions">
          <button type="button" className="button" onClick={cancel}>
            {t("password.cancel")}
          </button>
          <button
            type="submit"
            className="button button--primary"
            disabled={busy || password === ""}
          >
            {busy ? t("password.connecting") : t("password.connect")}
          </button>
        </div>
      </form>
    </Dialog>
  );
}
