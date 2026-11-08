/**
 * The "Edit server notice" dialog (Epic 4, issue #258, FR-10.29): the Root channel's description,
 * edited with the same description editor and save rules as the Channel window (issue #250).
 * Only the description is sent; the server still decides and a refusal is shown here (FR-10.59).
 */
import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type ChannelResult } from "../../api";
import { refusalText } from "../channeladmin/channelAdminModel";
import { Dialog } from "../servers/Dialog";
import { RichCommentEditor } from "../userpanel/RichCommentEditor";
import { commentToSend } from "../userpanel/commentHtml";
import type { EditorLimits } from "../userpanel/richEditorModel";
import "../admin/admin.css";
import "../channeladmin/channeladmin.css";
import "../userpanel/userpanel.css";

interface ServerNoticeDialogProps {
  serverId: number;
  /** Sanitized description HTML to start from; empty for none. */
  initialHtml: string;
  /** The server's description length limits, as the Channel window uses them. */
  limits: EditorLimits;
  onClose: () => void;
}

export function ServerNoticeDialog({
  serverId,
  initialHtml,
  limits,
  onClose,
}: ServerNoticeDialogProps) {
  const { t } = useTranslation("admin");
  const ids = { description: useId(), hint: useId() };
  const [draft, setDraft] = useState(initialHtml);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const dirty = commentToSend(draft) !== commentToSend(initialHtml);

  const save = async () => {
    if (busy || !dirty) return;
    setBusy(true);
    setError(null);
    try {
      // Same rule as the Channel window: only the changed description is sent ("" clears it).
      const result: ChannelResult = await getApi().channelEdit(serverId, 0, {
        name: null,
        description: commentToSend(draft),
        parentId: null,
        position: null,
        maxUsers: null,
        linksAdd: [],
        linksRemove: [],
      });
      const refusal = refusalText(t, result);
      if (refusal !== null) {
        setError(refusal);
        return;
      }
      onClose();
    } catch (failure) {
      console.error("Saving the server notice failed", failure);
      setError(t("channel.failed"));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      className="channel-window"
      title={t("serverInfo.noticeEditTitle")}
      onClose={onClose}
      closeLabel={t("channel.close")}
    >
      <div className="channel-window__body">
        <div className="channel-window__field admin-field">
          <div className="channel-window__label-row">
            <span id={ids.description}>{t("serverInfo.notice")}</span>
          </div>
          <RichCommentEditor
            labelId={ids.description}
            describedBy={ids.hint}
            initialHtml={initialHtml}
            loadKey={0}
            disabled={busy}
            purpose="description"
            limits={limits}
            onChange={setDraft}
          />
          <p id={ids.hint} className="channel-window__hint">
            {t("channel.descriptionHint")}
          </p>
        </div>
      </div>
      {error !== null && (
        <p role="alert" className="channel-window__error">
          {error}
        </p>
      )}
      <div className="channel-window__footer">
        <span className="channel-window__status" aria-live="polite">
          {dirty && !busy ? t("channel.unsaved") : ""}
        </span>
        <button type="button" className="button" onClick={onClose}>
          {t("channel.cancel")}
        </button>
        <button
          type="button"
          className="button button--primary"
          disabled={busy || !dirty}
          onClick={() => {
            void save();
          }}
        >
          {busy ? t("channel.saving") : t("channel.save")}
        </button>
      </div>
    </Dialog>
  );
}
