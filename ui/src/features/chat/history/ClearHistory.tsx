import { useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type Conversation } from "../../../api";
import { useServerState } from "../../channels";
import { clearConversation } from "../chatStore";
import "./history.css";

/** "Clear history" button of the open conversation, with a confirmation dialog. */
export function ClearHistory({ conversation }: { conversation: Conversation }) {
  const { t } = useTranslation("follow");
  const { serverId } = useServerState();
  const [open, setOpen] = useState(false);
  const opener = useRef<HTMLButtonElement>(null);

  return (
    <>
      <button
        ref={opener}
        type="button"
        className="follow-toggle"
        disabled={serverId === null}
        onClick={() => {
          setOpen(true);
        }}
      >
        {t("history.clear")}
      </button>
      {open && (
        <ClearHistoryDialog
          conversation={conversation}
          onClose={() => {
            setOpen(false);
            opener.current?.focus();
          }}
        />
      )}
    </>
  );
}

/**
 * Confirms and clears the history of a conversation; also opened from the channel-name menu
 * (issue #170). The engine records the clear, so history sync does not bring it back (#119).
 */
export function ClearHistoryDialog({
  conversation,
  onClose,
}: {
  conversation: Conversation;
  /** Called after clearing or cancelling; the caller puts focus back where it belongs. */
  onClose: () => void;
}) {
  const { t } = useTranslation("follow");
  const { serverId } = useServerState();
  const [error, setError] = useState<string | null>(null);

  async function confirm() {
    if (serverId === null) return;
    try {
      await getApi().historyClear({ kind: "conversation", serverId, conversation });
      clearConversation(conversation);
      onClose();
    } catch (e) {
      console.error("Clearing history failed", e);
      setError(t("history.failed"));
    }
  }

  return <ConfirmDialog error={error} onConfirm={() => void confirm()} onCancel={onClose} />;
}

function ConfirmDialog(props: {
  error: string | null;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const { t } = useTranslation("follow");
  const titleId = useId();
  const descId = useId();
  const ref = useRef<HTMLDivElement>(null);
  const { onCancel } = props;

  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    dialog.querySelector<HTMLElement>("button")?.focus();
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.stopPropagation();
        onCancel();
      } else if (event.key === "Tab") {
        const items = [...dialog.querySelectorAll<HTMLElement>("button")];
        const first = items[0];
        const last = items[items.length - 1];
        if (!first || !last) return;
        if (event.shiftKey && document.activeElement === first) {
          event.preventDefault();
          last.focus();
        } else if (!event.shiftKey && document.activeElement === last) {
          event.preventDefault();
          first.focus();
        }
      }
    };
    dialog.addEventListener("keydown", onKeyDown);
    return () => {
      dialog.removeEventListener("keydown", onKeyDown);
    };
  }, [onCancel]);

  return (
    <div className="history-modal">
      <div
        ref={ref}
        className="history-modal__dialog"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={descId}
      >
        <h2 id={titleId}>{t("history.title")}</h2>
        <p id={descId}>{t("history.body")}</p>
        {props.error && <p role="alert">{props.error}</p>}
        <div className="history-modal__buttons">
          <button type="button" onClick={props.onCancel}>
            {t("history.cancel")}
          </button>
          <button type="button" className="history-modal__danger" onClick={props.onConfirm}>
            {t("history.confirm")}
          </button>
        </div>
      </div>
    </div>
  );
}
