import { useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi } from "../../api";
import "./app.css";

/**
 * What the prompt needs from the engine. `onRequired` reports that a direct connection would
 * be used while the user has not decided yet; `answer` stores the choice.
 */
export interface DirectConsentPromptApi {
  onRequired: (handler: () => void) => () => void;
  answer: (allow: boolean) => Promise<void>;
}

const engineApi: DirectConsentPromptApi = {
  onRequired: (handler) =>
    getApi().on("directConsentRequired", () => {
      handler();
    }),
  async answer(allow) {
    await getApi().p2pConsentDecide(allow);
  },
};

/**
 * Asks once before the first direct connection to another Mumcord user (FR-2.36): it reveals
 * the IP address to that person. The engine waits for the answer, so the dialog needs an
 * explicit Yes or No; Escape does not close it.
 */
export function DirectConsentPrompt({ api = engineApi }: { api?: DirectConsentPromptApi }) {
  const { t } = useTranslation("appwindow");
  const [open, setOpen] = useState(false);
  const [failed, setFailed] = useState(false);
  const [busy, setBusy] = useState(false);
  const titleId = useId();
  const descriptionId = useId();
  const ref = useRef<HTMLDialogElement>(null);
  const firstRef = useRef<HTMLButtonElement>(null);

  useEffect(
    () =>
      api.onRequired(() => {
        setOpen(true);
      }),
    [api],
  );

  useEffect(() => {
    const dialog = ref.current;
    if (!open || !dialog) return;
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog.showModal();
    firstRef.current?.focus();
    const onCancel = (event: Event) => {
      event.preventDefault();
    };
    dialog.addEventListener("cancel", onCancel);
    return () => {
      dialog.removeEventListener("cancel", onCancel);
      if (dialog.open) dialog.close();
      if (opener?.isConnected) opener.focus();
    };
  }, [open]);

  const answer = (allow: boolean) => {
    setBusy(true);
    setFailed(false);
    api.answer(allow).then(
      () => {
        setBusy(false);
        setOpen(false);
      },
      (error: unknown) => {
        console.error("Saving the direct connection answer failed", error);
        setBusy(false);
        setFailed(true);
      },
    );
  };

  if (!open) return null;
  return (
    <dialog
      ref={ref}
      className="app-dialog"
      aria-modal="true"
      aria-labelledby={titleId}
      aria-describedby={descriptionId}
    >
      <h2 id={titleId} className="app-dialog__title">
        {t("directConsent.title")}
      </h2>
      <p id={descriptionId}>{t("directConsent.description")}</p>
      {failed && (
        <p role="alert" className="app-dialog__error">
          {t("directConsent.failed")}
        </p>
      )}
      <div className="app-dialog__actions">
        <button
          ref={firstRef}
          type="button"
          className="button"
          disabled={busy}
          onClick={() => {
            answer(false);
          }}
        >
          {t("directConsent.no")}
        </button>
        <button
          type="button"
          className="button button--primary"
          disabled={busy}
          onClick={() => {
            answer(true);
          }}
        >
          {t("directConsent.yes")}
        </button>
      </div>
    </dialog>
  );
}
