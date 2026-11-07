import { useEffect, useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type CrashReport } from "../../../api";
import { CrashDialog } from "./CrashReportsCard";
import { formatWhen } from "./useCrashReports";

/**
 * The confirmation before one crash report is sent (#573): it shows exactly what goes out,
 * says plainly that the system details can identify the computer, and sends only on "Send".
 */
export function UploadDialog({
  report,
  onClose,
  onSent,
}: {
  report: CrashReport;
  onClose: () => void;
  onSent: () => void;
}) {
  const { t, i18n } = useTranslation("settings");
  const commentId = useId();
  const [comment, setComment] = useState("");
  const [preview, setPreview] = useState("");
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    getApi()
      .crashUploadPreview(report.id, comment)
      .then(
        (text) => {
          if (active) setPreview(text);
        },
        (failure: unknown) => {
          console.error("Reading the crash report for the upload failed", failure);
        },
      );
    return () => {
      active = false;
    };
  }, [report.id, comment]);

  const send = async () => {
    setSending(true);
    setError(null);
    try {
      await getApi().crashUpload(report.id, comment);
      onSent();
    } catch (failure: unknown) {
      console.error("Uploading the crash report failed", failure);
      setError(t("crashReports.uploadFailed"));
      setSending(false);
    }
  };

  return (
    <CrashDialog
      className="cr-dialog cr-upload"
      title={t("crashReports.uploadTitle")}
      onClose={onClose}
      actions={(close) => (
        <>
          <button type="button" className="settings-button cr-left" onClick={close}>
            {t("crashReports.cancel")}
          </button>
          <button
            type="button"
            className="settings-button settings-button--primary"
            disabled={sending}
            onClick={() => void send()}
          >
            {sending ? t("crashReports.uploadSending") : t("crashReports.uploadSend")}
          </button>
        </>
      )}
    >
      <p className="settings-hint cr-tight">
        {formatWhen(report, i18n.language)} · <code className="cr-code">{report.id}</code>
      </p>
      <p className="cr-section-label cr-label">{t("crashReports.uploadWhat")}</p>
      <textarea
        className="cr-pre cr-pre--short"
        readOnly
        value={preview}
        aria-label={t("crashReports.reportText")}
      />
      <div className="cr-comment">
        <label htmlFor={commentId}>{t("crashReports.uploadComment")}</label>
        <textarea
          id={commentId}
          className="settings-input cr-textarea"
          rows={3}
          value={comment}
          placeholder={t("crashReports.uploadCommentPlaceholder")}
          onChange={(event) => {
            setComment(event.target.value);
          }}
        />
      </div>
      <p className="settings-hint cr-tight cr-privacy">{t("crashReports.uploadWhere")}</p>
      <p className="cr-shared" role="note">
        <strong>{t("crashReports.uploadSharedLabel")}</strong> {t("crashReports.uploadShared")}
      </p>
      <p role="status" className="settings-error">
        {error}
      </p>
    </CrashDialog>
  );
}
