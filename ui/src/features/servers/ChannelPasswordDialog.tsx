import { useEffect, useId, useRef, useState, type SubmitEvent } from "react";
import { useTranslation } from "react-i18next";
import { Dialog } from "./Dialog";
import {
  cancelChannelPassword,
  submitChannelPassword,
  type ChannelPasswordPrompt,
} from "./channelPasswordStore";

interface ChannelPasswordDialogProps {
  prompt: ChannelPasswordPrompt;
  channelName: string;
}

/**
 * Asks for a channel password after the server refused a join (FR-2.78). The password is stored
 * for the server as an access token; a wrong one is removed again by the engine.
 */
export function ChannelPasswordDialog({ prompt, channelName }: ChannelPasswordDialogProps) {
  const { t } = useTranslation("servers");
  const id = useId();
  const inputRef = useRef<HTMLInputElement>(null);
  const [password, setPassword] = useState("");
  const [visible, setVisible] = useState(false);
  const { busy, wrong, failed } = prompt;
  const error = busy
    ? ""
    : wrong
      ? t("channelPassword.wrong")
      : failed
        ? t("channelPassword.failed")
        : "";

  // Back in the field after a refused try, so the next one starts typing at once.
  useEffect(() => {
    if ((wrong || failed) && !busy) {
      inputRef.current?.focus();
      inputRef.current?.select();
    }
  }, [wrong, failed, busy]);

  const submit = (event: SubmitEvent) => {
    event.preventDefault();
    if (busy || password === "") return;
    void submitChannelPassword(password);
  };

  return (
    <Dialog title={t("channelPassword.title")} onClose={cancelChannelPassword}>
      <form onSubmit={submit}>
        <p>{t("channelPassword.intro", { channel: channelName })}</p>
        <div className="servers-form__field">
          <label htmlFor={`${id}-password`}>{t("channelPassword.label")}</label>
          <div className="servers-password__row">
            <input
              ref={inputRef}
              id={`${id}-password`}
              type={visible ? "text" : "password"}
              value={password}
              autoComplete="off"
              disabled={busy}
              aria-invalid={error !== ""}
              aria-describedby={error !== "" ? `${id}-error` : undefined}
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
            {error}
          </p>
        </div>
        <div className="servers-dialog__actions">
          <button type="button" className="button" onClick={cancelChannelPassword}>
            {t("channelPassword.cancel")}
          </button>
          <button
            type="submit"
            className="button button--primary"
            disabled={busy || password === ""}
          >
            {busy ? t("channelPassword.joining") : t("channelPassword.join")}
          </button>
        </div>
      </form>
    </Dialog>
  );
}
