import { useEffect, useEffectEvent, useId, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type CrashReport } from "../../../api";
import { Group } from "../components/Section";
import { errorText } from "../errors";
import { pickSavePath } from "../fileDialogs";
import { UploadDialog } from "./UploadDialog";
import { formatWhen, kindKey, useCrashReports } from "./useCrashReports";
import "./crash.css";

type Status = { kind: "success" | "error"; text: string } | null;
type Confirm = { kind: "one"; report: CrashReport } | { kind: "all" } | null;

const CopyIcon = () => (
  <svg
    viewBox="0 0 24 24"
    width="14"
    height="14"
    fill="none"
    stroke="currentColor"
    strokeWidth="2"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <rect x="9" y="9" width="13" height="13" rx="2" />
    <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
  </svg>
);

const TrashIcon = () => (
  <svg
    viewBox="0 0 24 24"
    width="14"
    height="14"
    fill="none"
    stroke="currentColor"
    strokeWidth="2"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <path d="M3 6h18M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6M10 11v6M14 11v6" />
  </svg>
);

/** Settings → Advanced & Developer: the saved crash reports (#568). Nothing is uploaded. */
export function CrashReportsCard() {
  const { t, i18n } = useTranslation("settings");
  const crashes = useCrashReports();
  const [status, setStatus] = useState<Status>(null);
  const [details, setDetails] = useState<{ report: CrashReport; text: string } | null>(null);
  const [confirm, setConfirm] = useState<Confirm>(null);
  const [uploadFor, setUploadFor] = useState<CrashReport | null>(null);
  const reports = crashes.reports ?? [];
  const when = (report: CrashReport) => formatWhen(report, i18n.language);

  const run = async (action: () => Promise<unknown>, success: string, what: string) => {
    setStatus(null);
    try {
      await action();
      setStatus({ kind: "success", text: success });
    } catch (error: unknown) {
      console.error(`${what} failed`, error);
      setStatus({ kind: "error", text: errorText(t, error) });
    }
  };

  const copy = (report: CrashReport) =>
    run(
      async () => getApi().copyText(await crashes.read(report.id)),
      t("crashReports.copied"),
      "Copying a crash report",
    );

  const exportReports = async (ids: string[]) => {
    const path = await pickSavePath(
      "mumcord-crash-reports.zip",
      { name: t("crashReports.fileType"), extensions: ["zip"] },
      t("crashReports.exportAll"),
    );
    if (path === null) return;
    await run(
      () => crashes.exportTo(ids, path),
      t("crashReports.exported", { path }),
      "Exporting crash reports",
    );
  };

  const showDetails = async (report: CrashReport) => {
    try {
      setDetails({ report, text: await crashes.read(report.id) });
    } catch (error: unknown) {
      console.error("Reading a crash report failed", error);
      setStatus({ kind: "error", text: errorText(t, error) });
    }
  };

  return (
    <Group
      title={t("crashReports.title")}
      icon="alert"
      wide
      badge={
        reports.length > 0 ? (
          <span className="plugins-tab__count cr-count">{reports.length}</span>
        ) : undefined
      }
      info={t("crashReports.info")}
      actions={
        <>
          <button
            type="button"
            className="settings-button"
            onClick={() => void run(crashes.openFolder, "", "Opening the crash folder")}
          >
            {t("crashReports.openFolder")}
          </button>
          {reports.length > 0 && (
            <>
              <button
                type="button"
                className="settings-button"
                onClick={() => void exportReports([])}
              >
                {t("crashReports.exportAll")}
              </button>
              <button
                type="button"
                className="settings-button settings-button--danger"
                onClick={() => {
                  setConfirm({ kind: "all" });
                }}
              >
                {t("crashReports.deleteAll")}
              </button>
            </>
          )}
        </>
      }
    >
      <p className="settings-hint cr-tight">{t("crashReports.info")}</p>
      {crashes.reports !== null && reports.length === 0 ? (
        <div className="cr-empty">
          <span className="cr-empty__icon" aria-hidden="true">
            ✓
          </span>
          <div>
            <p className="cr-empty__title">{t("crashReports.emptyTitle")}</p>
            <p className="settings-hint">{t("crashReports.emptyText")}</p>
          </div>
        </div>
      ) : (
        <ul className="cr-list" aria-label={t("crashReports.listLabel")}>
          {reports.map((report) => (
            <li key={report.id} className="cr-row">
              <div className="cr-row__main">
                <span className="cr-row__when">{when(report)}</span>
                <span className="cr-chip">v{report.version}</span>
                <span
                  className={
                    report.kind === "panic" ? "cr-chip cr-chip--plugin" : "cr-chip cr-chip--warn"
                  }
                >
                  {t(`crashReports.${kindKey(report)}`)}
                </span>
                {report.uploaded && (
                  <span className="cr-chip cr-chip--ok">{t("crashReports.uploaded")}</span>
                )}
              </div>
              <span className="cr-row__cause">{report.cause}</span>
              <div className="cr-row__actions">
                <button
                  type="button"
                  className="settings-button cr-small cr-iconbtn"
                  aria-label={t("crashReports.copyLabel", { when: when(report) })}
                  onClick={() => void copy(report)}
                >
                  <CopyIcon />
                  {t("crashReports.copy")}
                </button>
                <button
                  type="button"
                  className="settings-button cr-small"
                  aria-label={t("crashReports.detailsLabel", { when: when(report) })}
                  onClick={() => void showDetails(report)}
                >
                  {t("crashReports.details")}
                </button>
                <button
                  type="button"
                  className="settings-button cr-small"
                  aria-label={t("crashReports.uploadLabel", { when: when(report) })}
                  onClick={() => {
                    setUploadFor(report);
                  }}
                >
                  {t("crashReports.upload")}
                </button>
                <button
                  type="button"
                  className="settings-button settings-button--danger cr-small cr-iconbtn"
                  aria-label={t("crashReports.deleteLabel", { when: when(report) })}
                  onClick={() => {
                    setConfirm({ kind: "one", report });
                  }}
                >
                  <TrashIcon />
                  {t("crashReports.delete")}
                </button>
              </div>
            </li>
          ))}
        </ul>
      )}
      <p
        role="status"
        className={`cr-status ${status?.kind === "error" ? "settings-error" : "settings-success"}`}
      >
        {status?.text}
      </p>

      {details !== null && (
        <CrashDialog
          className="cr-dialog"
          title={t("crashReports.detailsTitle", { when: when(details.report) })}
          onClose={() => {
            setDetails(null);
          }}
          actions={(close) => (
            <>
              <button type="button" className="settings-button cr-left" onClick={close}>
                {t("crashReports.close")}
              </button>
              <button
                type="button"
                className="settings-button settings-button--danger cr-iconbtn"
                onClick={() => {
                  setConfirm({ kind: "one", report: details.report });
                  close();
                }}
              >
                <TrashIcon />
                {t("crashReports.delete")}
              </button>
              <button
                type="button"
                className="settings-button"
                onClick={() => void exportReports([details.report.id])}
              >
                {t("crashReports.export")}
              </button>
              <button
                type="button"
                className="settings-button"
                onClick={() => {
                  setUploadFor(details.report);
                  close();
                }}
              >
                {t("crashReports.upload")}
              </button>
              <button
                type="button"
                className="settings-button settings-button--primary cr-iconbtn"
                onClick={() => void copy(details.report)}
              >
                <CopyIcon />
                {t("crashReports.copyForBug")}
              </button>
            </>
          )}
        >
          <textarea
            className="cr-pre"
            readOnly
            value={details.text}
            aria-label={t("crashReports.reportText")}
          />
          <p className="settings-hint">{t("crashReports.privacy")}</p>
        </CrashDialog>
      )}

      {uploadFor !== null && (
        <UploadDialog
          report={uploadFor}
          onClose={() => {
            setUploadFor(null);
          }}
          onSent={() => {
            setUploadFor(null);
            setStatus({ kind: "success", text: t("crashReports.uploadDone") });
            void crashes.reload();
          }}
        />
      )}

      {confirm !== null && (
        <CrashDialog
          className="cr-confirm"
          role="alertdialog"
          title={
            confirm.kind === "all"
              ? t("crashReports.confirmDeleteAllTitle")
              : t("crashReports.confirmDeleteTitle")
          }
          onClose={() => {
            setConfirm(null);
          }}
          actions={(close) => (
            <>
              <button type="button" className="settings-button" onClick={close}>
                {t("crashReports.cancel")}
              </button>
              <button
                type="button"
                className="settings-button settings-button--danger"
                onClick={() => {
                  const target = confirm;
                  close();
                  if (target.kind === "all") {
                    void run(
                      crashes.removeAll,
                      t("crashReports.deletedAll"),
                      "Deleting crash reports",
                    );
                  } else {
                    void run(
                      () => crashes.remove(target.report.id),
                      t("crashReports.deleted"),
                      "Deleting a crash report",
                    );
                  }
                }}
              >
                {confirm.kind === "all" ? t("crashReports.deleteAll") : t("crashReports.delete")}
              </button>
            </>
          )}
        >
          <p>
            {confirm.kind === "all"
              ? t("crashReports.confirmDeleteAllText", { count: reports.length })
              : t("crashReports.confirmDeleteText", { when: when(confirm.report) })}
          </p>
        </CrashDialog>
      )}
    </Group>
  );
}

/** A modal dialog that returns focus to where it was opened from. */
export function CrashDialog({
  title,
  className,
  role,
  onClose,
  actions,
  children,
}: {
  title: string;
  className: string;
  role?: "alertdialog";
  onClose: () => void;
  actions: (close: () => void) => ReactNode;
  children: ReactNode;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  const titleId = useId();
  const cancel = useEffectEvent(() => {
    onClose();
  });

  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog.showModal();
    const onCancel = (event: Event) => {
      event.preventDefault();
      cancel();
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
      className={`app-dialog ${className}`}
      role={role}
      aria-modal="true"
      aria-labelledby={titleId}
    >
      <h2 id={titleId} className="app-dialog__title">
        {title}
      </h2>
      {children}
      <div className="app-dialog__actions">{actions(onClose)}</div>
    </dialog>
  );
}
