import { useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi } from "../../api";
import { useServerState } from "../channels";
import { Dialog } from "../servers/Dialog";
import { movablePeople } from "./channelModel";
import "./channel-header.css";

/** "Move users here…" (issue #172): pick people from other channels and move them in. */
export function MoveUsersDialog({
  channelId,
  onClose,
}: {
  channelId: number;
  onClose: () => void;
}) {
  const { t } = useTranslation("channelheader");
  const server = useServerState();
  const [chosen, setChosen] = useState<ReadonlySet<number>>(new Set());
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);
  const channel = server.channels.get(channelId);
  const people = movablePeople(server.users, server.channels, server.selfSession, channelId);

  const toggle = (session: number) => {
    setChosen((current) => {
      const next = new Set(current);
      if (!next.delete(session)) next.add(session);
      return next;
    });
  };

  const move = async () => {
    const serverId = server.serverId;
    if (serverId === null) return;
    setBusy(true);
    setFailed(false);
    const results = await Promise.allSettled(
      [...chosen].map((session) => getApi().personMove(serverId, session, channelId)),
    );
    setBusy(false);
    const errors = results.filter((r) => r.status === "rejected");
    if (errors.length === 0) {
      onClose();
      return;
    }
    console.error("Moving people failed", errors.length);
    setFailed(true);
  };

  return (
    <Dialog
      className="move-users"
      title={t("moveDialog.title", { channel: channel?.name ?? "" })}
      onClose={onClose}
    >
      {people.length === 0 ? (
        <p className="move-users__empty">{t("moveDialog.empty")}</p>
      ) : (
        <fieldset className="move-users__list">
          <legend className="visually-hidden">{t("moveDialog.legend")}</legend>
          {people.map((user) => (
            <label key={user.session} className="move-users__person">
              <input
                type="checkbox"
                checked={chosen.has(user.session)}
                onChange={() => {
                  toggle(user.session);
                }}
              />
              <span className="move-users__name">{user.name}</span>
              <span className="move-users__from">{server.channels.get(user.channelId)?.name}</span>
            </label>
          ))}
        </fieldset>
      )}
      {failed && (
        <p className="move-users__error" role="alert">
          {t("moveDialog.failed")}
        </p>
      )}
      <div className="move-users__footer">
        <button type="button" className="button" onClick={onClose}>
          {t("moveDialog.cancel")}
        </button>
        <button
          type="button"
          className="button button--primary"
          disabled={busy || chosen.size === 0}
          onClick={() => {
            void move();
          }}
        >
          {t("moveDialog.submit")}
        </button>
      </div>
    </Dialog>
  );
}
