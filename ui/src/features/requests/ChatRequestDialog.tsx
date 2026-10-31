import { useCallback, useId, useState, type SubmitEvent } from "react";
import { useTranslation } from "react-i18next";
import { toEngineError } from "../../api";
import { Modal } from "../userpanel/Modal";
import {
  closeChatRequestDialog,
  useChatRequestTarget,
  type ChatRequestTarget,
} from "./chatRequestDialogStore";
import { sendChatRequest } from "./requestsStore";
import "./requests.css";

/** Longest note the engine accepts (in characters). */
const NOTE_MAX = 200;

/** Renders the "Send chat request" dialog while one is open; mount once. */
export function ChatRequestDialogHost() {
  const target = useChatRequestTarget();
  return target ? (
    <ChatRequestDialog
      key={`${String(target.serverId)}:${String(target.session)}`}
      target={target}
    />
  ) : null;
}

function ChatRequestDialog({ target }: { target: ChatRequestTarget }) {
  const { t } = useTranslation("members");
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);
  const noteId = useId();
  const counterId = useId();
  const length = Array.from(note).length;

  const close = useCallback(() => {
    closeChatRequestDialog();
    if (target.opener?.isConnected) target.opener.focus();
  }, [target]);

  const submit = (event: SubmitEvent) => {
    event.preventDefault();
    setBusy(true);
    setFailed(false);
    sendChatRequest(target.serverId, target.session, note).then(close, (error: unknown) => {
      console.error("Sending a chat request failed", toEngineError(error).code);
      setBusy(false);
      setFailed(true);
    });
  };

  return (
    <Modal title={t("chatRequest.title", { name: target.name })} onClose={close}>
      <form className="chat-request-form" onSubmit={submit}>
        <label htmlFor={noteId}>{t("chatRequest.note")}</label>
        <textarea
          id={noteId}
          className="chat-request-form__note"
          rows={3}
          value={note}
          maxLength={NOTE_MAX}
          aria-describedby={counterId}
          data-testid="chat-request-note"
          onChange={(event) => {
            setNote(event.target.value);
          }}
        />
        <span id={counterId} className="chat-request-form__counter">
          <span aria-hidden="true">
            {t("chatRequest.counter", { count: length, max: NOTE_MAX })}
          </span>
          <span className="visually-hidden">
            {t("chatRequest.counterLabel", { count: length, max: NOTE_MAX })}
          </span>
        </span>
        {failed && (
          <p role="alert" className="userpanel-error">
            {t("chatRequest.failed")}
          </p>
        )}
        <div className="userpanel-form__actions">
          <button type="button" className="button" onClick={close}>
            {t("common:cancel")}
          </button>
          <button
            type="submit"
            className="button button--primary"
            disabled={busy}
            data-testid="chat-request-send"
          >
            {t("chatRequest.send")}
          </button>
        </div>
      </form>
    </Modal>
  );
}
