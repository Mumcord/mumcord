import { useEffect, useEffectEvent, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type CloseChoice } from "../../api";
import "./app.css";

/**
 * "Minimize to tray or quit?" (FR-1.38). The engine emits `closeRequested` when the window is
 * closed while connected and the close behaviour is "ask"; the answer goes back via
 * `closeChoice`. Escape or cancelling keeps the window open and connected.
 */
export function CloseDialog() {
  const [open, setOpen] = useState(false);

  useEffect(
    () =>
      getApi().on("closeRequested", () => {
        setOpen(true);
      }),
    [],
  );

  if (!open) return null;
  return (
    <CloseDialogContent
      onDone={() => {
        setOpen(false);
      }}
    />
  );
}

function CloseDialogContent({ onDone }: { onDone: () => void }) {
  const { t } = useTranslation("appwindow");
  const titleId = useId();
  const descriptionId = useId();
  const ref = useRef<HTMLDialogElement>(null);
  const firstRef = useRef<HTMLButtonElement>(null);
  const [remember, setRemember] = useState(false);
  const [failed, setFailed] = useState(false);
  const cancel = useEffectEvent(() => {
    onDone();
  });

  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog.showModal();
    firstRef.current?.focus();
    const onCancel = (event: Event) => {
      event.preventDefault();
      cancel();
    };
    dialog.addEventListener("cancel", onCancel);
    return () => {
      dialog.removeEventListener("cancel", onCancel);
      if (dialog.open) dialog.close();
      if (opener?.isConnected) opener.focus();
    };
  }, []);

  const choose = (choice: CloseChoice) => {
    getApi()
      .closeChoice(choice, remember)
      .then(onDone, (error: unknown) => {
        console.error("Answering the close question failed", error);
        setFailed(true);
      });
  };

  return (
    <dialog
      ref={ref}
      className="app-dialog"
      aria-modal="true"
      aria-labelledby={titleId}
      aria-describedby={descriptionId}
    >
      <h2 id={titleId} className="app-dialog__title">
        {t("close.title")}
      </h2>
      <p id={descriptionId}>{t("close.description")}</p>
      <label className="app-dialog__remember">
        <input
          type="checkbox"
          checked={remember}
          onChange={(event) => {
            setRemember(event.target.checked);
          }}
        />
        {t("close.remember")}
      </label>
      {failed && (
        <p role="alert" className="app-dialog__error">
          {t("close.failed")}
        </p>
      )}
      <div className="app-dialog__actions">
        <button
          ref={firstRef}
          type="button"
          className="button"
          onClick={() => {
            choose("quit");
          }}
        >
          {t("close.quit")}
        </button>
        <button
          type="button"
          className="button button--primary"
          onClick={() => {
            choose("minimizeToTray");
          }}
        >
          {t("close.minimize")}
        </button>
      </div>
    </dialog>
  );
}
