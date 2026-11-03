import { useId, useRef } from "react";
import { useTranslation } from "react-i18next";
import type { FileCard } from "../../../api";
import { ProgramIcon, WarningIcon } from "../cards/fileIcons";
import { formatBytes } from "../cards/fileFormat";
import { FileDialog } from "./FileDialog";

const NAME_MARK = "\u0000";

interface RiskyFileDialogProps {
  card: FileCard;
  /** "download" replaces the plain confirmation (FR-7.22); "open" asks once more (FR-7.29). */
  action: "download" | "open";
  onConfirm: () => void;
  onClose: () => void;
}

/**
 * The warning for programs and scripts (mockup 6). It is an alert dialog and the safe answer has
 * the focus, so Enter never runs the risky action by accident.
 */
export function RiskyFileDialog({ card, action, onConfirm, onClose }: RiskyFileDialogProps) {
  const { t, i18n } = useTranslation("files");
  const safeRef = useRef<HTMLButtonElement>(null);
  const textId = useId();
  const vars = { name: card.name, sender: card.senderName };
  // The name is bold wherever the translation puts it, so the line is split at a marker.
  const [before = "", after = ""] = t("dialog.riskyFile", {
    name: NAME_MARK,
    size: formatBytes(card.size, i18n.language),
    sender: card.senderName,
  }).split(NAME_MARK);

  return (
    <FileDialog
      title={t("dialog.riskyTitle")}
      icon={
        <span className="file-dialog__warning" aria-hidden="true">
          <WarningIcon size={20} />
        </span>
      }
      role="alertdialog"
      initialFocus={safeRef}
      describedBy={textId}
      onClose={onClose}
    >
      <p className="file-dialog__file">
        <ProgramIcon />
        <span>
          {before}
          <strong>{card.name}</strong>
          {after}
        </span>
      </p>
      <p id={textId}>
        {action === "download" ? t("dialog.riskyBody", vars) : t("dialog.riskyOpenBody", vars)}
      </p>
      <p className="file-dialog__note">{t("dialog.riskyNote")}</p>
      <div className="app-dialog__actions">
        <button type="button" className="button" onClick={onConfirm}>
          {action === "download" ? t("dialog.riskyConfirm") : t("dialog.riskyOpenConfirm")}
        </button>
        <button ref={safeRef} type="button" className="button button--primary" onClick={onClose}>
          {action === "download" ? t("dialog.riskyCancel") : t("dialog.cancel")}
        </button>
      </div>
    </FileDialog>
  );
}
