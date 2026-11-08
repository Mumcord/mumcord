/**
 * The server's refusals in plain words (#248): what was not allowed and where, e.g. "You may not
 * move people in Raid." A refusal a dialog already shows (kick, ban, ban list, channel password)
 * gets no second notice.
 */
import type { TFunction } from "i18next";
import type { MumcordApi, ServerDenied, Unsubscribe } from "../../api";
import { getServerStateOf } from "../channels";
import { noticeKey, showToast } from "../notifications/notificationStore";

/** The plain-language text of a refusal; the server's own text where it gave one. */
export function deniedText(t: TFunction<"admin">, denied: ServerDenied): string {
  const channelId = denied.channelId;
  const channel =
    channelId === null ? undefined : getServerStateOf(denied.serverId).channels.get(channelId);
  switch (denied.kind) {
    case "permission": {
      const right = denied.permission;
      if (right === null) return t("denied.permission");
      const action = t(`denied.right.${right}`);
      return channel === undefined
        ? t("denied.sentence", { action })
        : t("denied.sentenceIn", { action, channel: channel.name });
    }
    case "text":
    case "unknown":
      return denied.error.message === "" ? t("denied.unknown") : denied.error.message;
    default:
      return t(`denied.kind.${denied.kind}`);
  }
}

export function bindDeniedNotices(
  api: MumcordApi,
  context: () => { t: TFunction<"admin">; serverName: (serverId: number) => string | undefined },
): Unsubscribe {
  return api.on("serverDenied", (denied) => {
    if (denied.handled) return;
    const { t, serverName } = context();
    const server = serverName(denied.serverId);
    showToast({
      // The same refusal in the same place updates one toast instead of stacking up: moving a
      // whole channel into a full one makes the server refuse once per person (issue #255).
      key: noticeKey(
        denied.serverId,
        `denied:${String(denied.channelId ?? 0)}:${denied.kind}:${denied.permission ?? ""}`,
      ),
      serverId: denied.serverId,
      conversation: { kind: "channel", channelId: denied.channelId ?? 0 },
      title: server === undefined ? t("denied.title") : t("denied.titleOn", { server }),
      text: deniedText(t, denied),
      open: () => undefined,
    });
  });
}
