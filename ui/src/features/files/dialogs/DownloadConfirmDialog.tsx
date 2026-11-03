import { useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type FileCard } from "../../../api";
import { formatBytes } from "../cards/fileFormat";
import { FileDialog } from "./FileDialog";

interface DownloadConfirmDialogProps {
  card: FileCard;
  onConfirm: () => void;
  onClose: () => void;
}

/**
 * The confirmation every download needs (FR-7.22), which can never be switched off: name, size,
 * sender, the full checksum and the save folder. Download has the focus, so Enter downloads and
 * Escape cancels.
 */
export function DownloadConfirmDialog({ card, onConfirm, onClose }: DownloadConfirmDialogProps) {
  const { t, i18n } = useTranslation("files");
  const confirmRef = useRef<HTMLButtonElement>(null);
  const listId = useId();
  const [folder, setFolder] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    getApi()
      .filesSaveFolder()
      .then(
        (path) => {
          if (active) setFolder(path);
        },
        (error: unknown) => {
          console.error("Reading the download folder failed", error);
        },
      );
    return () => {
      active = false;
    };
  }, []);

  const rows: [string, string][] = [
    [t("dialog.confirmName"), card.name],
    [t("dialog.confirmSize"), formatBytes(card.size, i18n.language)],
    [t("dialog.confirmSender"), card.senderName],
    [t("dialog.confirmChecksum"), card.sha256],
    [t("dialog.confirmFolder"), folder ?? "…"],
  ];

  return (
    <FileDialog
      title={t("dialog.confirmTitle")}
      initialFocus={confirmRef}
      describedBy={listId}
      onClose={onClose}
    >
      <dl id={listId} className="file-dialog__facts">
        {rows.map(([label, value], index) => (
          <div key={label} className="file-dialog__fact">
            <dt>{label}</dt>
            <dd className={index === 3 ? "file-dialog__hash" : undefined}>{value}</dd>
          </div>
        ))}
      </dl>
      <div className="app-dialog__actions">
        <button type="button" className="button" onClick={onClose}>
          {t("dialog.cancel")}
        </button>
        <button
          ref={confirmRef}
          type="button"
          className="button button--primary"
          onClick={onConfirm}
        >
          {t("dialog.confirm")}
        </button>
      </div>
    </FileDialog>
  );
}
