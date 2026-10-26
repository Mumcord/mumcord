import { useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi } from "../../api";
import "./app.css";

/**
 * What the prompt needs from the engine. `pending` is true for an AppImage whose user has not
 * answered yet; `answer(true)` writes the menu entry, then both answers store the choice.
 */
export interface AppImageMenuPromptApi {
  pending: () => Promise<boolean>;
  answer: (add: boolean) => Promise<void>;
}

const engineApi: AppImageMenuPromptApi = {
  async pending() {
    const api = getApi();
    const [info, settings] = await Promise.all([api.appInfo(), api.settingsGet()]);
    return info.appimage && settings.appimageMenuEntry === "unset";
  },
  async answer(add) {
    const api = getApi();
    if (add) await api.appimageMenuEntryAdd();
    const settings = await api.settingsGet();
    await api.settingsUpdate({ ...settings, appimageMenuEntry: add ? "yes" : "no" });
  },
};

/**
 * One-time question for AppImage users: add Mumcord to the app menu? Mumcord never writes the
 * menu entry before the answer (FR-2.55). Escape only hides the question until the next start.
 */
export function AppImageMenuPrompt({ api = engineApi }: { api?: AppImageMenuPromptApi }) {
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
        console.error("Reading the app menu question failed", error);
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

  const answer = (add: boolean) => {
    setBusy(true);
    setFailed(false);
    api.answer(add).then(
      () => {
        setBusy(false);
        setOpen(false);
      },
      (error: unknown) => {
        console.error("Saving the app menu answer failed", error);
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
        {t("appImageMenu.title")}
      </h2>
      <p id={descriptionId}>{t("appImageMenu.description")}</p>
      {failed && (
        <p role="alert" className="app-dialog__error">
          {t("appImageMenu.failed")}
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
          {t("appImageMenu.no")}
        </button>
        <button
          type="button"
          className="button button--primary"
          disabled={busy}
          onClick={() => {
            answer(true);
          }}
        >
          {t("appImageMenu.yes")}
        </button>
      </div>
    </dialog>
  );
}
