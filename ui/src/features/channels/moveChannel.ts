/**
 * Sends a channel drag (issue #676) as channel edits: the dragged channel's new parent and/or
 * position first, then any sibling positions the new order needs. The tree only changes when the
 * server's updates arrive, so a refused move leaves it as it was and a toast says why.
 */
import type { TFunction } from "i18next";
import { getApi } from "../../api";
import { refusalText } from "../channeladmin/channelAdminModel";
import { noticeKey, showToast } from "../notifications/notificationStore";
import type { ChannelMovePlan } from "./channelDrag";
import { getServerStateOf } from "./serverStore";

export async function moveChannel(
  t: TFunction<"channels">,
  tAdmin: TFunction<"admin">,
  serverId: number,
  plan: ChannelMovePlan,
): Promise<void> {
  const channels = getServerStateOf(serverId).channels;
  const name = channels.get(plan.channelId)?.name ?? "";
  const parent = channels.get(plan.toParentId)?.name ?? "";
  const fail = (text: string) => {
    showToast({
      key: noticeKey(serverId, `channel-move:${String(plan.channelId)}`),
      serverId,
      conversation: { kind: "channel", channelId: plan.channelId },
      title: t("drag.failedTitle", { channel: name }),
      text,
      tone: "error",
      open: () => undefined,
    });
  };
  for (const step of plan.steps) {
    try {
      const result = await getApi().channelEdit(serverId, step.channelId, {
        name: null,
        description: null,
        parentId: step.parentId,
        position: step.position,
        maxUsers: null,
        linksAdd: [],
        linksRemove: [],
      });
      if (result.kind === "refused") {
        fail(
          step.parentId !== null && result.denial === "permission"
            ? t("drag.refusedParent", { channel: parent })
            : (refusalText(tAdmin, result) ?? ""),
        );
        return;
      }
    } catch (error) {
      console.error("Moving a channel failed", error);
      fail(tAdmin("channel.failed"));
      return;
    }
  }
}
