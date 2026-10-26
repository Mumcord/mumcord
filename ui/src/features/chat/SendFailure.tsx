/**
 * Under an own message that could not be sent (FR-2.74, FR-2.47): an error mark with the reason
 * and "Retry" / "Delete". The polite notice for screen readers comes from the send itself.
 */
import { useTranslation } from "react-i18next";
import { removeOutgoing, type OutgoingSend } from "./chatStore";
import { retrySend } from "./sendMessage";

export function SendFailure({ entry, reason }: { entry: OutgoingSend; reason: string }) {
  const { t } = useTranslation("chat");
  return (
    <div className="chat-failed">
      <p className="chat-failed__reason">
        <svg viewBox="0 0 24 24" width="14" height="14" aria-hidden="true" focusable="false">
          <circle cx="12" cy="12" r="9" fill="none" stroke="currentColor" strokeWidth="2" />
          <path
            d="M12 7v6M12 16.5v.5"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
          />
        </svg>{" "}
        <strong>{t("failed.notSent")}</strong> {reason}
      </p>
      <div className="chat-failed__actions">
        <button
          type="button"
          className="chat-failed__button"
          onClick={() => {
            retrySend(t, entry);
          }}
        >
          {t("failed.retry")}
        </button>
        <button
          type="button"
          className="chat-failed__button"
          onClick={() => {
            removeOutgoing(entry);
          }}
        >
          {t("failed.remove")}
        </button>
      </div>
    </div>
  );
}
