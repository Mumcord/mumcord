import { useEffect, useEffectEvent, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import "./app.css";

/**
 * What the notice needs from the engine. `pending` is true once for users who had the window
 * overlay switched on before the update; `dismiss` stores `window_overlay_notice_shown`.
 */
export interface WindowOverlayNoticeApi {
  pending: () => Promise<boolean>;
  dismiss: () => Promise<void>;
}

/** Until the engine offers the commands there is nothing to announce. */
const noNotice: WindowOverlayNoticeApi = {
  pending: () => Promise.resolve(false),
  dismiss: () => Promise.resolve(),
};

/**
 * One-time notice that the window overlay is gone (FR-2.33, Q2.9). Either button marks it as
 * shown, so it never comes back; "Open overlay settings" also leads to the in-game overlay.
 */
export function WindowOverlayNotice({
  api = noNotice,
  onOpenSettings,
}: {
  api?: WindowOverlayNoticeApi;
  onOpenSettings: () => void;
}) {
  const [open, setOpen] = useState(false);

  useEffect(() => {
    let active = true;
    api.pending().then(
      (pending) => {
        if (active) setOpen(pending);
      },
      (error: unknown) => {
        console.error("Reading the window overlay notice failed", error);
      },
    );
    return () => {
      active = false;
    };
  }, [api]);

  if (!open) return null;
  return (
    <NoticeContent
      onDone={(openSettings) => {
        setOpen(false);
        api.dismiss().catch((error: unknown) => {
          console.error("Saving the window overlay notice failed", error);
        });
        if (openSettings) onOpenSettings();
      }}
    />
  );
}

function NoticeContent({ onDone }: { onDone: (openSettings: boolean) => void }) {
  const { t } = useTranslation("appwindow");
  const titleId = useId();
  const descriptionId = useId();
  const ref = useRef<HTMLDialogElement>(null);
  const firstRef = useRef<HTMLButtonElement>(null);
  const dismiss = useEffectEvent(() => {
    onDone(false);
  });

  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog.showModal();
    firstRef.current?.focus();
    const onCancel = (event: Event) => {
      event.preventDefault();
      dismiss();
    };
    dialog.addEventListener("cancel", onCancel);
    return () => {
      dialog.removeEventListener("cancel", onCancel);
      if (dialog.open) dialog.close();
      if (opener?.isConnected) opener.focus();
    };
  }, []);

  return (
    <dialog
      ref={ref}
      className="app-dialog"
      aria-modal="true"
      aria-labelledby={titleId}
      aria-describedby={descriptionId}
    >
      <h2 id={titleId} className="app-dialog__title">
        {t("windowOverlay.title")}
      </h2>
      <p id={descriptionId}>{t("windowOverlay.description")}</p>
      <div className="app-dialog__actions">
        <button
          ref={firstRef}
          type="button"
          className="button"
          onClick={() => {
            onDone(false);
          }}
        >
          {t("windowOverlay.dismiss")}
        </button>
        <button
          type="button"
          className="button button--primary"
          onClick={() => {
            onDone(true);
          }}
        >
          {t("windowOverlay.openSettings")}
        </button>
      </div>
    </dialog>
  );
}
