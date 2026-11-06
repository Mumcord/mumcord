import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type CrashReport } from "../../api";
import { CrashDialog } from "../settings/crash/CrashReportsCard";
import { UploadDialog } from "../settings/crash/UploadDialog";
import { kindKey } from "../settings/crash/useCrashReports";
import "../settings/crash/crash.css";

const BUG_REPORT_URL = "https://github.com/Mumcord/mumcord/issues/new";

/** "Mumcord closed unexpectedly" after a crash in an earlier run (#568). Shown once per report. */
export function CrashNotice() {
  const { t } = useTranslation("settings");
  const [report, setReport] = useState<CrashReport | null>(null);
  const [status, setStatus] = useState<{ kind: "success" | "error"; text: string } | null>(null);
  const [uploading, setUploading] = useState(false);

  useEffect(() => {
    let active = true;
    getApi()
      .crashTakeUnseen()
      .then(
        (unseen) => {
          // The newest one speaks for all; the others are in Settings.
          if (active) setReport(unseen[0] ?? null);
        },
        (error: unknown) => {
          console.error("Reading new crash reports failed", error);
        },
      );
    return () => {
      active = false;
    };
  }, []);

  if (report === null) return null;
  if (uploading) {
    return (
      <UploadDialog
        report={report}
        onClose={() => {
          setUploading(false);
        }}
        onSent={() => {
          setUploading(false);
          setStatus({ kind: "success", text: t("crashReports.uploadDone") });
        }}
      />
    );
  }
  const fail = (what: string) => (error: unknown) => {
    console.error(`${what} failed`, error);
    setStatus({ kind: "error", text: t("crashReports.actionFailed") });
  };

  return (
    <CrashDialog
      className="cr-notice"
      role="alertdialog"
      title={t("crashReports.noticeTitle")}
      onClose={() => {
        setReport(null);
      }}
      actions={(close) => (
        <>
          <button type="button" className="settings-button cr-left" onClick={close}>
            {t("crashReports.close")}
          </button>
          <button
            type="button"
            className="settings-button"
            onClick={() => {
              getApi().crashOpenFolder().catch(fail("Opening the crash folder"));
            }}
          >
            {t("crashReports.openFolder")}
          </button>
          <button
            type="button"
            className="settings-button"
            onClick={() => {
              getApi().openLink(BUG_REPORT_URL).catch(fail("Opening the bug report page"));
            }}
          >
            {t("crashReports.reportBug")}
          </button>
          <button
            type="button"
            className="settings-button"
            onClick={() => {
              setUploading(true);
            }}
          >
            {t("crashReports.upload")}
          </button>
          <button
            type="button"
            className="settings-button settings-button--primary cr-iconbtn"
            onClick={() => {
              getApi()
                .crashRead(report.id)
                .then((text) => getApi().copyText(text))
                .then(() => {
                  setStatus({ kind: "success", text: t("crashReports.noticeCopied") });
                })
                .catch(fail("Copying the crash report"));
            }}
          >
            {t("crashReports.copyForBug")}
          </button>
        </>
      )}
    >
      <p>{t("crashReports.noticeText")}</p>
      <div className="cr-file">
        <span>{report.id}</span>
        <small>
          {t(`crashReports.${kindKey(report)}`)} · v{report.version} · {report.cause}
        </small>
      </div>
      <p
        role="status"
        className={`cr-notice-status ${status?.kind === "error" ? "settings-error" : "settings-success"}`}
      >
        {status?.text}
      </p>
    </CrashDialog>
  );
}
