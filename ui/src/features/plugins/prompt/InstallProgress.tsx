import { useTranslation } from "react-i18next";
import { formatSize } from "../updates/formatSize";
import { useInstallProgress, type ActiveInstallStage } from "./installProgressStore";
import "./progress.css";

export interface InstallProgressProps {
  id: string;
  /** The plugin's display name (untrusted text, rendered as text only). */
  name: string;
  /** `slim`: a thin bar for list rows (automatic updates). */
  variant?: "full" | "slim";
}

/** Screen readers hear the download only at these steps, not on every percent. */
const ANNOUNCE_STEP = 25;

/**
 * The progress of a running install or update of one plugin: bytes downloaded, then checking the
 * signature, unpacking and starting. Renders nothing while none runs.
 */
export function InstallProgress({ id, name, variant = "full" }: InstallProgressProps) {
  const { t, i18n } = useTranslation("pluginBrowse");
  const progress = useInstallProgress(id);
  if (progress === null) return null;

  const percent = percentOf(progress);
  const text =
    progress.stage === "downloading"
      ? t("progress.downloading", {
          received: formatSize(progress.received, i18n.language),
          total: formatSize(progress.total, i18n.language),
        })
      : t(`progress.${progress.stage}`);
  const announcement =
    progress.stage === "downloading"
      ? t("progress.announce", {
          name,
          percent: Math.floor(percent / ANNOUNCE_STEP) * ANNOUNCE_STEP,
        })
      : `${name}: ${text}`;

  return (
    <div className={`plugin-progress plugin-progress--${variant}`}>
      <div
        className="plugin-progress__bar"
        role="progressbar"
        aria-label={t("progress.label", { name })}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={percent}
        aria-valuetext={`${String(percent)}% · ${text}`}
      >
        <div className="plugin-progress__fill" style={{ width: `${String(percent)}%` }} />
      </div>
      <span className="plugin-progress__text" aria-hidden="true">
        {text}
      </span>
      <span className="plugin-progress__sr-only" role="status" aria-live="polite">
        {announcement}
      </span>
    </div>
  );
}

function percentOf(progress: ActiveInstallStage): number {
  if (progress.stage !== "downloading") return 100;
  if (progress.total === 0) return 0;
  return Math.min(100, Math.floor((progress.received / progress.total) * 100));
}
