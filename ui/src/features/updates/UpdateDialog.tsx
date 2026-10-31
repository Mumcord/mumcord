import { useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { UpdateStatus } from "../../api";
import { AvailableView } from "./AvailableView";
import { FailedView } from "./FailedView";
import { ProgressView } from "./ProgressView";
import { ReadyView } from "./ReadyView";
import "./updates.css";

/** What the dialog asks the updater to do; every call rejects when it did not work. */
export interface UpdateActions {
  download: () => Promise<void>;
  cancel: () => Promise<void>;
  restart: () => Promise<void>;
  skip: (version: string) => Promise<void>;
  openReleasePage: (url: string) => void;
}

export interface UpdateDialogProps {
  status: UpdateStatus;
  secondsLeft: number | null;
  /** Servers the user is connected to; the restart drops and restores these connections. */
  connectedServers: number;
  actions: UpdateActions;
  onClose: () => void;
}

/**
 * The update dialog (FR-3.145): the offer with its notes, the download, the finished update or
 * what went wrong. Closing it never stops a running download. Rendered only while open.
 */
export function UpdateDialog(props: UpdateDialogProps) {
  const { t } = useTranslation("appwindow");
  const { status, actions } = props;
  const ref = useRef<HTMLDialogElement>(null);
  const titleId = useId();
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);
  const onCloseRef = useRef(props.onClose);
  useEffect(() => {
    onCloseRef.current = props.onClose;
  }, [props.onClose]);

  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog.showModal();
    // Start on the main action rather than on the first version's toggle.
    dialog.querySelector<HTMLElement>(".upd-actions__main:not(:disabled)")?.focus();
    const onCancel = (event: Event) => {
      event.preventDefault();
      onCloseRef.current();
    };
    dialog.addEventListener("cancel", onCancel);
    return () => {
      dialog.removeEventListener("cancel", onCancel);
      if (dialog.open) dialog.close();
      if (opener?.isConnected) opener.focus();
    };
  }, []);

  const kind = viewKind(status);
  // A new view replaces the button that had focus; keep the keyboard inside the dialog.
  useEffect(() => {
    const dialog = ref.current;
    if (!dialog || dialog.contains(document.activeElement)) return;
    dialog.querySelector<HTMLElement>(".upd-actions__main:not(:disabled)")?.focus();
  }, [kind]);

  const run = (action: () => Promise<void>, after?: () => void) => {
    setBusy(true);
    setFailed(false);
    action().then(
      () => {
        setBusy(false);
        after?.();
      },
      (error: unknown) => {
        console.error("An update action failed", error);
        setBusy(false);
        setFailed(true);
      },
    );
  };

  const offer = status.offer;
  if (offer === null) return null;
  const close = props.onClose;
  let view;
  switch (kind) {
    case "ready":
      view = (
        <ReadyView
          status={status}
          offer={offer}
          connectedServers={props.connectedServers}
          titleId={titleId}
          busy={busy}
          onLater={close}
          onRestart={() => {
            run(actions.restart);
          }}
        />
      );
      break;
    case "progress":
      view = (
        <ProgressView
          status={status}
          offer={offer}
          secondsLeft={props.secondsLeft}
          titleId={titleId}
          onCancel={() => {
            run(actions.cancel);
          }}
        />
      );
      break;
    case "failed":
      view = (
        <FailedView
          offer={offer}
          error={status.error ?? "install"}
          titleId={titleId}
          busy={busy}
          onRetry={() => {
            run(actions.download);
          }}
          onClose={close}
        />
      );
      break;
    case "available":
      view = (
        <AvailableView
          status={status}
          offer={offer}
          titleId={titleId}
          busy={busy}
          onDownload={() => {
            run(actions.download);
          }}
          onSkip={() => {
            run(() => actions.skip(offer.version), close);
          }}
          onLater={close}
          onOpenReleasePage={() => {
            actions.openReleasePage(offer.url);
          }}
        />
      );
  }

  return (
    <dialog ref={ref} className="upd-dialog" aria-modal="true" aria-labelledby={titleId}>
      {view}
      {failed && (
        <p role="alert" className="upd-error">
          {t("updater.actionFailed")}
        </p>
      )}
    </dialog>
  );
}

function viewKind(status: UpdateStatus): "available" | "progress" | "ready" | "failed" {
  switch (status.phase) {
    case "ready":
      return "ready";
    case "downloading":
    case "verifying":
    case "installing":
      return "progress";
    case "failed":
      return status.error === null ? "available" : "failed";
    default:
      return "available";
  }
}
