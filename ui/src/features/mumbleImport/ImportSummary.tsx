import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { MumbleImportDone, Settings } from "../../api";
import { useDate, useKeyName } from "./format";

export interface ImportOutcome {
  done: MumbleImportDone;
  /** Settings the import changed, and their values before, for the undo. */
  patch: Partial<Settings>;
  before: Partial<Settings>;
  /** Device names to show for a device patch. */
  deviceNames: string[];
  settingsFailed: boolean;
}

interface SummaryRow {
  key: string;
  title: string;
  detail: ReactNode;
  status: "done" | "duplicate" | "failed";
}

function useRows({ done, patch, deviceNames, settingsFailed }: ImportOutcome): SummaryRow[] {
  const { t } = useTranslation("mumbleImport");
  const keyName = useKeyName();
  const date = useDate();
  const rows: SummaryRow[] = [];
  if (done.added.length > 0) {
    rows.push({
      key: "added",
      title: t("summary.added", { count: done.added.length }),
      detail: done.added.map((s) => s.label).join(", "),
      status: "done",
    });
  }
  if (done.skipped.length > 0) {
    rows.push({
      key: "skipped",
      title: t("summary.skipped", { count: done.skipped.length }),
      detail: t("summary.skippedDetail", {
        count: done.skipped.length,
        names: done.skipped.join(", "),
      }),
      status: "duplicate",
    });
  }
  if (done.passwordsSaved > 0) {
    rows.push({
      key: "passwords",
      title: t("summary.passwords", { count: done.passwordsSaved }),
      detail: t("summary.passwordsDetail"),
      status: "done",
    });
  }
  if (done.identity) {
    const parts = [t("identity.named", { name: done.identity.name })];
    if (done.identityValidUntil !== null)
      parts.push(t("identity.validUntil", { date: date(done.identityValidUntil) }));
    parts.push(
      done.identity.isDefault ? t("summary.identityDefault") : t("summary.identityServers"),
    );
    rows.push({
      key: "identity",
      title: t("summary.identity"),
      detail: parts.join(" · "),
      status: "done",
    });
  } else if (done.identitySkipped) {
    rows.push({
      key: "identity",
      title: t("summary.identitySkipped"),
      detail: t("summary.identitySkippedDetail"),
      status: "duplicate",
    });
  }
  const devices = "inputDevice" in patch || "outputDevice" in patch;
  const ptt = "pttKey" in patch;
  const volume = "outputVolumePercent" in patch || ("talkMode" in patch && !ptt);
  if (devices || ptt || volume) {
    const titles: string[] = [];
    if (devices && ptt) titles.push(t("summary.devicesAndPtt"));
    else if (devices) titles.push(t("devices.title"));
    else if (ptt) titles.push(t("ptt.title"));
    if (volume) titles.push(t("volume.title"));
    const details = [...(devices ? deviceNames : [])];
    if (ptt && patch.pttKey) details.push(keyName(patch.pttKey));
    rows.push({
      key: "settings",
      title: titles.join(", "),
      detail: settingsFailed ? t("settingsFailed") : details.join(", "),
      status: settingsFailed ? "failed" : "done",
    });
  }
  return rows;
}

interface ImportSummaryProps {
  outcome: ImportOutcome;
}

/** What the import did, row by row (mockup `66-import-summary`). */
export function ImportSummary({ outcome }: ImportSummaryProps) {
  const { t } = useTranslation("mumbleImport");
  const rows = useRows(outcome);
  return (
    <ul className="mumble-import__list">
      {rows.map((row) => (
        <li key={row.key} className="mumble-import__row">
          <div className="mumble-import__main">
            <span
              className={`mumble-import__mark mumble-import__mark--${row.status}`}
              aria-hidden="true"
            >
              {row.status === "failed" ? "!" : "✓"}
            </span>
            <div className="mumble-import__text">
              <span className="mumble-import__title">{row.title}</span>
              <span className="mumble-import__detail">{row.detail}</span>
            </div>
            {row.status === "done" && (
              <span className="mumble-import__status">
                <span aria-hidden="true">✓ </span>
                {t("summary.done")}
              </span>
            )}
            {row.status === "duplicate" && (
              <span className="mumble-import__pill">{t("summary.duplicate")}</span>
            )}
          </div>
        </li>
      ))}
    </ul>
  );
}
