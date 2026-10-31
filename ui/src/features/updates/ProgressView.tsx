import { useTranslation } from "react-i18next";
import type { UpdateOffer, UpdateStatus } from "../../api";
import { DialogHeader } from "./DialogHeader";
import { StepList, type StepState } from "./StepList";

const MEGABYTE = 1_000_000;

export interface ProgressViewProps {
  status: UpdateStatus;
  offer: UpdateOffer;
  secondsLeft: number | null;
  titleId: string;
  onCancel: () => void;
}

/** Download, verification and installation of an update, which runs on while the dialog is closed. */
export function ProgressView({ status, offer, secondsLeft, titleId, onCancel }: ProgressViewProps) {
  const { t, i18n } = useTranslation("appwindow");
  const received = status.progress?.received ?? null;
  const total = status.progress?.total ?? null;
  const downloading = status.phase === "downloading";
  // Verifying and installing come after the whole bundle is in.
  const percent = !downloading
    ? 100
    : received !== null && total !== null && total > 0
      ? Math.min(100, Math.floor((received / total) * 100))
      : null;
  const megabytes = new Intl.NumberFormat(i18n.language, { maximumFractionDigits: 0 });
  const label = t("updater.progress.label");

  const stepState = (index: number): StepState => {
    const current = status.phase === "installing" ? 2 : status.phase === "verifying" ? 1 : 0;
    return index < current ? "done" : index === current ? "current" : "pending";
  };

  return (
    <>
      <DialogHeader
        titleId={titleId}
        title={t("updater.progress.title", { version: offer.version })}
        subtitle={t("updater.progress.subtitle")}
      />
      <div
        className="upd-bar"
        role="progressbar"
        aria-label={label}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={percent ?? undefined}
        aria-valuetext={percent === null ? t("updater.progress.unknown") : `${String(percent)}%`}
      >
        <div
          className={`upd-bar__fill${percent === null ? " upd-bar__fill--unknown" : ""}`}
          style={{ width: `${String(percent ?? 100)}%` }}
        />
      </div>
      <div className="upd-progress-text">
        <span>
          {percent !== null && `${String(percent)}%`}
          {downloading && received !== null && (
            <>
              {percent !== null && " · "}
              {total === null
                ? t("updater.progress.received", {
                    received: megabytes.format(received / MEGABYTE),
                  })
                : t("updater.progress.size", {
                    received: megabytes.format(received / MEGABYTE),
                    total: megabytes.format(total / MEGABYTE),
                  })}
            </>
          )}
        </span>
        {downloading && secondsLeft !== null && <span>{timeLeft(t, secondsLeft)}</span>}
      </div>
      <StepList
        steps={[
          { label: t("updater.steps.download"), state: stepState(0) },
          { label: t("updater.steps.verify"), state: stepState(1) },
          { label: t("updater.steps.install"), state: stepState(2) },
        ]}
      />
      <div className="upd-actions">
        <button
          type="button"
          className="button upd-actions__main"
          // Once installing started, stopping half way would leave a broken copy.
          disabled={status.phase === "installing"}
          onClick={onCancel}
        >
          {t("updater.progress.cancel")}
        </button>
      </div>
    </>
  );
}

function timeLeft(t: (key: string, options: { count: number }) => string, seconds: number) {
  return seconds < 60
    ? t("updater.progress.secondsLeft", { count: Math.max(1, seconds) })
    : t("updater.progress.minutesLeft", { count: Math.round(seconds / 60) });
}
