import { useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi } from "../../api";
import { useServerState } from "../channels";
import { Dialog } from "../servers/Dialog";
import { deleteImpact, refusalText } from "./channelAdminModel";
import "./channeladmin.css";

/**
 * "Delete channel?" (FR-10.25): names the channel, how many sub-channels go with it and how many
 * people the server moves to the parent. One click, no typing the name.
 */
export function DeleteChannelDialog({
  channelId,
  onClose,
}: {
  channelId: number;
  onClose: () => void;
}) {
  const { t } = useTranslation("admin");
  const server = useServerState();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const channel = server.channels.get(channelId);
  const parent = channel?.parentId == null ? undefined : server.channels.get(channel.parentId);
  const { subChannels, people } = deleteImpact(server.channels, server.users, channelId);

  const remove = async () => {
    if (server.serverId === null) return;
    setBusy(true);
    setError(null);
    try {
      const result = await getApi().channelRemove(server.serverId, channelId);
      const refusal = refusalText(t, result);
      if (refusal === null) onClose();
      else setError(refusal);
    } catch (failure) {
      console.error("Deleting the channel failed", failure);
      setError(t("channel.failed"));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      role="alertdialog"
      className="channel-confirm"
      title={t("channel.deleteTitle", { channel: channel?.name ?? "" })}
      onClose={onClose}
    >
      {subChannels > 0 && <p>{t("channel.deleteSubs", { count: subChannels })}</p>}
      {people > 0 && (
        <p>{t("channel.deletePeople", { count: people, parent: parent?.name ?? "" })}</p>
      )}
      {subChannels === 0 && people === 0 && <p>{t("channel.deleteEmpty")}</p>}
      {error !== null && (
        <p role="alert" className="channel-window__error">
          {error}
        </p>
      )}
      <div className="servers-dialog__actions">
        <button type="button" className="button" onClick={onClose}>
          {t("channel.cancel")}
        </button>
        <button
          type="button"
          className="button servers-button--danger"
          disabled={busy}
          onClick={() => {
            void remove();
          }}
        >
          {t("channel.deleteConfirm")}
        </button>
      </div>
    </Dialog>
  );
}
