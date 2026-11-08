import { useRef } from "react";
import { useTranslation } from "react-i18next";
import { deleteImpact } from "../channeladmin/channelAdminModel";
import { Dialog } from "../servers/Dialog";
import { channelNamePath, type ChannelMovePlan } from "./channelDrag";
import { useServerState } from "./serverStore";

/**
 * "Move “Raid”?" (issue #676): asked only when a dragged channel gets a new parent. Names what
 * moves along and the path before and after. Focus starts on Move, so Enter confirms; Esc and
 * Cancel leave the tree as it was.
 */
export function MoveChannelDialog({
  plan,
  onConfirm,
  onClose,
}: {
  plan: ChannelMovePlan;
  onConfirm: () => void;
  onClose: () => void;
}) {
  const { t } = useTranslation("channels");
  const { t: tCommon } = useTranslation("common");
  const server = useServerState();
  const confirmRef = useRef<HTMLButtonElement>(null);
  const name = server.channels.get(plan.channelId)?.name ?? "";
  const { subChannels, people } = deleteImpact(server.channels, server.users, plan.channelId);
  const along = {
    channel: name,
    subChannels: t("drag.subChannels", { count: subChannels }),
    people: t("tree.people", { count: people }),
  };
  const before = channelNamePath(server.channels, plan.channelId).join(" › ");
  const after = [...channelNamePath(server.channels, plan.toParentId), name].join(" › ");
  return (
    <Dialog
      className="channel-move"
      title={t("drag.confirmTitle", { channel: name })}
      onClose={onClose}
      initialFocus={confirmRef}
    >
      <p>
        {subChannels > 0
          ? people > 0
            ? t("drag.alongAll", along)
            : t("drag.alongSubChannels", along)
          : people > 0
            ? t("drag.alongPeople", along)
            : t("drag.alongNone", along)}
      </p>
      <p className="channel-move__path">
        {before}
        <span aria-hidden="true"> → </span>
        <span className="visually-hidden"> {t("drag.pathTo")} </span>
        <b>{after}</b>
      </p>
      <div className="servers-dialog__actions">
        <button type="button" className="button" onClick={onClose}>
          {tCommon("cancel")}
        </button>
        <button
          ref={confirmRef}
          type="button"
          className="button button--primary"
          onClick={onConfirm}
        >
          {t("drag.confirm")}
        </button>
      </div>
    </Dialog>
  );
}
