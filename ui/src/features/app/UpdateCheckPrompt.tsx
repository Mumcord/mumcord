import { useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi } from "../../api";
import "./app.css";

/**
 * What the prompt needs from the engine. `pending` is true for users who finished the wizard
 * before the question existed and have not answered yet; `answer` stores the choice.
 */
export interface UpdateCheckPromptApi {
  pending: () => Promise<boolean>;
  answer: (check: boolean) => Promise<void>;
}

const engineApi: UpdateCheckPromptApi = {
  async pending() {
    const settings = await getApi().settingsGet();
    return settings.wizardCompleted && settings.updateCheck === "unset";
  },
  async answer(check) {
    const api = getApi();
    const settings = await api.settingsGet();
    await api.settingsUpdate({ ...settings, updateCheck: check ? "yes" : "no" });
  },
};

/**
 * One-time question for existing users: check for new versions? Nothing is sent to GitHub
 * before the answer is yes (FR-2.56). Escape only hides the question until the next start.
 */
export function UpdateCheckPrompt({ api = engineApi }: { api?: UpdateCheckPromptApi }) {
  const { t } = useTranslation("appwindow");
  const [open, setOpen] = useState(false);
  const [failed, setFailed] = useState(false);
  const [busy, setBusy] = useState(false);
  const titleId = useId();
  const descriptionId = useId();
  const ref = useRef<HTMLDialogElement>(null);
  const firstRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    let active = true;
    api.pending().then(
      (pending) => {
        if (active) setOpen(pending);
      },
      (error: unknown) => {
        console.error("Reading the update check question failed", error);
      },
    );
    return () => {
      active = false;
    };
  }, [api]);

  useEffect(() => {
    const dialog = ref.current;
    if (!open || !dialog) return;
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog.showModal();
    firstRef.current?.focus();
    const onCancel = (event: Event) => {
      event.preventDefault();
      setOpen(false);
    };
    dialog.addEventListener("cancel", onCancel);
    return () => {
      dialog.removeEventListener("cancel", onCancel);
      if (dialog.open) dialog.close();
      if (opener?.isConnected) opener.focus();
    };
  }, [open]);

  const answer = (check: boolean) => {
    setBusy(true);
    setFailed(false);
    api.answer(check).then(
      () => {
        setBusy(false);
        setOpen(false);
      },
      (error: unknown) => {
        console.error("Saving the update check answer failed", error);
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
        {t("updateCheck.title")}
      </h2>
      <p id={descriptionId}>{t("updateCheck.description")}</p>
      {failed && (
        <p role="alert" className="app-dialog__error">
          {t("updateCheck.failed")}
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
          {t("updateCheck.no")}
        </button>
        <button
          type="button"
          className="button button--primary"
          disabled={busy}
          onClick={() => {
            answer(true);
          }}
        >
          {t("updateCheck.yes")}
        </button>
      </div>
    </dialog>
  );
}
