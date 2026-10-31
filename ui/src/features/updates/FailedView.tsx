import { useTranslation } from "react-i18next";
import type { UpdateError, UpdateOffer } from "../../api";
import { DialogHeader } from "./DialogHeader";

export interface FailedViewProps {
  offer: UpdateOffer;
  error: UpdateError;
  titleId: string;
  busy: boolean;
  onRetry: () => void;
  onClose: () => void;
}

/** A download, its checks or the install failed; the installed version still works. */
export function FailedView(props: FailedViewProps) {
  const { t } = useTranslation("appwindow");
  return (
    <>
      <DialogHeader
        titleId={props.titleId}
        title={t("updater.failed.title")}
        subtitle={t("updater.failed.subtitle", { version: props.offer.version })}
      />
      <p role="alert" className="upd-error">
        {t(`updater.failed.${props.error}`)}
      </p>
      <div className="upd-actions">
        <button type="button" className="upd-text-button" onClick={props.onClose}>
          {t("updater.failed.close")}
        </button>
        <button
          type="button"
          className="button button--primary upd-actions__main"
          disabled={props.busy}
          onClick={props.onRetry}
        >
          {t("updater.failed.retry")}
        </button>
      </div>
    </>
  );
}
