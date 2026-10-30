import { useEffect, useEffectEvent, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import "./app.css";

/** What a one-time notice needs from the engine: whether it is due, and marking it as shown. */
export interface SettingsNoticeApi {
  pending: () => Promise<boolean>;
  dismiss: () => Promise<void>;
}

/** Keys under the `appwindow` namespace with the notice's title, description and buttons. */
export type SettingsNoticeTexts = "windowOverlay" | "audioDevice";

/** Until the engine offers the commands there is nothing to announce. */
const noNotice: SettingsNoticeApi = {
  pending: () => Promise.resolve(false),
  dismiss: () => Promise.resolve(),
};

/**
 * A one-time notice that leads to a settings section. Either button marks it as shown, so it
 * never comes back; the second one also opens the settings.
 */
export function SettingsNotice({
  api = noNotice,
  texts,
  onOpenSettings,
}: {
  api?: SettingsNoticeApi | undefined;
  texts: SettingsNoticeTexts;
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
        console.error(`Reading the ${texts} notice failed`, error);
      },
    );
    return () => {
      active = false;
    };
  }, [api, texts]);

  if (!open) return null;
  return (
    <NoticeContent
      texts={texts}
      onDone={(openSettings) => {
        setOpen(false);
        api.dismiss().catch((error: unknown) => {
          console.error(`Saving the ${texts} notice failed`, error);
        });
        if (openSettings) onOpenSettings();
      }}
    />
  );
}

function NoticeContent({
  texts,
  onDone,
}: {
  texts: SettingsNoticeTexts;
  onDone: (openSettings: boolean) => void;
}) {
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
        {t(`${texts}.title`)}
      </h2>
      <p id={descriptionId}>{t(`${texts}.description`)}</p>
      <div className="app-dialog__actions">
        <button
          ref={firstRef}
          type="button"
          className="button"
          onClick={() => {
            onDone(false);
          }}
        >
          {t(`${texts}.dismiss`)}
        </button>
        <button
          type="button"
          className="button button--primary"
          onClick={() => {
            onDone(true);
          }}
        >
          {t(`${texts}.openSettings`)}
        </button>
      </div>
    </dialog>
  );
}
